//! Bounded HTTP backend for the official MCP transport worker. Redirects,
//! transparent tool retries, cookies, and automatic credential flows are off.
use super::{Client, Diagnostics, Owner, CONNECTIONS, FRAME_LIMIT, TOTAL_LIMIT};
use anyhow::{ensure, Context, Result};
use futures_util::{
    stream::{self, BoxStream},
    StreamExt,
};
use reqwest::{
    header::{HeaderName, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE},
    Method, Response, StatusCode, Url,
};
use rmcp::{
    model::{ClientJsonRpcMessage, ClientRequest, ErrorData, ServerJsonRpcMessage},
    transport::{
        common::client_side_sse::NeverRetry,
        streamable_http_client::{
            StreamableHttpClient, StreamableHttpClientTransport,
            StreamableHttpClientTransportConfig, StreamableHttpError, StreamableHttpPostResponse,
        },
    },
};
use sse_stream::{Sse, SseStream};
use std::{
    collections::HashMap,
    io,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct HttpSpec {
    pub url: String,
    pub bearer_token: Option<String>,
    pub timeout: Duration,
}
pub fn validate_url(value: &str) -> Result<Url> {
    ensure!(value.len() <= 4096, "MCP URL exceeds 4096 bytes");
    let url = Url::parse(value).context("Invalid MCP URL")?;
    ensure!(
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none(),
        "MCP URLs need HTTP(S), a host, and no embedded credentials or fragment"
    );
    Ok(url)
}
pub fn loopback(url: &Url) -> bool {
    url.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    })
}
impl HttpSpec {
    fn validate(&self) -> Result<()> {
        let url = validate_url(&self.url)?;
        ensure!(
            self.timeout >= Duration::from_millis(100) && self.timeout <= Duration::from_secs(120),
            "MCP timeout must be 0.1–120 seconds"
        );
        if let Some(token) = &self.bearer_token {
            ensure!(
                !token.is_empty()
                    && token.len() <= 16_000
                    && token.bytes().all(|b| (0x21..=0x7e).contains(&b)),
                "Invalid MCP bearer token"
            );
            ensure!(
                url.scheme() == "https" || loopback(&url),
                "MCP bearer credentials require HTTPS outside loopback"
            );
        }
        Ok(())
    }
}

struct Budget {
    bytes: usize,
    frames: usize,
    window: Instant,
}
#[derive(Clone)]
struct Backend {
    // The worker can outlive an abandoned initialization/client future. Keep
    // its slot until in-flight streams and bounded session cleanup finish.
    _permit: Arc<tokio::sync::OwnedSemaphorePermit>,
    client: reqwest::Client,
    url: String,
    timeout: Duration,
    cancel: CancellationToken,
    diagnostics: Diagnostics,
    budget: Arc<Mutex<Budget>>,
}
type Error = StreamableHttpError<io::Error>;
impl Backend {
    fn failure(&self, message: &str) -> io::Error {
        self.diagnostics.fail(message);
        self.cancel.cancel();
        io::Error::other(message.to_owned())
    }
    fn count(&self, bytes: usize, frames: usize) -> io::Result<()> {
        let mut budget = self
            .budget
            .lock()
            .map_err(|_| io::Error::other("MCP HTTP budget lock failed"))?;
        budget.bytes = budget.bytes.saturating_add(bytes);
        if budget.window.elapsed() >= Duration::from_secs(1) {
            budget.window = Instant::now();
            budget.frames = 0;
        }
        budget.frames = budget.frames.saturating_add(frames);
        if budget.bytes > TOTAL_LIMIT {
            return Err(self.failure("MCP HTTP connection exceeded 32 MiB"));
        }
        if budget.frames > 128 {
            return Err(self.failure("MCP HTTP frame rate exceeded 128 per second"));
        }
        Ok(())
    }
    fn request(
        &self,
        method: Method,
        uri: &str,
        session: Option<&str>,
        auth: Option<String>,
        headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<reqwest::RequestBuilder, Error> {
        if uri != self.url {
            return Err(Error::Client(
                self.failure("MCP HTTP endpoint changed unexpectedly"),
            ));
        }
        let mut request = self
            .client
            .request(method, uri)
            .header(ACCEPT, "application/json, text/event-stream");
        if let Some(session) = session {
            if session.is_empty()
                || session.len() > 1024
                || !session.bytes().all(|b| (0x21..=0x7e).contains(&b))
            {
                return Err(Error::Client(self.failure("Invalid MCP session ID")));
            }
            request = request.header("mcp-session-id", session);
        }
        if let Some(auth) = auth {
            let mut value = HeaderValue::from_str(&format!("Bearer {auth}"))
                .map_err(|_| Error::Client(self.failure("Invalid MCP authorization header")))?;
            value.set_sensitive(true);
            request = request.header(AUTHORIZATION, value);
        }
        let mut size = 0;
        for (name, value) in headers {
            // These headers come only from the SDK's negotiated protocol. A
            // peer may annotate Mcp-Param-* headers, never credentials or Host.
            if !name.as_str().starts_with("mcp-") {
                return Err(Error::Client(
                    self.failure("Unexpected MCP protocol header"),
                ));
            }
            size += name.as_str().len() + value.as_bytes().len();
            if size > 16_384 {
                return Err(Error::Client(
                    self.failure("MCP request headers exceed 16 KiB"),
                ));
            }
            request = request.header(name, value);
        }
        Ok(request)
    }
    async fn send(&self, request: reqwest::RequestBuilder) -> Result<Response, Error> {
        tokio::select! {
            _=self.cancel.cancelled()=>Err(Error::Client(io::Error::other("MCP HTTP request cancelled"))),
            response=tokio::time::timeout(self.timeout, request.send())=>match response {
                Ok(Ok(response))=>Ok(response),
                Ok(Err(_))=>Err(Error::Client(self.failure("MCP HTTP connection failed"))),
                Err(_)=>Err(Error::Client(self.failure("MCP HTTP response headers timed out"))),
            }
        }
    }
    fn session(&self, response: &Response) -> Result<Option<String>, Error> {
        response
            .headers()
            .get("mcp-session-id")
            .map(|value| {
                let id = value
                    .to_str()
                    .map_err(|_| Error::Client(self.failure("Invalid MCP session ID")))?;
                if id.is_empty()
                    || id.len() > 1024
                    || !id.bytes().all(|b| (0x21..=0x7e).contains(&b))
                {
                    return Err(Error::Client(self.failure("Invalid MCP session ID")));
                }
                Ok(id.to_owned())
            })
            .transpose()
    }
    async fn body(&self, mut response: Response) -> Result<Vec<u8>, Error> {
        if response
            .content_length()
            .is_some_and(|n| n > FRAME_LIMIT as u64)
        {
            return Err(Error::Client(
                self.failure("MCP HTTP JSON body exceeds 1 MiB"),
            ));
        }
        let read = async {
            let mut body = Vec::new();
            loop {
                let chunk = tokio::select! {
                    _=self.cancel.cancelled()=>return Err(Error::Client(io::Error::other("MCP HTTP body cancelled"))),
                    chunk=response.chunk()=>chunk.map_err(|_|Error::Client(self.failure("MCP HTTP body read failed")))?,
                };
                let Some(chunk) = chunk else { break };
                self.count(chunk.len(), 0).map_err(Error::Client)?;
                if body.len() + chunk.len() > FRAME_LIMIT {
                    return Err(Error::Client(
                        self.failure("MCP HTTP JSON body exceeds 1 MiB"),
                    ));
                }
                body.extend_from_slice(&chunk);
            }
            self.count(0, 1).map_err(Error::Client)?;
            Ok(body)
        };
        tokio::time::timeout(self.timeout, read)
            .await
            .map_err(|_| Error::Client(self.failure("MCP HTTP body timed out")))?
    }
    fn sse(
        &self,
        response: Response,
        max: usize,
    ) -> BoxStream<'static, Result<Sse, sse_stream::Error>> {
        let backend = self.clone();
        let stream = stream::try_unfold(
            (response, SseBudget::default(), backend),
            move |(mut response, mut budget, backend)| async move {
                let chunk = tokio::select! {
                    _=backend.cancel.cancelled()=>return Err(io::Error::other("MCP SSE cancelled")),
                    chunk=response.chunk()=>chunk.map_err(|_|backend.failure("MCP SSE read failed"))?,
                };
                let Some(chunk) = chunk else { return Ok(None) };
                let frames = budget
                    .observe(&chunk, max.min(FRAME_LIMIT))
                    .map_err(|_| backend.failure("MCP SSE event exceeds 1 MiB"))?;
                backend.count(chunk.len(), frames)?;
                Ok(Some((chunk, (response, budget, backend))))
            },
        );
        let backend = self.clone();
        SseStream::from_bytes_stream(stream)
            .map(move |event| {
                let event = event.map_err(|_| {
                    sse_stream::Error::Body(Box::new(
                        backend.failure("Invalid or interrupted MCP SSE stream"),
                    ))
                })?;
                if let Some(data) = event.data.as_deref().filter(|data| !data.trim().is_empty()) {
                    if serde_json::from_str::<ServerJsonRpcMessage>(data).is_err() {
                        return Err(sse_stream::Error::Body(Box::new(
                            backend.failure("Invalid MCP SSE JSON-RPC frame"),
                        )));
                    }
                }
                if event.id.as_ref().is_some_and(|id| id.len() > 4096) {
                    return Err(sse_stream::Error::Body(Box::new(
                        backend.failure("MCP SSE event ID exceeds 4 KiB"),
                    )));
                }
                Ok(event)
            })
            .boxed()
    }
}
#[derive(Default)]
struct SseBudget {
    event: usize,
    line: usize,
    cr: bool,
}
impl SseBudget {
    fn observe(&mut self, chunk: &[u8], maximum: usize) -> Result<usize, ()> {
        let mut frames = 0;
        for &byte in chunk {
            self.event += 1;
            if self.event > maximum {
                return Err(());
            }
            if self.cr && byte == b'\n' {
                self.cr = false;
                continue;
            }
            self.cr = byte == b'\r';
            if byte == b'\r' || byte == b'\n' {
                if self.line == 0 {
                    self.event = 0;
                    frames += 1;
                }
                self.line = 0;
            } else {
                self.line += 1;
            }
        }
        Ok(frames)
    }
}
impl StreamableHttpClient for Backend {
    type Error = io::Error;
    async fn post_message(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session: Option<Arc<str>>,
        auth: Option<String>,
        headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<StreamableHttpPostResponse, Error> {
        self.post_message_with_max_sse_event_size(uri, message, session, auth, headers, FRAME_LIMIT)
            .await
    }
    async fn post_message_with_max_sse_event_size(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session: Option<Arc<str>>,
        auth: Option<String>,
        headers: HashMap<HeaderName, HeaderValue>,
        maximum: usize,
    ) -> Result<StreamableHttpPostResponse, Error> {
        let bytes = serde_json::to_vec(&message)
            .map_err(|_| Error::Client(self.failure("Cannot encode MCP HTTP request")))?;
        if bytes.len() > FRAME_LIMIT {
            return Err(Error::Client(
                self.failure("MCP HTTP request exceeds 1 MiB"),
            ));
        }
        let request = self
            .request(Method::POST, &uri, session.as_deref(), auth, headers)?
            .header(CONTENT_TYPE, "application/json")
            .body(bytes);
        let response = self.send(request).await?;
        let status = response.status();
        // Never follow redirects or turn an expired session into a tool retry.
        if status.is_redirection() {
            return Err(Error::Client(
                self.failure("MCP HTTP redirects are disabled"),
            ));
        }
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(Error::Client(
                self.failure("MCP HTTP authorization was rejected"),
            ));
        }
        if status == StatusCode::NOT_FOUND && session.is_some() {
            return Err(Error::SessionExpired);
        }
        let session_id = self.session(&response)?;
        if status == StatusCode::ACCEPTED || status == StatusCode::NO_CONTENT {
            return Ok(StreamableHttpPostResponse::Accepted);
        }
        let kind = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if status.is_success() && kind == "text/event-stream" {
            return Ok(StreamableHttpPostResponse::Sse(
                self.sse(response, maximum),
                session_id,
            ));
        }
        let body = self.body(response).await?;
        if status.is_success()
            && body.is_empty()
            && !matches!(message, ClientJsonRpcMessage::Request(_))
        {
            return Ok(StreamableHttpPostResponse::Accepted);
        }
        if kind == "application/json" {
            if let Ok(parsed) = serde_json::from_slice::<ServerJsonRpcMessage>(&body) {
                if status.is_success() || matches!(parsed, ServerJsonRpcMessage::Error(_)) {
                    return Ok(StreamableHttpPostResponse::Json(parsed, session_id));
                }
            }
        }
        // Discovery has no side effects. Only a legacy-style 400/404/405 can
        // trigger the SDK's initialize handshake; authentication never does.
        if session.is_none() && matches!(status.as_u16(), 400 | 404 | 405) {
            if let ClientJsonRpcMessage::Request(request) = message {
                if matches!(request.request, ClientRequest::DiscoverRequest(_)) {
                    return Ok(StreamableHttpPostResponse::Json(
                        ServerJsonRpcMessage::error(
                            ErrorData::invalid_request(
                                "Legacy MCP discovery requires initialize",
                                None,
                            ),
                            Some(request.id),
                        ),
                        None,
                    ));
                }
            }
        }
        Err(Error::Client(self.failure(
            "Invalid MCP HTTP status, content type, or JSON-RPC response",
        )))
    }
    async fn get_stream(
        &self,
        uri: Arc<str>,
        session: Option<Arc<str>>,
        last: Option<String>,
        auth: Option<String>,
        headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<BoxStream<'static, Result<Sse, sse_stream::Error>>, Error> {
        self.get_stream_with_max_sse_event_size(uri, session, last, auth, headers, FRAME_LIMIT)
            .await
    }
    async fn get_stream_with_max_sse_event_size(
        &self,
        uri: Arc<str>,
        session: Option<Arc<str>>,
        last: Option<String>,
        auth: Option<String>,
        headers: HashMap<HeaderName, HeaderValue>,
        maximum: usize,
    ) -> Result<BoxStream<'static, Result<Sse, sse_stream::Error>>, Error> {
        let mut request = self.request(Method::GET, &uri, session.as_deref(), auth, headers)?;
        if let Some(last) = last {
            if last.len() > 4096 {
                return Err(Error::Client(self.failure("MCP SSE cursor exceeds 4 KiB")));
            }
            request = request.header("last-event-id", last);
        }
        let response = self.send(request).await?;
        if response.status() == StatusCode::METHOD_NOT_ALLOWED {
            return Err(Error::ServerDoesNotSupportSse);
        }
        if !response.status().is_success() {
            return Err(Error::Client(
                self.failure("MCP SSE endpoint rejected the request"),
            ));
        }
        if response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_none_or(|s| {
                !s.split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .eq_ignore_ascii_case("text/event-stream")
            })
        {
            return Err(Error::Client(
                self.failure("MCP SSE endpoint returned an invalid content type"),
            ));
        }
        Ok(self.sse(response, maximum))
    }
    async fn delete_session(
        &self,
        uri: Arc<str>,
        session: Arc<str>,
        auth: Option<String>,
        headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<(), Error> {
        // Cleanup must still be attempted after the task token is cancelled.
        let request = self.request(Method::DELETE, &uri, Some(&session), auth, headers)?;
        let response = tokio::time::timeout(Duration::from_secs(1), request.send())
            .await
            .map_err(|_| Error::Client(io::Error::other("MCP HTTP session cleanup timed out")))?
            .map_err(|_| Error::Client(io::Error::other("MCP HTTP session cleanup failed")))?;
        if response.status().is_success() || matches!(response.status().as_u16(), 404 | 405) {
            Ok(())
        } else {
            Err(Error::Client(io::Error::other(
                "MCP HTTP session cleanup was rejected",
            )))
        }
    }
}

impl Client {
    pub async fn connect_http(spec: &HttpSpec, cancel: CancellationToken) -> Result<Self> {
        spec.validate()?;
        ensure!(!cancel.is_cancelled(), "MCP connection cancelled");
        let permit = Arc::new(
            CONNECTIONS
                .get_or_init(|| Arc::new(Semaphore::new(16)))
                .clone()
                .try_acquire_owned()
                .context("Native MCP connection limit reached")?,
        );
        let cancel = cancel.child_token();
        let diagnostics = Diagnostics::default();
        let mut builder = reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(spec.timeout.min(Duration::from_secs(10)))
            .redirect(reqwest::redirect::Policy::none())
            .pool_max_idle_per_host(0);
        // Do not allow a hosts-file or resolver override to turn a loopback-only
        // activation into remote access. Explicit remote hosts need network access.
        builder = builder.resolve_to_addrs(
            "localhost",
            &["127.0.0.1:0".parse().unwrap(), "[::1]:0".parse().unwrap()],
        );
        let backend = Backend {
            _permit: permit.clone(),
            client: builder.build().context("Cannot create MCP HTTP client")?,
            url: spec.url.clone(),
            timeout: spec.timeout,
            cancel: cancel.clone(),
            diagnostics: diagnostics.clone(),
            budget: Arc::new(Mutex::new(Budget {
                bytes: 0,
                frames: 0,
                window: Instant::now(),
            })),
        };
        let mut config = StreamableHttpClientTransportConfig::with_uri(spec.url.clone())
            .max_concurrent_requests(2)
            .control_request_timeout(Duration::from_secs(1))
            .max_sse_event_size(FRAME_LIMIT)
            .reinit_on_expired_session(false);
        config.channel_buffer_capacity = 8;
        config.retry_config = Arc::new(NeverRetry::default());
        config.auth_header = spec.bearer_token.clone();
        let transport = StreamableHttpClientTransport::with_client(backend, config);
        Self {
            service: None,
            owner: Owner {
                cancel,
                task: None,
                group: None,
                http_permit: Some(permit),
            },
            diagnostics,
            timeout: spec.timeout,
            tools: Vec::new(),
            pid: None,
        }
        .initialize(transport)
        .await
    }
}
