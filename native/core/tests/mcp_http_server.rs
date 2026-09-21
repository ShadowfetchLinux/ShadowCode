#![cfg(unix)]
mod support;
use reqwest::{header, StatusCode};
use serde_json::{json, Value};
use shadowcode_core::{
    config::Config,
    control,
    engine::StartRequest,
    mcp::{
        http::HttpSpec,
        server::{http, Access},
        Client,
    },
    paths::AppPaths,
    service::Service,
};
use std::{fs, path::PathBuf, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

const TOKEN: &str = "test-only-http-bearer-credential-1234567890";
struct Fixture {
    _root: tempfile::TempDir,
    project: PathBuf,
    other: PathBuf,
    paths: AppPaths,
    service: Service,
    control: control::Server,
}
impl Fixture {
    fn new(endpoint: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        let other = root.path().join("other");
        fs::create_dir(&project).unwrap();
        fs::create_dir(&other).unwrap();
        let paths = AppPaths::isolated(&root.path().join("profile")).unwrap();
        Config::patch(&paths,json!({"model":{"provider":"local","name":"fixture","default":"fixture","endpoint":endpoint,"context_limit":16384},"trusted_workspaces":[project,other],"agent":{"max_steps":5,"model_retries":0}})).unwrap();
        let service = Service::open(paths.clone(), Some(other.clone())).unwrap();
        let control = control::Server::start_with_mode(service.clone(), "server").unwrap();
        Self {
            _root: root,
            project,
            other,
            paths,
            service,
            control,
        }
    }
    async fn gateway(&self, access: Access) -> Gateway {
        Gateway::open(self.paths.clone(), self.project.clone(), access).await
    }
    async fn close(&self) {
        self.control.close();
        self.control.wait_closed().await;
        self.service.engine.shutdown().await.unwrap();
    }
}
struct Gateway {
    url: String,
    address: std::net::SocketAddr,
    cancel: CancellationToken,
    worker: Option<JoinHandle<anyhow::Result<()>>>,
}
impl Drop for Gateway {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
impl Gateway {
    async fn open(paths: AppPaths, project: PathBuf, access: Access) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let cancel = CancellationToken::new();
        let (ready, rx) = tokio::sync::oneshot::channel();
        let worker = tokio::spawn(http::serve(
            paths,
            project,
            access,
            listener,
            TOKEN.into(),
            cancel.clone(),
            Some(ready),
        ));
        rx.await.unwrap();
        Self {
            url: format!("http://{address}/mcp"),
            address,
            cancel,
            worker: Some(worker),
        }
    }
    async fn client(&self) -> Client {
        Client::connect_http(
            &HttpSpec {
                url: self.url.clone(),
                bearer_token: Some(TOKEN.into()),
                timeout: Duration::from_secs(5),
            },
            CancellationToken::new(),
        )
        .await
        .unwrap()
    }
    fn request(&self, body: Value) -> reqwest::RequestBuilder {
        self.request_version(body, "2025-11-25")
    }
    fn request_version(&self, body: Value, version: &str) -> reqwest::RequestBuilder {
        reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(8))
            .build()
            .unwrap()
            .post(&self.url)
            .bearer_auth(TOKEN)
            .header(header::ACCEPT, "application/json, text/event-stream")
            .header("mcp-protocol-version", version)
            .json(&body)
    }
    async fn close(mut self) {
        self.cancel.cancel();
        tokio::time::timeout(Duration::from_secs(8), self.worker.take().unwrap())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(TcpStream::connect(self.address).await.is_err());
    }
}
async fn call(client: &mut Client, name: &str, args: Value) -> Value {
    client.call(name, args).await.unwrap()
}
fn rpc(method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":1,"method":method,"params":params})
}
fn dead(pid: u32) -> bool {
    fs::read_to_string(format!("/proc/{pid}/stat")).map_or(true, |stat| {
        stat.rsplit_once(") ")
            .is_some_and(|(_, state)| state.starts_with('Z') || state.starts_with('X'))
    })
}

#[tokio::test]
async fn modern_sdk_and_legacy_requests_share_real_catalog_with_read_only_defaults() {
    let f = Fixture::new("http://127.0.0.1:1/v1");
    let visible = f
        .service
        .engine
        .store()
        .create_session(&f.project, "fixture", "visible")
        .unwrap();
    f.service
        .engine
        .store()
        .create_session(&f.other, "fixture", "private-other-project")
        .unwrap();
    let g = f.gateway(Access::default()).await;
    let mut c = g.client().await;
    assert_eq!(c.tools().len(), 17);
    let sessions = call(&mut c, "shadow_sessions", json!({})).await;
    assert!(!sessions.to_string().contains("private-other-project"));
    assert!(sessions
        .to_string()
        .contains(visible["id"].as_str().unwrap()));
    let denied = call(
        &mut c,
        "shadow_memory",
        json!({"action":"append","note":"must not write"}),
    )
    .await;
    assert_eq!(denied["isError"], true);
    let cross = call(&mut c, "shadow_understand", json!({"workspace":f.other})).await;
    assert_eq!(cross["isError"], true);
    assert_eq!(f.service.workspace().unwrap(), f.other);
    let initialized=g.request(rpc("initialize",json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"legacy-http","version":"1"}}))).send().await.unwrap();
    assert_eq!(initialized.status(), StatusCode::OK);
    assert!(initialized.headers().get("mcp-session-id").is_none());
    assert!(
        initialized.json::<Value>().await.unwrap()["result"]["serverInfo"]["name"] == "ShadowCode"
    );
    let notification = g
        .request(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
        .send()
        .await
        .unwrap();
    assert_eq!(notification.status(), StatusCode::ACCEPTED);
    assert!(notification.bytes().await.unwrap().is_empty());
    for method in ["tools/list", "resources/list", "prompts/list"] {
        let result = g.request(rpc(method, json!({}))).send().await.unwrap();
        assert_eq!(result.status(), StatusCode::OK, "{method}");
        assert!(result
            .json::<Value>()
            .await
            .unwrap()
            .get("result")
            .is_some());
    }
    c.close().await.unwrap();
    g.close().await;
    f.close().await;
}

#[tokio::test]
async fn authentication_host_origin_paths_and_protocol_errors_are_inert_and_private() {
    let f = Fixture::new("http://127.0.0.1:1/v1");
    let g = f.gateway(Access::default()).await;
    let body = rpc("tools/list", json!({}));
    for request in [
        g.request(body.clone())
            .header(header::AUTHORIZATION, "Bearer wrong"),
        g.request(body.clone())
            .header(header::AUTHORIZATION, "Basic wrong"),
    ] {
        let result = request.send().await.unwrap();
        assert_eq!(result.status(), StatusCode::UNAUTHORIZED);
        assert!(result.headers().contains_key(header::WWW_AUTHENTICATE));
        assert!(!result.text().await.unwrap().contains(TOKEN));
    }
    let wrong = reqwest::Client::new()
        .post(&g.url)
        .bearer_auth("wrong-single-credential-1234567890")
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
    let duplicate = g
        .request(body.clone())
        .header("mcp-protocol-version", "2025-11-25")
        .send()
        .await
        .unwrap();
    assert_eq!(duplicate.status(), StatusCode::BAD_REQUEST);
    let missing = reqwest::Client::new()
        .post(&g.url)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);
    for (name, value) in [
        ("origin", "https://untrusted.example"),
        ("origin", "null"),
        ("host", "untrusted.example"),
    ] {
        let result = g
            .request(body.clone())
            .header(name, value)
            .send()
            .await
            .unwrap();
        assert_eq!(result.status(), StatusCode::FORBIDDEN);
        assert!(!result.text().await.unwrap().contains(TOKEN));
    }
    let query = reqwest::Client::new()
        .post(format!("{}?token={TOKEN}", g.url))
        .bearer_auth(TOKEN)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(query.status(), StatusCode::NOT_FOUND);
    assert!(!query.text().await.unwrap().contains(TOKEN));
    for method in [reqwest::Method::GET, reqwest::Method::DELETE] {
        let result = reqwest::Client::new()
            .request(method, &g.url)
            .bearer_auth(TOKEN)
            .header(header::ACCEPT, "text/event-stream")
            .send()
            .await
            .unwrap();
        assert_eq!(result.status(), StatusCode::METHOD_NOT_ALLOWED);
    }
    let bad_version = g
        .request_version(body.clone(), "2099-01-01")
        .send()
        .await
        .unwrap();
    assert_eq!(bad_version.status(), StatusCode::BAD_REQUEST);
    let mismatched=g.request_version(rpc("tools/call",json!({"name":"shadow_status","arguments":{},"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}})), "2026-07-28")
        .header("mcp-method","tools/call").header("mcp-name","shadow_run").send().await.unwrap();
    assert_eq!(mismatched.status(), StatusCode::BAD_REQUEST);
    let malformed = g
        .request(Value::Null)
        .header(header::CONTENT_TYPE, "application/json")
        .body("{malformed")
        .send()
        .await
        .unwrap();
    // rmcp's bounded JSON parser deliberately returns 415 for an invalid
    // representation; malformed input must never reach a native tool.
    assert_eq!(malformed.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let oversized = g
        .request(Value::Null)
        .body("x".repeat(1_048_577))
        .send()
        .await
        .unwrap();
    assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert!(f.service.engine.store().jobs(10).unwrap().is_empty());
    let mut c = g.client().await;
    assert_eq!(
        call(&mut c, "shadow_status", json!({})).await["isError"],
        false
    );
    c.close().await.unwrap();
    g.close().await;
    f.close().await;
}

#[tokio::test]
async fn owned_tasks_survive_http_reconnect_and_shutdown_cancels_only_this_gateway() {
    let model=support::server(|_,_| (json!({"choices":[{"message":{"role":"assistant","content":"late reply"},"finish_reason":"stop"}]}),Duration::from_secs(30))).await;
    let f = Fixture::new(&model.endpoint);
    let unrelated = f
        .service
        .engine
        .start(StartRequest {
            workspace: f.other.clone(),
            task: "unrelated".into(),
            session_id: None,
            model: None,
            mode: "review".into(),
            queue: false,
            images: Vec::new(),
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while model.requests.lock().unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let g = f
        .gateway(Access {
            allow_write: true,
            allow_approvals: true,
        })
        .await;
    let independent = f.gateway(Access::default()).await;
    let mut c = g.client().await;
    let started = call(
        &mut c,
        "shadow_test",
        json!({"command":"sleep 60 & echo $! > child.pid; wait","timeout":120}),
    )
    .await;
    assert_eq!(started["isError"], false, "{started}");
    let job = started["structuredContent"]["job"].clone();
    let id = job["id"].as_str().unwrap();
    let mut other = independent.client().await;
    let leaked = call(&mut other, "shadow_jobs", json!({"job_id":id})).await;
    assert_eq!(leaked["isError"], true);
    let approval = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let result = call(&mut c, "shadow_jobs", json!({"job_id":id})).await;
            if let Some(approval) = result["structuredContent"]["approvals"]
                .as_array()
                .unwrap()
                .first()
            {
                break approval.clone();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(approval["command"].as_str().unwrap().contains("child.pid"));
    assert!(!f.project.join("child.pid").exists());
    let approved = call(
        &mut c,
        "shadow_approve",
        json!({"approval_id":approval["id"],"decision":"approve"}),
    )
    .await;
    assert_eq!(approved["isError"], false, "{approved}");
    let pid = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(pid) = fs::read_to_string(f.project.join("child.pid"))
                .ok()
                .and_then(|v| v.trim().parse::<u32>().ok())
            {
                break pid;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(!dead(pid));
    c.close().await.unwrap();
    let mut c = g.client().await;
    assert_eq!(
        call(&mut c, "shadow_jobs", json!({"job_id":id})).await["structuredContent"]["job"]
            ["status"],
        "running"
    );
    assert!(!dead(pid));
    let queued = call(
        &mut c,
        "shadow_run",
        json!({"task":"cancel without model request","queue":true}),
    )
    .await;
    assert_eq!(queued["isError"], false, "{queued}");
    let queued_id = queued["structuredContent"]["job"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    c.close().await.unwrap();
    g.close().await;
    assert!(dead(pid));
    assert_eq!(
        model.requests.lock().unwrap().len(),
        1,
        "queued task must never reach the model"
    );
    assert_eq!(
        f.service.engine.job(id).unwrap().unwrap().status,
        "cancelled"
    );
    assert_eq!(
        f.service.engine.job(&queued_id).unwrap().unwrap().status,
        "cancelled"
    );
    assert_eq!(
        f.service.engine.job(&unrelated.id).unwrap().unwrap().status,
        "running"
    );
    assert_eq!(
        call(&mut other, "shadow_status", json!({})).await["isError"],
        false
    );
    other.close().await.unwrap();
    independent.close().await;
    f.close().await;
}

#[tokio::test]
async fn slow_partial_requests_and_abandoned_gateway_release_sockets_and_temporary_profile() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    fs::create_dir(&project).unwrap();
    let paths = AppPaths::isolated(&root.path().join("profile")).unwrap();
    let g = Gateway::open(paths.clone(), project.clone(), Access::default()).await;
    let mut stream = TcpStream::connect(g.address).await.unwrap();
    stream.write_all(format!("POST /mcp HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {TOKEN}\r\nContent-Type: application/json\r\nContent-Length: 100\r\n\r\n{{",g.address).as_bytes()).await.unwrap();
    let mut wire = String::new();
    tokio::time::timeout(Duration::from_secs(8), stream.read_to_string(&mut wire))
        .await
        .unwrap()
        .unwrap();
    assert!(wire.starts_with("HTTP/1.1 408"), "{wire}");
    let mut header = TcpStream::connect(g.address).await.unwrap();
    header
        .write_all(b"POST /mcp HTTP/1.1\r\nHost:")
        .await
        .unwrap();
    g.worker.as_ref().unwrap().abort();
    drop(g);
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            match Service::open(paths.clone(), Some(project.clone())) {
                Ok(service) => {
                    service.engine.shutdown().await.unwrap();
                    break;
                }
                Err(error) => {
                    assert!(error.to_string().contains("already running"), "{error:#}");
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
        }
    })
    .await
    .unwrap();
    let mut byte = [0u8; 1];
    let closed = tokio::time::timeout(Duration::from_secs(2), header.read(&mut byte))
        .await
        .unwrap();
    assert!(
        matches!(closed, Ok(0))
            || matches!(closed, Err(ref error) if error.kind() == std::io::ErrorKind::ConnectionReset),
        "abandoned socket must close: {closed:?}"
    );
}

#[test]
fn gateway_validation_rejects_external_binds_and_weak_or_invalid_credentials() {
    assert!(http::validate("0.0.0.0:8000".parse().unwrap(), TOKEN).is_err());
    assert!(http::validate("[::]:8000".parse().unwrap(), TOKEN).is_err());
    for token in [
        "",
        "short",
        "long-token-with-a-space-1234567890 x",
        "long-token-with-newline-1234567890\n",
    ] {
        assert!(http::validate("127.0.0.1:0".parse().unwrap(), token).is_err());
    }
    assert!(http::validate("[::1]:0".parse().unwrap(), TOKEN).is_ok());
}
