//! Loopback reverse proxies for the preview frame.
//!
//! Each previewed server gets its own listener on `127.0.0.1:<random>`, so the
//! page keeps its absolute paths (`/src/main.tsx`, `/@vite/client`) and each
//! server is its own origin. Requests are forwarded unchanged apart from
//! `Host`/`Origin`/`Referer` (rewritten to the dev server's own address) and
//! `Accept-Encoding: identity`, so HTML can be read to add the picker script.
//! WebSocket upgrades (hot reload) are passed through byte for byte.
use super::Target;
use anyhow::{bail, ensure, Context, Result};
use bytes::Bytes;
use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full, Limited};
use hyper::{
    body::Incoming,
    header::{self, HeaderMap, HeaderName, HeaderValue},
    server::conn::http1,
    service::service_fn,
    Method, Request, Response, StatusCode, Version,
};
use hyper_util::rt::{TokioIo, TokioTimer};
use std::{
    collections::HashSet,
    convert::Infallible,
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{Mutex, Semaphore},
};
use tokio_util::sync::CancellationToken;

/// The picker script's reserved path. Requests under this prefix are answered
/// by the proxy and never forwarded to the dev server.
pub const RESERVED: &str = "/__shadowcode_preview__/";
pub const PICKER_PATH: &str = "/__shadowcode_preview__/picker.js";
const PICKER: &str = include_str!("picker.js");
const PICKER_TAG: &str = "<script src=\"/__shadowcode_preview__/picker.js\"></script>";
/// Proxies kept open at once; the least recently opened one closes first.
const MAX_PROXIES: usize = 8;
const MAX_CONNECTIONS: usize = 96;
/// HTML larger than this is not buffered for the picker.
const HTML_LIMIT: usize = 16 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

type BoxError = Box<dyn std::error::Error + Send + Sync>;
type ProxyBody = BoxBody<Bytes, BoxError>;

struct Proxy {
    target: Target,
    /// `http://127.0.0.1:<port>`: the preview frame's origin.
    origin: String,
    /// `127.0.0.1:<port>`: the only `Host` accepted.
    authority: String,
    script: Bytes,
}

struct Entry {
    proxy: Arc<Proxy>,
    app_origin: String,
    port: u16,
    cancel: CancellationToken,
    opened: Instant,
}
impl Drop for Entry {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// A proxy the preview frame can load.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Opened {
    /// `http://127.0.0.1:<port>` (no trailing slash).
    pub proxy_origin: String,
    pub proxy_port: u16,
    /// The dev server's own origin, e.g. `http://localhost:5173`.
    pub target_origin: String,
}

/// The engine's open preview proxies (shared by every view of one engine).
#[derive(Default)]
pub struct Previews {
    open: Mutex<Vec<Entry>>,
}

impl Previews {
    /// Open (or reuse) the proxy for `target`, embedded by `app_origin`.
    /// `forbidden` are ports the engine itself listens on.
    pub async fn open(
        &self,
        target: Target,
        app_origin: &str,
        forbidden: &HashSet<u16>,
    ) -> Result<Opened> {
        target.check()?;
        super::check_app_origin(app_origin)?;
        let mut open = self.open.lock().await;
        ensure!(
            !forbidden.contains(&target.port) && !open.iter().any(|e| e.port == target.port),
            "Port {} belongs to ShadowCode itself; the preview only opens your project's servers",
            target.port
        );
        if let Some(entry) = open
            .iter()
            .find(|e| e.proxy.target == target && e.app_origin == app_origin)
        {
            return Ok(entry.opened_view());
        }
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .context("Could not open a loopback port for the preview")?;
        let port = listener.local_addr()?.port();
        let origin = format!("http://127.0.0.1:{port}");
        let script = PICKER.replace("/*APP_ORIGIN*/\"\"", &serde_json::to_string(app_origin)?);
        let proxy = Arc::new(Proxy {
            target,
            authority: format!("127.0.0.1:{port}"),
            origin,
            script: Bytes::from(script),
        });
        let cancel = CancellationToken::new();
        tokio::spawn(serve(listener, proxy.clone(), cancel.clone()));
        if open.len() >= MAX_PROXIES {
            open.sort_by_key(|e| e.opened);
            open.remove(0);
        }
        let entry = Entry {
            proxy,
            app_origin: app_origin.to_owned(),
            port,
            cancel,
            opened: Instant::now(),
        };
        let view = entry.opened_view();
        open.push(entry);
        Ok(view)
    }
    /// The loopback ports of the open proxies.
    pub async fn ports(&self) -> Vec<u16> {
        self.open.lock().await.iter().map(|e| e.port).collect()
    }
    /// Close every proxy (their listeners and connections stop).
    pub async fn close_all(&self) {
        self.open.lock().await.clear();
    }
}
impl Entry {
    fn opened_view(&self) -> Opened {
        Opened {
            proxy_origin: self.proxy.origin.clone(),
            proxy_port: self.port,
            target_origin: self.proxy.target.origin(),
        }
    }
}

async fn serve(listener: TcpListener, proxy: Arc<Proxy>, cancel: CancellationToken) {
    let permits = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    loop {
        let accepted = tokio::select! {
            _ = cancel.cancelled() => return,
            accepted = listener.accept() => accepted,
        };
        let Ok((stream, _)) = accepted else {
            tokio::time::sleep(Duration::from_millis(50)).await;
            continue;
        };
        let Ok(permit) = permits.clone().try_acquire_owned() else {
            continue;
        };
        let (proxy, cancel) = (proxy.clone(), cancel.clone());
        tokio::spawn(async move {
            let _permit = permit;
            let tunnels = cancel.clone();
            let service = service_fn(move |request| {
                let (proxy, tunnels) = (proxy.clone(), tunnels.clone());
                async move { Ok::<_, Infallible>(proxy.handle(request, tunnels).await) }
            });
            let connection = http1::Builder::new()
                .timer(TokioTimer::new())
                .header_read_timeout(Duration::from_secs(30))
                .serve_connection(TokioIo::new(stream), service)
                .with_upgrades();
            tokio::select! {
                _ = connection => {}
                _ = cancel.cancelled() => {}
            }
        });
    }
}

impl Proxy {
    async fn handle(
        &self,
        request: Request<Incoming>,
        cancel: CancellationToken,
    ) -> Response<ProxyBody> {
        let host = request
            .headers()
            .get(header::HOST)
            .and_then(|h| h.to_str().ok());
        // DNS rebinding: a page that points its own name at 127.0.0.1 sends
        // its own Host, which never matches.
        if host != Some(self.authority.as_str()) {
            return plain(StatusCode::MISDIRECTED_REQUEST, "Unknown host");
        }
        let path = request.uri().path();
        if path == PICKER_PATH {
            let mut response = Response::new(full(self.script.clone()));
            let headers = response.headers_mut();
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/javascript; charset=utf-8"),
            );
            headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            headers.insert(
                header::X_CONTENT_TYPE_OPTIONS,
                HeaderValue::from_static("nosniff"),
            );
            return response;
        }
        if path.starts_with(RESERVED) {
            return plain(StatusCode::NOT_FOUND, "Not found");
        }
        match self.forward(request, cancel).await {
            Ok(response) => response,
            Err(error) => self.unreachable(&error),
        }
    }

    async fn forward(
        &self,
        mut request: Request<Incoming>,
        cancel: CancellationToken,
    ) -> Result<Response<ProxyBody>> {
        let upgrade = wants_upgrade(request.headers());
        let client_side = upgrade.then(|| hyper::upgrade::on(&mut request));
        let head = request.method() == Method::HEAD;
        let stream = connect(&self.target).await?;
        let (mut sender, connection) =
            hyper::client::conn::http1::handshake::<_, Incoming>(TokioIo::new(stream)).await?;
        tokio::spawn(connection.with_upgrades());
        let (mut parts, body) = request.into_parts();
        parts.uri = parts
            .uri
            .path_and_query()
            .map_or("/", |p| p.as_str())
            .parse()?;
        parts.version = Version::HTTP_11;
        self.outgoing_headers(&mut parts.headers, upgrade);
        let mut response = sender
            .send_request(Request::from_parts(parts, body))
            .await?;

        if response.status() == StatusCode::SWITCHING_PROTOCOLS {
            let Some(client_side) = client_side else {
                bail!("The dev server switched protocols without being asked");
            };
            let server_side = hyper::upgrade::on(&mut response);
            tokio::spawn(async move {
                let (Ok(client), Ok(server)) = (client_side.await, server_side.await) else {
                    return;
                };
                let (mut client, mut server) = (TokioIo::new(client), TokioIo::new(server));
                tokio::select! {
                    _ = tokio::io::copy_bidirectional(&mut client, &mut server) => {}
                    _ = cancel.cancelled() => {}
                }
            });
            let (parts, _) = response.into_parts();
            return Ok(Response::from_parts(parts, empty()));
        }

        let (mut parts, body) = response.into_parts();
        self.incoming_headers(&mut parts.headers);
        let has_body = !head
            && parts.status != StatusCode::NO_CONTENT
            && parts.status != StatusCode::NOT_MODIFIED;
        if has_body && is_html(&parts.headers) {
            let html = Limited::new(body, HTML_LIMIT)
                .collect()
                .await
                .map_err(|e| anyhow::anyhow!("Could not read the page: {e}"))?
                .to_bytes();
            let html = Bytes::from(inject_picker(&html));
            parts
                .headers
                .insert(header::CONTENT_LENGTH, html.len().into());
            // The body changed; the dev server's validators no longer match it.
            parts.headers.remove(header::ETAG);
            parts.headers.remove(header::LAST_MODIFIED);
            parts
                .headers
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            return Ok(Response::from_parts(parts, full(html)));
        }
        Ok(Response::from_parts(
            parts,
            body.map_err(|e| Box::new(e) as BoxError).boxed(),
        ))
    }

    fn outgoing_headers(&self, headers: &mut HeaderMap, upgrade: bool) {
        remove_hop_by_hop(headers, upgrade);
        if let Ok(host) = HeaderValue::from_str(&self.target.authority()) {
            headers.insert(header::HOST, host);
        }
        headers.insert(
            header::ACCEPT_ENCODING,
            HeaderValue::from_static("identity"),
        );
        let target = self.target.origin();
        for name in [header::ORIGIN, header::REFERER] {
            let replaced = headers
                .get(&name)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| rebase(v, &self.origin, &target));
            if let Some(value) = replaced.and_then(|v| HeaderValue::from_str(&v).ok()) {
                headers.insert(name, value);
            }
        }
    }

    fn incoming_headers(&self, headers: &mut HeaderMap) {
        remove_hop_by_hop(headers, false);
        let target = self.target.origin();
        let location = headers
            .get(header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| rebase(v, &target, &self.origin));
        if let Some(value) = location.and_then(|v| HeaderValue::from_str(&v).ok()) {
            headers.insert(header::LOCATION, value);
        }
        // The page is shown inside ShadowCode's window.
        headers.remove(header::X_FRAME_OPTIONS);
        let policies: Vec<HeaderValue> = headers
            .get_all(header::CONTENT_SECURITY_POLICY)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .filter_map(|v| HeaderValue::from_str(&without_frame_ancestors(v)).ok())
            .collect();
        headers.remove(header::CONTENT_SECURITY_POLICY);
        for policy in policies.into_iter().filter(|p| !p.is_empty()) {
            headers.append(header::CONTENT_SECURITY_POLICY, policy);
        }
    }

    fn unreachable(&self, error: &anyhow::Error) -> Response<ProxyBody> {
        let refused = error
            .chain()
            .filter_map(|e| e.downcast_ref::<std::io::Error>())
            .any(|e| e.kind() == std::io::ErrorKind::ConnectionRefused);
        let message = if refused {
            format!(
                "Nothing is answering on {}. Start the dev server, then reload.",
                self.target.origin()
            )
        } else {
            format!(
                "The preview could not load {}: {error:#}",
                self.target.origin()
            )
        };
        let page = format!(
            "<!doctype html><meta charset=utf-8><title>Preview unavailable</title>\
             <body style=\"font:14px system-ui;padding:24px;color:#555\"><p>{}</p>",
            escape_html(&message)
        );
        let mut response = Response::new(full(Bytes::from(page)));
        *response.status_mut() = StatusCode::BAD_GATEWAY;
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        );
        response
    }
}

async fn connect(target: &Target) -> Result<TcpStream> {
    let mut last: Option<std::io::Error> = None;
    for address in target.addresses() {
        // Loopback only, whatever the name says.
        ensure!(
            address.ip().is_loopback() || is_mapped_loopback(address),
            "Not a loopback address"
        );
        match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(address)).await {
            Ok(Ok(stream)) => {
                let _ = stream.set_nodelay(true);
                return Ok(stream);
            }
            // "Refused" says the most (nothing listens yet); keep it over a
            // later address's error such as IPv6 being unavailable.
            Ok(Err(error)) => {
                if last
                    .as_ref()
                    .is_none_or(|l| l.kind() != std::io::ErrorKind::ConnectionRefused)
                {
                    last = Some(error)
                }
            }
            Err(_) => {
                last = Some(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "connection timed out",
                ))
            }
        }
    }
    Err(last.map_or_else(
        || anyhow::anyhow!("No loopback address"),
        anyhow::Error::from,
    ))
}

fn is_mapped_loopback(address: SocketAddr) -> bool {
    match address {
        SocketAddr::V6(v6) => v6.ip().to_ipv4_mapped().is_some_and(|ip| ip.is_loopback()),
        SocketAddr::V4(_) => false,
    }
}

fn wants_upgrade(headers: &HeaderMap) -> bool {
    headers.contains_key(header::UPGRADE)
        && headers
            .get_all(header::CONNECTION)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(','))
            .any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
}

/// Connection-scoped headers never cross the proxy; an upgrade keeps
/// `Connection: upgrade` and `Upgrade`.
fn remove_hop_by_hop(headers: &mut HeaderMap, upgrade: bool) {
    let named: Vec<HeaderName> = headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .filter_map(|n| HeaderName::from_bytes(n.trim().as_bytes()).ok())
        .collect();
    for name in named {
        if !(upgrade && name == header::UPGRADE) {
            headers.remove(name);
        }
    }
    for name in [
        "keep-alive",
        "proxy-connection",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailer",
        "transfer-encoding",
    ] {
        headers.remove(name);
    }
    headers.remove(header::CONNECTION);
    if upgrade {
        headers.insert(header::CONNECTION, HeaderValue::from_static("upgrade"));
    } else {
        headers.remove(header::UPGRADE);
    }
}

/// `from` + rest → `to` + rest, when `value` is `from` or starts with `from/`.
fn rebase(value: &str, from: &str, to: &str) -> Option<String> {
    let head = value.get(..from.len())?;
    let rest = &value[from.len()..];
    (head.eq_ignore_ascii_case(from) && (rest.is_empty() || rest.starts_with(['/', '?', '#'])))
        .then(|| format!("{to}{rest}"))
}

/// An uncompressed HTML response: the only kind the picker is added to.
pub fn is_html(headers: &HeaderMap) -> bool {
    let html = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            let mime = v.split(';').next().unwrap_or("").trim();
            mime.eq_ignore_ascii_case("text/html")
                || mime.eq_ignore_ascii_case("application/xhtml+xml")
        });
    let plain = headers
        .get(header::CONTENT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .is_none_or(|v| v.trim().is_empty() || v.trim().eq_ignore_ascii_case("identity"));
    html && plain
}

/// Add the picker `<script>` as the first thing in `<head>` (so it sees the
/// page's first console errors), else after `<html>`, else after the doctype,
/// else at the start.
pub fn inject_picker(html: &[u8]) -> Vec<u8> {
    let scan = &html[..html.len().min(256 * 1024)];
    let lower = scan.to_ascii_lowercase();
    let after_tag = |name: &[u8]| -> Option<usize> {
        let mut from = 0;
        while let Some(found) = find(&lower[from..], name) {
            let start = from + found;
            let next = lower.get(start + name.len()).copied();
            if matches!(next, Some(b'>' | b' ' | b'\t' | b'\n' | b'\r' | b'/')) {
                let end = find(&lower[start..], b">")?;
                return Some(start + end + 1);
            }
            from = start + name.len();
        }
        None
    };
    let at = after_tag(b"<head")
        .or_else(|| after_tag(b"<html"))
        .or_else(|| after_tag(b"<!doctype"))
        .unwrap_or(0);
    let mut out = Vec::with_capacity(html.len() + PICKER_TAG.len());
    out.extend_from_slice(&html[..at]);
    out.extend_from_slice(PICKER_TAG.as_bytes());
    out.extend_from_slice(&html[at..]);
    out
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn without_frame_ancestors(policy: &str) -> String {
    policy
        .split(';')
        .map(str::trim)
        .filter(|d| {
            !d.is_empty()
                && !d
                    .split_whitespace()
                    .next()
                    .is_some_and(|name| name.eq_ignore_ascii_case("frame-ancestors"))
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn full(bytes: Bytes) -> ProxyBody {
    Full::new(bytes).map_err(|never| match never {}).boxed()
}
fn empty() -> ProxyBody {
    Empty::new().map_err(|never| match never {}).boxed()
}
fn plain(status: StatusCode, message: &'static str) -> Response<ProxyBody> {
    let mut response = Response::new(full(Bytes::from_static(message.as_bytes())));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}

#[cfg(test)]
mod tests;
