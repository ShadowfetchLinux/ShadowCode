//! Authenticated loopback Streamable HTTP. The gateway credential identifies
//! one owner across stateless requests; transport reconnects do not replay jobs.
use super::{Access, Backend, Cleanup, Handler, Owned};
use crate::{mcp::FRAME_LIMIT, paths::AppPaths, workspace::Workspace};
use anyhow::{ensure, Result};
use bytes::Bytes;
use http_body::{Body, Frame};
use http_body_util::{BodyExt, Full, Limited};
use hyper::{
    body::Incoming,
    header::{AUTHORIZATION, CONNECTION, CONTENT_TYPE, HOST, ORIGIN, WWW_AUTHENTICATE},
    server::conn::http1,
    service::service_fn,
    Request, Response, StatusCode,
};
use hyper_util::rt::{TokioIo, TokioTimer};
use rmcp::transport::streamable_http_server::{
    session::never::NeverSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use sha2::{Digest, Sha256};
use std::{
    convert::Infallible,
    future::Future,
    io,
    net::SocketAddr,
    path::PathBuf,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::{Duration, Instant},
};
use tokio::{net::TcpListener, sync::Semaphore, task::JoinSet, time::Sleep};
use tokio_util::sync::CancellationToken;
use tower_service::Service;

type McpService = StreamableHttpService<Handler, NeverSessionManager>;
type BoxResponse = Response<http_body_util::combinators::BoxBody<Bytes, Infallible>>;
const RESPONSE_LIMIT: usize = 8 * FRAME_LIMIT;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Validate before opening a listener or profile. Authentication is mandatory;
/// local plaintext is intentionally never exposed on an external interface.
pub fn validate(address: SocketAddr, token: &str) -> Result<()> {
    ensure!(
        address.ip().is_loopback(),
        "MCP HTTP must bind a loopback IP address"
    );
    ensure!(
        (32..=512).contains(&token.len())
            && token.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~+/=".contains(&b)),
        "MCP HTTP token must contain 32–512 bearer-token characters; use a randomly generated secret"
    );
    Ok(())
}

// Bound output even if an SDK method is added outside the native tool limits.
// Dropping this body also drops the SDK's per-request cancellation guard.
struct ResponseBody {
    inner: http_body_util::combinators::BoxBody<Bytes, Infallible>,
    timeout: Pin<Box<Sleep>>,
    remaining: usize,
    done: bool,
}
impl Body for ResponseBody {
    type Data = Bytes;
    type Error = io::Error;
    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, io::Error>>> {
        if self.done {
            return Poll::Ready(None);
        }
        if self.timeout.as_mut().poll(cx).is_ready() {
            self.done = true;
            return Poll::Ready(Some(Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "MCP response deadline exceeded",
            ))));
        }
        match Pin::new(&mut self.inner).poll_frame(cx) {
            Poll::Ready(Some(Ok(frame))) => {
                let size = frame.data_ref().map_or(0, Bytes::len);
                if size > self.remaining {
                    self.done = true;
                    return Poll::Ready(Some(Err(io::Error::other("MCP response exceeds 8 MiB"))));
                }
                self.remaining -= size;
                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Ready(Some(Err(error))) => match error {},
            Poll::Ready(None) => {
                self.done = true;
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
    fn is_end_stream(&self) -> bool {
        self.done || self.inner.is_end_stream()
    }
}
fn bounded(response: BoxResponse) -> Response<ResponseBody> {
    response.map(|inner| ResponseBody {
        inner,
        timeout: Box::pin(tokio::time::sleep(REQUEST_TIMEOUT)),
        remaining: RESPONSE_LIMIT,
        done: false,
    })
}
fn reject(status: StatusCode, message: &'static str) -> BoxResponse {
    let mut response = Response::new(Full::new(Bytes::from_static(message.as_bytes())).boxed());
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, "text/plain; charset=utf-8".parse().unwrap());
    response
        .headers_mut()
        .insert(CONNECTION, "close".parse().unwrap());
    if status == StatusCode::UNAUTHORIZED {
        response.headers_mut().insert(
            WWW_AUTHENTICATE,
            "Bearer realm=\"ShadowCode local MCP\"".parse().unwrap(),
        );
    }
    response
}
struct Rate {
    since: Instant,
    requests: usize,
}
#[derive(Clone)]
struct Gateway {
    service: McpService,
    digest: [u8; 32],
    authorities: Arc<Vec<String>>,
    rate: Arc<Mutex<Rate>>,
    cancel: CancellationToken,
}
impl Gateway {
    async fn request(&self, request: Request<Incoming>) -> BoxResponse {
        if self.cancel.is_cancelled() {
            return reject(StatusCode::SERVICE_UNAVAILABLE, "Gateway is stopping");
        }
        {
            let mut rate = self.rate.lock().unwrap_or_else(|e| e.into_inner());
            if rate.since.elapsed() >= Duration::from_secs(1) {
                rate.since = Instant::now();
                rate.requests = 0;
            }
            rate.requests += 1;
            if rate.requests > 128 {
                return reject(
                    StatusCode::TOO_MANY_REQUESTS,
                    "Request rate exceeds 128 per second",
                );
            }
        }
        let headers = request.headers();
        if headers.contains_key(ORIGIN) {
            return reject(
                StatusCode::FORBIDDEN,
                "Browser origins are not accepted by this local gateway",
            );
        }
        if headers.get_all(HOST).iter().count() != 1
            || !headers
                .get(HOST)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|host| {
                    self.authorities
                        .iter()
                        .any(|allowed| allowed.eq_ignore_ascii_case(host))
                })
        {
            return reject(StatusCode::FORBIDDEN, "Invalid gateway host");
        }
        let token = headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|value| {
                let (scheme, token) = value.split_once(' ')?;
                scheme.eq_ignore_ascii_case("Bearer").then_some(token)
            });
        let authenticated = token.is_some_and(|token| {
            let actual: [u8; 32] = Sha256::digest(token.as_bytes()).into();
            actual
                .iter()
                .zip(self.digest)
                .fold(0u8, |difference, (left, right)| difference | (left ^ right))
                == 0
        });
        if headers.get_all(AUTHORIZATION).iter().count() != 1 || !authenticated {
            return reject(
                StatusCode::UNAUTHORIZED,
                "A valid gateway bearer credential is required",
            );
        }
        if request.uri().scheme().is_some()
            || request
                .uri()
                .path_and_query()
                .is_none_or(|p| p.as_str() != "/mcp")
        {
            return reject(
                StatusCode::NOT_FOUND,
                "Use the /mcp endpoint without query parameters",
            );
        }
        // These routing/metadata headers are single values. Reject ambiguity
        // before the SDK reads just the first value of a duplicated header.
        if headers.keys().any(|name| {
            name.as_str().starts_with("mcp-") && headers.get_all(name).iter().count() != 1
        }) {
            return reject(StatusCode::BAD_REQUEST, "Duplicate MCP metadata header");
        }
        let (mut parts, body) = request.into_parts();
        // Authentication never reaches MCP tool contexts, errors or SDK logs.
        parts.headers.remove(AUTHORIZATION);
        let body = tokio::select! {
            _=self.cancel.cancelled()=>return reject(StatusCode::SERVICE_UNAVAILABLE,"Gateway is stopping"),
            result=tokio::time::timeout(Duration::from_secs(5),Limited::new(body,FRAME_LIMIT).collect())=>match result {
                Err(_)=>return reject(StatusCode::REQUEST_TIMEOUT,"Request body deadline exceeded"),
                Ok(Err(_))=>return reject(StatusCode::PAYLOAD_TOO_LARGE,"Invalid request body or request exceeds 1 MiB"),
                Ok(Ok(body))=>body.to_bytes(),
            }
        };
        let request = Request::from_parts(parts, Full::new(body));
        let mut service = self.service.clone();
        tokio::select! {
            _=self.cancel.cancelled()=>reject(StatusCode::SERVICE_UNAVAILABLE,"Gateway is stopping"),
            result=tokio::time::timeout(REQUEST_TIMEOUT,service.call(request))=>match result {
                Ok(Ok(response))=>response,
                Ok(Err(error))=>match error {},
                Err(_)=>reject(StatusCode::GATEWAY_TIMEOUT,"MCP request deadline exceeded"),
            }
        }
    }
}

/// Run one authenticated HTTP gateway until cancelled. A credential grants access
/// to its single fixed-project owner; use separate gateways for separate clients.
/// Unfinished delegated jobs stop before shutdown returns. Shared engine work is
/// unaffected; project background processes follow their documented lifetime.
pub async fn serve(
    paths: AppPaths,
    workspace: PathBuf,
    access: Access,
    listener: TcpListener,
    token: String,
    cancel: CancellationToken,
    ready: Option<tokio::sync::oneshot::Sender<()>>,
) -> Result<()> {
    let address = listener.local_addr()?;
    validate(address, &token)?;
    ensure!(
        !access.allow_approvals || access.allow_write,
        "--allow-approvals requires --allow-write"
    );
    let workspace = Arc::new(Workspace::open(&workspace)?);
    let backend =
        Arc::new(Backend::open(paths.clone(), workspace.path.clone(), false, None).await?);
    let handler = Handler {
        backend: backend.clone(),
        paths,
        workspace,
        access,
        owned: Arc::new(tokio::sync::Mutex::new(Owned::default())),
        calls: Arc::new(Semaphore::new(8)),
        cancel: cancel.child_token(),
        http: true,
    };
    let mut cleanup = Cleanup(Some(handler.clone()));
    let authorities = vec![address.to_string(), format!("localhost:{}", address.port())];
    let mut config = StreamableHttpServerConfig::default()
        .with_allowed_hosts(authorities.clone())
        .enforce_origin_validation();
    config.legacy_session_mode = false;
    config.json_response = true;
    config.cancellation_token = handler.cancel.clone();
    config.max_request_body_bytes = FRAME_LIMIT;
    let owned_handler = handler.clone();
    let gateway = Gateway {
        service: StreamableHttpService::new(
            move || Ok(owned_handler.clone()),
            Arc::new(NeverSessionManager::default()),
            config,
        ),
        digest: Sha256::digest(token.as_bytes()).into(),
        authorities: Arc::new(authorities),
        rate: Arc::new(Mutex::new(Rate {
            since: Instant::now(),
            requests: 0,
        })),
        cancel: handler.cancel.clone(),
    };
    drop(token);
    if ready.is_some_and(|ready| ready.send(()).is_err()) {
        handler.cancel.cancel();
    }
    let slots = Arc::new(Semaphore::new(32));
    let mut connections = JoinSet::new();
    let result = loop {
        tokio::select! {
            _=handler.cancel.cancelled()=>break Ok(()),
            Some(_)=connections.join_next()=>{},
            incoming=listener.accept()=>{
                let (stream,_) = match incoming { Ok(value)=>value, Err(error)=>break Err(error.into()) };
                let Ok(permit) = slots.clone().try_acquire_owned() else { drop(stream); continue; };
                let gateway=gateway.clone();
                connections.spawn(async move {
                    let _permit=permit;
                    let cancel=gateway.cancel.clone();
                    let service=service_fn(move |request| { let gateway=gateway.clone(); async move { Ok::<_,Infallible>(bounded(gateway.request(request).await)) } });
                    let mut builder=http1::Builder::new();
                    builder.timer(TokioTimer::new()).keep_alive(false).header_read_timeout(Duration::from_secs(5)).max_headers(64).max_buf_size(65536);
                    let connection=builder.serve_connection(TokioIo::new(stream),service);
                    tokio::pin!(connection);
                    tokio::select! { _=cancel.cancelled()=>{}, _=&mut connection=>{}, _=tokio::time::sleep(Duration::from_secs(135))=>{} }
                });
            }
        }
    };
    handler.cancel.cancel();
    drop(listener);
    while connections.join_next().await.is_some() {}
    let cleaned = handler.cleanup().await;
    let closed = backend.close().await;
    cleanup.0.take();
    cleaned?;
    closed?;
    result
}
