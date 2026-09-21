use serde_json::{json, Value};
use shadowcode_core::{
    config::Config,
    control::{Endpoint, Server},
    paths::AppPaths,
    service::{Request, Service},
};
use std::{fs, os::unix::fs::MetadataExt, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};
fn setup() -> (tempfile::TempDir, Service) {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("project");
    fs::create_dir(&workspace).unwrap();
    let paths = AppPaths::isolated(&root.path().join("profile")).unwrap();
    Config::patch(&paths,json!({"trusted_workspaces":[workspace],"model":{"provider":"local","name":"fixture","endpoint":"http://127.0.0.1:9/v1"},"permissions":{"approve_shell":false}})).unwrap();
    (root, Service::open(paths, Some(workspace)).unwrap())
}
fn request(method: &str, path: &str, body: Value) -> Request {
    Request {
        method: method.into(),
        path: path.into(),
        body,
    }
}
async fn reply(stream: &mut UnixStream) -> Value {
    let length = stream.read_u32().await.unwrap();
    let mut bytes = vec![0; length as usize];
    stream.read_exact(&mut bytes).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}
async fn stop(server: Server, service: Service) {
    server.close();
    server.wait_closed().await;
    service.engine.shutdown().await.unwrap();
}
#[tokio::test]
async fn private_socket_shares_engine_without_changing_desktop_selection() {
    let (root, service) = setup();
    let primary = service.workspace().unwrap();
    let desktop = service
        .dispatch(request(
            "POST",
            "/api/sessions",
            json!({"workspace":primary}),
        ))
        .await
        .unwrap();
    let server = Server::start(service.clone()).unwrap();
    assert_eq!(
        fs::metadata(server.endpoint().path()).unwrap().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(server.endpoint().path().parent().unwrap())
            .unwrap()
            .mode()
            & 0o777,
        0o700
    );
    let other = root.path().join("other");
    fs::create_dir(&other).unwrap();
    let client = server.endpoint().client(other.clone(), None);
    assert!(client.available().await.unwrap());
    let result = client
        .dispatch(request(
            "POST",
            "/api/projects/trust",
            json!({"path":other}),
        ))
        .await
        .unwrap();
    assert!(result["session_id"].as_str().is_some());
    let session = client
        .dispatch(request("POST", "/api/sessions", json!({"workspace":other})))
        .await
        .unwrap();
    let output = client
        .dispatch(request(
            "POST",
            "/api/workspace/exec",
            json!({"command":"printf local-control","session_id":session["id"]}),
        ))
        .await
        .unwrap();
    assert_eq!(output["stdout"], "local-control");
    assert_eq!(service.workspace().unwrap(), primary);
    assert_eq!(
        service.engine.paths().remembered_workspace().unwrap(),
        primary
    );
    assert_ne!(desktop["id"], session["id"]);
    let event = service
        .engine
        .store()
        .recent_events(session["id"].as_str().unwrap(), 10)
        .unwrap();
    assert!(event
        .iter()
        .any(|event| event["type"] == "terminal.completed"));
    assert!(client
        .dispatch(request(
            "POST",
            "/api/workspace/exec",
            json!({"command":"touch unexpected","session_id":desktop["id"]})
        ))
        .await
        .is_err());
    assert!(!other.join("unexpected").exists());
    stop(server, service).await;
}
#[tokio::test]
async fn client_disconnect_cancels_manual_command_and_releases_workspace() {
    let (_root, service) = setup();
    let workspace = service.workspace().unwrap();
    let server = Server::start(service.clone()).unwrap();
    let client = server.endpoint().client(workspace.clone(), None);
    let operation = tokio::spawn(async move {
        client
            .dispatch(request(
                "POST",
                "/api/workspace/exec",
                json!({"command":"sleep 60 & echo $! > child.pid; wait","timeout":120}),
            ))
            .await
    });
    let pid = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            // Shell redirection creates the file before echo writes the PID.
            // An empty value would accidentally probe /proc/stat below.
            if let Some(pid) = fs::read_to_string(workspace.join("child.pid"))
                .ok()
                .and_then(|value| value.trim().parse::<u32>().ok())
                .filter(|pid| *pid > 0)
            {
                break pid;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    operation.abort();
    let _ = operation.await;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let alive = fs::read_to_string(format!("/proc/{pid}/stat"))
                .is_ok_and(|stat| !stat.contains(") Z "));
            if !alive {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let client = server.endpoint().client(workspace, None);
    let next = client
        .dispatch(request(
            "POST",
            "/api/workspace/exec",
            json!({"command":"printf ready"}),
        ))
        .await
        .unwrap();
    assert_eq!(next["stdout"], "ready");
    stop(server, service).await;
}
#[tokio::test]
async fn malformed_and_oversized_requests_do_not_break_the_owner() {
    let (_root, service) = setup();
    let server = Server::start(service.clone()).unwrap();
    let mut stream = UnixStream::connect(server.endpoint().path()).await.unwrap();
    stream.write_u32(9_000_000).await.unwrap();
    assert!(reply(&mut stream).await["error"]
        .as_str()
        .unwrap()
        .contains("size limit"));
    let mut stream = UnixStream::connect(server.endpoint().path()).await.unwrap();
    stream.write_u32(1).await.unwrap();
    stream.write_all(b"{").await.unwrap();
    assert!(reply(&mut stream).await["error"]
        .as_str()
        .unwrap()
        .contains("Invalid local request"));
    let mut stream = UnixStream::connect(server.endpoint().path()).await.unwrap();
    let body=json!({"protocol":999,"profile":"wrong","workspace":service.workspace().unwrap(),"session_id":null,"request":{"method":"POST","path":"/api/workspace/exec","body":{"command":"touch unexpected"}}}).to_string();
    stream.write_u32(body.len() as u32).await.unwrap();
    stream.write_all(body.as_bytes()).await.unwrap();
    assert!(reply(&mut stream).await["error"]
        .as_str()
        .unwrap()
        .contains("mismatch"));
    assert!(!service.workspace().unwrap().join("unexpected").exists());
    assert!(server
        .endpoint()
        .client(service.workspace().unwrap(), None)
        .available()
        .await
        .unwrap());
    stop(server, service).await;
}
#[tokio::test]
async fn active_and_non_socket_endpoints_are_never_replaced() {
    let (_root, service) = setup();
    let endpoint = Endpoint::for_paths(service.engine.paths()).unwrap();
    fs::write(endpoint.path(), "Preserve this file").unwrap();
    assert!(Server::start(service.clone()).is_err());
    assert_eq!(
        fs::read_to_string(endpoint.path()).unwrap(),
        "Preserve this file"
    );
    fs::remove_file(endpoint.path()).unwrap();
    let server = Server::start(service.clone()).unwrap();
    assert!(Server::start(service.clone()).is_err());
    assert!(endpoint
        .client(service.workspace().unwrap(), None)
        .available()
        .await
        .unwrap());
    stop(server, service).await;
    assert!(!endpoint.path().exists());
}
#[tokio::test]
async fn server_shutdown_releases_idle_connections_and_the_profile_lock() {
    let (_root, service) = setup();
    let paths = service.engine.paths().clone();
    let workspace = service.workspace().unwrap();
    let server = Server::start(service.clone()).unwrap();
    let mut idle = UnixStream::connect(server.endpoint().path()).await.unwrap();
    idle.write_u32(100).await.unwrap();
    idle.write_all(b"partial").await.unwrap();
    assert!(Service::open(paths.clone(), Some(workspace.clone())).is_err());
    let endpoint = server.endpoint().clone();
    stop(server, service).await;
    let next = Service::open(paths, Some(workspace)).unwrap();
    assert!(!endpoint.path().exists());
    next.engine.shutdown().await.unwrap();
}
#[tokio::test]
async fn temporary_command_owner_allows_observation_but_rejects_new_work() {
    let (_root, service) = setup();
    let server = Server::start_with_mode(service.clone(), "command").unwrap();
    let client = server.endpoint().client(service.workspace().unwrap(), None);
    let info = client
        .dispatch(request("GET", "/api/runtime", Value::Null))
        .await
        .unwrap();
    assert_eq!(info["persistent"], false);
    assert!(client
        .dispatch(request("GET", "/api/jobs", Value::Null))
        .await
        .is_ok());
    assert!(client
        .dispatch(request(
            "POST",
            "/api/workspace/exec",
            json!({"command":"touch unexpected"})
        ))
        .await
        .unwrap_err()
        .to_string()
        .contains("foreground CLI"));
    assert!(!service.workspace().unwrap().join("unexpected").exists());
    stop(server, service).await;
}
