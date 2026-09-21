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

#[tokio::test]
async fn attached_views_keep_navigation_independent_and_detach_without_stopping_owner() {
    let (root, service) = setup();
    let primary = service.workspace().unwrap();
    let other = root.path().join("view-project");
    fs::create_dir(&other).unwrap();
    let server = Server::start_with_mode(service.clone(), "server").unwrap();
    let client = server.endpoint().client(primary.clone(), None);
    let first = client.open_view().await.unwrap();
    let second = client.open_view().await.unwrap();
    let selected = first
        .dispatch(request(
            "POST",
            "/api/projects/trust",
            json!({"path": other}),
        ))
        .await
        .unwrap();
    let status = first
        .dispatch(request("GET", "/api/workspace/status", Value::Null))
        .await
        .unwrap();
    assert_eq!(status["workspace"], json!(other));
    assert_eq!(
        second
            .dispatch(request("GET", "/api/workspace/status", Value::Null))
            .await
            .unwrap()["workspace"],
        json!(primary)
    );
    assert_eq!(service.workspace().unwrap(), primary);
    // Omitted session_id uses the view's retained conversation selection.
    first
        .dispatch(request(
            "POST",
            "/api/workspace/exec",
            json!({"command": "printf attached-view"}),
        ))
        .await
        .unwrap();
    assert!(service
        .engine
        .store()
        .recent_events(selected["session_id"].as_str().unwrap(), 10)
        .unwrap()
        .iter()
        .any(|e| e["type"] == "terminal.completed"));
    first.close().await.unwrap();
    first.close().await.unwrap();
    assert!(first
        .dispatch(request("GET", "/api/health", Value::Null))
        .await
        .is_err());
    assert!(client.available().await.unwrap());
    assert_eq!(
        second
            .dispatch(request("GET", "/api/health", Value::Null))
            .await
            .unwrap()["workspace"],
        json!(primary)
    );
    second.close().await.unwrap();
    stop(server, service).await;
}

#[tokio::test]
async fn attached_view_leases_are_bounded_and_dropped_connections_release_slots() {
    let (_root, service) = setup();
    let server = Server::start_with_mode(service.clone(), "tui").unwrap();
    let client = server.endpoint().client(service.workspace().unwrap(), None);
    let mut views = Vec::new();
    for _ in 0..4 {
        views.push(client.open_view().await.unwrap());
    }
    let error = client.open_view().await.err().unwrap().to_string();
    assert!(error.contains("At most four"), "{error}");
    drop(views.pop());
    let replacement = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match client.open_view().await {
                Ok(view) => break view,
                Err(error) => {
                    assert!(error.to_string().contains("At most four"), "{error}");
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
        }
    })
    .await
    .unwrap();
    replacement.close().await.unwrap();
    drop(views);
    stop(server, service).await;
}

#[tokio::test]
async fn foreground_command_cannot_become_a_desktop_engine_owner() {
    let (_root, service) = setup();
    let server = Server::start_with_mode(service.clone(), "command").unwrap();
    let client = server.endpoint().client(service.workspace().unwrap(), None);
    assert!(client
        .open_view()
        .await
        .err()
        .unwrap()
        .to_string()
        .contains("foreground CLI"));
    stop(server, service).await;
}

#[tokio::test]
async fn attached_view_status_is_available_during_slow_manual_execution() {
    let (_root, service) = setup();
    let workspace = service.workspace().unwrap();
    let server = Server::start_with_mode(service.clone(), "server").unwrap();
    let view = std::sync::Arc::new(
        server
            .endpoint()
            .client(workspace.clone(), None)
            .open_view()
            .await
            .unwrap(),
    );
    let executing = view.clone();
    let operation = tokio::spawn(async move {
        executing
            .dispatch(request(
                "POST",
                "/api/workspace/exec",
                json!({"command": "touch started; sleep 30", "timeout": 60}),
            ))
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while !workspace.join("started").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let status = tokio::time::timeout(
        Duration::from_secs(2),
        view.dispatch(request("GET", "/api/health", Value::Null)),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(status["ok"], true);
    operation.abort();
    assert!(operation.await.unwrap_err().is_cancelled());
    view.close().await.unwrap();
    stop(server, service).await;
}

#[tokio::test]
async fn attached_views_receive_bounded_completion_notifications_and_detect_owner_exit() {
    let (_root, service) = setup();
    let server = Server::start_with_mode(service.clone(), "server").unwrap();
    let client = server.endpoint().client(service.workspace().unwrap(), None);
    let view = client.open_view().await.unwrap();
    let mut events = view.subscribe();
    let started = view
        .dispatch(request(
            "POST",
            "/api/jobs",
            json!({"task":"Exercise provider failure notification"}),
        ))
        .await
        .unwrap();
    let completed = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let event = events.recv().await.unwrap();
            assert!(serde_json::to_vec(&event).unwrap().len() < 16_384);
            if event["type"] == "agent.completed" {
                break event;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(completed["session_id"], started["session_id"]);
    assert!(!completed["payload"]["summary"].as_str().unwrap().is_empty());
    assert!(
        completed.get("id").is_none(),
        "Wakeup messages are not durable event rows"
    );
    server.close();
    server.wait_closed().await;
    let disconnected = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let event = events.recv().await.unwrap();
            if event["type"] == "view.disconnected" {
                break event;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(disconnected["type"], "view.disconnected");
    assert!(view
        .dispatch(request("GET", "/api/health", Value::Null))
        .await
        .unwrap_err()
        .to_string()
        .contains("closed"));
    let _ = view.close().await;
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn attached_view_reattaches_after_owner_restart_without_replay_or_duplicates() {
    let (_root, service) = setup();
    let paths = service.engine.paths().clone();
    let workspace = service.workspace().unwrap();
    let server = Server::start_with_mode(service.clone(), "server").unwrap();
    let client = server.endpoint().client(workspace.clone(), None);
    let view = client.open_view().await.unwrap();
    let mut events = view.subscribe();
    let started = view
        .dispatch(request(
            "POST",
            "/api/jobs",
            json!({"task": "Exercise provider failure notification"}),
        ))
        .await
        .unwrap();
    let session_id = started["session_id"].as_str().unwrap().to_owned();
    let _ = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let event = events.recv().await.unwrap();
            if event["type"] == "agent.completed" {
                break;
            }
        }
    })
    .await;
    let before = view
        .dispatch(request(
            "GET",
            &format!("/api/events?session_id={session_id}"),
            Value::Null,
        ))
        .await
        .unwrap();
    let before_ids: Vec<i64> = before["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|event| event["id"].as_i64())
        .collect();
    assert!(!before_ids.is_empty());
    assert!(view
        .reattach()
        .await
        .unwrap_err()
        .to_string()
        .contains("still connected"));
    server.close();
    server.wait_closed().await;
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let event = events.recv().await.unwrap();
            if event["type"] == "view.disconnected" {
                break;
            }
        }
    })
    .await
    .unwrap();
    service.engine.shutdown().await.unwrap();
    drop(service);

    let service = Service::open(paths, Some(workspace)).unwrap();
    let server = Server::start_with_mode(service.clone(), "server").unwrap();
    let attached = view.reattach().await.unwrap();
    assert_eq!(attached["reattached"], true);
    assert_eq!(attached["jobs_started"], 0);
    assert_eq!(attached["tools_replayed"], 0);
    let notice = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let event = events.recv().await.unwrap();
            if event["type"] == "view.reattached" {
                break event;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(notice["jobs_started"], 0);
    assert_eq!(notice["tools_replayed"], 0);
    let health = view
        .dispatch(request("GET", "/api/health", Value::Null))
        .await
        .unwrap();
    assert_eq!(health["ok"], true);
    let after = view
        .dispatch(request(
            "GET",
            &format!("/api/events?session_id={session_id}"),
            Value::Null,
        ))
        .await
        .unwrap();
    let after_ids: Vec<i64> = after["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|event| event["id"].as_i64())
        .collect();
    for id in &before_ids {
        assert!(after_ids.contains(id), "lost event {id}");
    }
    let unique: std::collections::HashSet<_> = after_ids.iter().collect();
    assert_eq!(
        unique.len(),
        after_ids.len(),
        "duplicate event ids after reattach"
    );
    let jobs = view
        .dispatch(request("GET", "/api/jobs", Value::Null))
        .await
        .unwrap();
    let running = jobs["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|job| job["status"] == "running" || job["status"] == "queued")
        .count();
    assert_eq!(running, 0, "reattach must not start or resume jobs");
    stop(server, service).await;
}
