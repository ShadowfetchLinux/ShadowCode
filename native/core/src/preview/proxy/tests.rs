use super::*;
use std::sync::Mutex as StdMutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const PAGE: &str = "<!DOCTYPE html>\n<HTML lang=en><Head>\n<title>Demo</title></Head><body><header>x</header><button>Save</button></body></html>";

/// A tiny dev server: HTML at `/`, JavaScript at `/app.js`, a redirect, a
/// page that forbids framing, and an echoing WebSocket-style upgrade at `/ws`.
/// It records each request line with its Host, Origin and Accept-Encoding.
async fn dev_server() -> (u16, Arc<StdMutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(StdMutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let log = log.clone();
            tokio::spawn(async move {
                let mut head = Vec::new();
                let mut byte = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") {
                    if stream.read(&mut byte).await.unwrap_or(0) == 0 {
                        return;
                    }
                    head.push(byte[0]);
                }
                let head = String::from_utf8_lossy(&head).to_string();
                let line = head.lines().next().unwrap_or("").to_owned();
                let header = |name: &str| {
                    head.lines()
                        .find_map(|l| {
                            let (k, v) = l.split_once(':')?;
                            k.eq_ignore_ascii_case(name).then(|| v.trim().to_owned())
                        })
                        .unwrap_or_default()
                };
                log.lock().unwrap().push(format!(
                    "{line} | host={} | origin={} | encoding={} | upgrade={}",
                    header("host"),
                    header("origin"),
                    header("accept-encoding"),
                    header("upgrade"),
                ));
                let path = line.split_whitespace().nth(1).unwrap_or("/").to_owned();
                let respond = |status: &str, kind: &str, extra: &str, body: &str| {
                    format!(
                        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nETag: \"abc\"\r\nConnection: close\r\n{extra}\r\n{body}",
                        body.len()
                    )
                };
                let reply = match path.as_str() {
                    "/" => respond("200 OK", "text/html; charset=utf-8", "", PAGE),
                    "/app.js" => respond(
                        "200 OK",
                        "application/javascript",
                        "",
                        "document.write('<head>');",
                    ),
                    "/redirect" => respond(
                        "302 Found",
                        "text/plain",
                        &format!("Location: http://localhost:{port}/next?x=1\r\n"),
                        "",
                    ),
                    "/framed" => respond(
                        "200 OK",
                        "text/html",
                        "X-Frame-Options: DENY\r\nContent-Security-Policy: default-src 'self'; frame-ancestors 'none'\r\n",
                        "<p>framed</p>",
                    ),
                    "/ws" => {
                        stream
                            .write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: test\r\n\r\n")
                            .await
                            .unwrap();
                        let mut buffer = [0u8; 256];
                        loop {
                            match stream.read(&mut buffer).await {
                                Ok(0) | Err(_) => return,
                                Ok(n) => {
                                    let mut echo = b"echo:".to_vec();
                                    echo.extend_from_slice(&buffer[..n]);
                                    if stream.write_all(&echo).await.is_err() {
                                        return;
                                    }
                                }
                            }
                        }
                    }
                    _ => respond("404 Not Found", "text/plain", "", "missing"),
                };
                let _ = stream.write_all(reply.as_bytes()).await;
                let _ = stream.shutdown().await;
            });
        }
    });
    (port, seen)
}

/// One HTTP/1.1 request with `Connection: close`; returns (head, body).
async fn fetch(port: u16, path: &str, host: &str) -> (String, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nOrigin: http://127.0.0.1:{port}\r\nAccept-Encoding: gzip, br\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), stream.read_to_end(&mut raw))
        .await
        .unwrap()
        .unwrap();
    let raw = String::from_utf8_lossy(&raw).to_string();
    let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((&raw, ""));
    (head.to_owned(), body.to_owned())
}

fn target(port: u16) -> Target {
    Target {
        host: "localhost".into(),
        port,
    }
}

#[tokio::test]
async fn forwards_and_injects_the_picker_into_html_only() {
    let (dev, seen) = dev_server().await;
    let previews = Previews::default();
    let opened = previews
        .open(target(dev), "tauri://localhost", &HashSet::new())
        .await
        .unwrap();
    assert_eq!(opened.target_origin, format!("http://localhost:{dev}"));
    assert_eq!(
        opened.proxy_origin,
        format!("http://127.0.0.1:{}", opened.proxy_port)
    );
    let proxy = opened.proxy_port;
    let host = format!("127.0.0.1:{proxy}");

    // HTML: the script is the first thing in <head>, not in <header>.
    let (head, body) = fetch(proxy, "/", &host).await;
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert!(
        body.starts_with(&format!(
            "<!DOCTYPE html>\n<HTML lang=en><Head>{PICKER_TAG}\n<title>"
        )),
        "{body}"
    );
    assert_eq!(body.matches(PICKER_TAG).count(), 1);
    let lower = head.to_ascii_lowercase();
    assert!(lower.contains(&format!(
        "content-length: {}",
        PAGE.len() + PICKER_TAG.len()
    )));
    assert!(!lower.contains("etag"));

    // JavaScript (even one mentioning <head>) passes through untouched.
    let (head, body) = fetch(proxy, "/app.js", &host).await;
    assert!(head.to_ascii_lowercase().contains("etag: \"abc\""));
    assert_eq!(body, "document.write('<head>');");

    // Redirects to the dev server's own address stay in the proxy.
    let (head, _) = fetch(proxy, "/redirect", &host).await;
    assert!(
        head.to_ascii_lowercase()
            .contains(&format!("location: http://127.0.0.1:{proxy}/next?x=1")),
        "{head}"
    );

    // Framing headers are dropped; the rest of the policy stays.
    let (head, body) = fetch(proxy, "/framed", &host).await;
    let lower = head.to_ascii_lowercase();
    assert!(!lower.contains("x-frame-options"), "{head}");
    assert!(
        lower.contains("content-security-policy: default-src 'self'\r")
            || lower.ends_with("content-security-policy: default-src 'self'"),
        "{head}"
    );
    assert!(body.contains(PICKER_TAG));

    // The picker script is served by the proxy with the app origin baked in.
    let (head, body) = fetch(proxy, PICKER_PATH, &host).await;
    assert!(head.to_ascii_lowercase().contains("text/javascript"));
    assert!(
        body.contains("var APP_ORIGIN = \"tauri://localhost\";"),
        "{}",
        &body[..600]
    );
    // Other reserved paths are never forwarded.
    let (head, _) = fetch(proxy, "/__shadowcode_preview__/secret", &host).await;
    assert!(head.starts_with("HTTP/1.1 404"));

    // Another Host (DNS rebinding) is refused before anything is forwarded.
    let (head, _) = fetch(proxy, "/", &format!("evil.example:{proxy}")).await;
    assert!(head.starts_with("HTTP/1.1 421"), "{head}");

    let seen = seen.lock().unwrap().clone();
    let paths: Vec<&str> = seen.iter().map(|l| l.split(' ').nth(1).unwrap()).collect();
    assert_eq!(paths, vec!["/", "/app.js", "/redirect", "/framed"]);
    for line in &seen {
        assert!(line.contains(&format!("host=localhost:{dev}")), "{line}");
        assert!(
            line.contains(&format!("origin=http://localhost:{dev}")),
            "{line}"
        );
        assert!(line.contains("encoding=identity"), "{line}");
    }

    // Reusing the same target and app gives the same proxy.
    let again = previews
        .open(target(dev), "tauri://localhost", &HashSet::new())
        .await
        .unwrap();
    assert_eq!(again.proxy_port, proxy);
    previews.close_all().await;
}

#[tokio::test]
async fn passes_websocket_upgrades_through() {
    let (dev, seen) = dev_server().await;
    let previews = Previews::default();
    let proxy = previews
        .open(target(dev), "http://127.0.0.1:4178", &HashSet::new())
        .await
        .unwrap()
        .proxy_port;
    let mut stream = TcpStream::connect(("127.0.0.1", proxy)).await.unwrap();
    stream
        .write_all(format!("GET /ws HTTP/1.1\r\nHost: 127.0.0.1:{proxy}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        assert_eq!(stream.read(&mut byte).await.unwrap(), 1);
        head.push(byte[0]);
    }
    let head = String::from_utf8(head).unwrap();
    assert!(head.starts_with("HTTP/1.1 101"), "{head}");
    assert!(head.to_ascii_lowercase().contains("upgrade: websocket"));
    stream.write_all(b"ping").await.unwrap();
    let mut reply = [0u8; 9];
    tokio::time::timeout(Duration::from_secs(5), stream.read_exact(&mut reply))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&reply, b"echo:ping");
    assert!(seen.lock().unwrap()[0].contains("upgrade=websocket"));
}

#[tokio::test]
async fn refuses_non_loopback_and_own_ports() {
    let previews = Previews::default();
    for host in ["example.com", "10.0.0.1", "0.0.0.0", "[fe80::1]"] {
        let error = previews
            .open(
                Target {
                    host: host.into(),
                    port: 80,
                },
                "tauri://localhost",
                &HashSet::new(),
            )
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("only opens servers on this computer"),
            "{error}"
        );
    }
    let error = previews
        .open(target(4321), "tauri://localhost", &HashSet::from([4321]))
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("belongs to ShadowCode"),
        "{error}"
    );
    // A proxy is never a target of another proxy.
    let first = previews
        .open(target(4322), "tauri://localhost", &HashSet::new())
        .await
        .unwrap();
    assert!(previews
        .open(
            target(first.proxy_port),
            "tauri://localhost",
            &HashSet::new()
        )
        .await
        .is_err());
    // The embedding window must be a concrete app origin.
    assert!(previews
        .open(target(4323), "*", &HashSet::new())
        .await
        .is_err());
    assert!(previews
        .open(target(4323), "null", &HashSet::new())
        .await
        .is_err());
}

#[tokio::test]
async fn explains_when_nothing_is_listening() {
    // Bind and drop to find a port that is (very likely) closed.
    let port = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let previews = Previews::default();
    let proxy = previews
        .open(target(port), "tauri://localhost", &HashSet::new())
        .await
        .unwrap()
        .proxy_port;
    let (head, body) = fetch(proxy, "/", &format!("127.0.0.1:{proxy}")).await;
    assert!(head.starts_with("HTTP/1.1 502"), "{head}");
    assert!(
        body.contains("Nothing is answering on http://localhost:"),
        "{body}"
    );
}

#[test]
fn injection_points() {
    let tag = PICKER_TAG;
    assert_eq!(
        String::from_utf8(inject_picker(b"<html><body>x</body></html>")).unwrap(),
        format!("<html>{tag}<body>x</body></html>")
    );
    assert_eq!(
        String::from_utf8(inject_picker(b"<head data-x='1'>t</head>")).unwrap(),
        format!("<head data-x='1'>{tag}t</head>")
    );
    assert_eq!(
        String::from_utf8(inject_picker(b"<header>only</header>")).unwrap(),
        format!("{tag}<header>only</header>")
    );
    assert_eq!(
        String::from_utf8(inject_picker(b"<!doctype html><p>hi")).unwrap(),
        format!("<!doctype html>{tag}<p>hi")
    );
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("Text/HTML; charset=utf-8"),
    );
    assert!(is_html(&headers));
    headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
    assert!(!is_html(&headers));
    headers.remove(header::CONTENT_ENCODING);
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    assert!(!is_html(&headers));
    assert_eq!(
        rebase(
            "http://localhost:5173/a",
            "http://localhost:5173",
            "http://127.0.0.1:9"
        ),
        Some("http://127.0.0.1:9/a".into())
    );
    assert_eq!(
        rebase(
            "http://localhost:51730/a",
            "http://localhost:5173",
            "http://127.0.0.1:9"
        ),
        None
    );
    assert_eq!(without_frame_ancestors("frame-ancestors 'none'"), "");
}
