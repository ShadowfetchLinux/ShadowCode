mod support;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use shadowcode_core::{
    config::Config, control::Server, engine::StartRequest, paths::AppPaths, service::Service,
};
use std::{fs, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};

async fn send(stream: &mut UnixStream, value: Value) {
    let bytes = value.to_string().into_bytes();
    stream.write_u32(bytes.len() as u32).await.unwrap();
    stream.write_all(&bytes).await.unwrap();
}
async fn read(stream: &mut UnixStream) -> Value {
    let count = stream.read_u32().await.unwrap();
    let mut bytes = vec![0; count as usize];
    stream.read_exact(&mut bytes).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}
async fn raw_owner(server: &Server, service: &Service) -> UnixStream {
    let paths = service.engine.paths();
    let mut identity = Vec::new();
    for root in [&paths.config, &paths.data, &paths.state] {
        identity.extend_from_slice(root.canonicalize().unwrap().as_os_str().as_encoded_bytes());
        identity.push(0);
    }
    let profile = format!("{:x}", Sha256::digest(identity));
    let mut stream = UnixStream::connect(server.endpoint().path()).await.unwrap();
    send(&mut stream,json!({"protocol":1,"profile":profile,"workspace":service.workspace().unwrap(),"session_id":null,"request":{"method":"POST","path":"/api/owned-jobs","body":null}})).await;
    assert_eq!(read(&mut stream).await["result"]["owned_jobs"], true);
    stream
}

fn setup(endpoint: &str) -> (tempfile::TempDir, Service) {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    fs::create_dir(&project).unwrap();
    let paths = AppPaths::isolated(&root.path().join("profile")).unwrap();
    Config::patch(&paths, json!({"model":{"provider":"local","name":"fixture","default":"fixture","endpoint":endpoint},"trusted_workspaces":[project],"agent":{"model_retries":0}})).unwrap();
    (root, Service::open(paths, Some(project)).unwrap())
}
async fn done(service: &Service, id: &Value) -> String {
    tokio::time::timeout(
        Duration::from_secs(4),
        service.engine.wait(id.as_str().unwrap()),
    )
    .await
    .unwrap()
    .unwrap()
    .status
}

#[tokio::test]
async fn dropping_an_owner_cancels_queued_jobs_without_waiting_for_an_unrelated_task() {
    let model = support::server(|_, _| (json!({}), Duration::from_secs(60))).await;
    let (_root, service) = setup(&model.endpoint);
    let workspace = service.workspace().unwrap();
    let unrelated = service
        .engine
        .start(StartRequest {
            workspace: workspace.clone(),
            task: "Unrelated foreground task".into(),
            session_id: None,
            model: None,
            mode: "plan".into(),
            queue: false,
        
            images: Vec::new(),
        })
        .await
        .unwrap();
    let server = Server::start(service.clone()).unwrap();
    let client = server.endpoint().client(workspace, None);
    let owner = client.own_jobs().await.unwrap();
    let a = owner
        .submit(json!({"task":"Owned queue one","queue":true}))
        .await
        .unwrap();
    let b = owner
        .submit(json!({"task":"Owned queue two","queue":true}))
        .await
        .unwrap();
    assert_eq!(a["status"], "queued");
    drop(owner);
    assert_eq!(done(&service, &a["id"]).await, "cancelled");
    assert_eq!(done(&service, &b["id"]).await, "cancelled");
    assert_eq!(
        service.engine.job(&unrelated.id).unwrap().unwrap().status,
        "running"
    );
    assert!(client.available().await.unwrap());
    assert_eq!(
        model.requests.lock().unwrap().len(),
        1,
        "Cancelled queued tasks must never call the model"
    );
    server.close();
    server.wait_closed().await;
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn owner_limits_reserve_control_capacity_and_invalid_submissions_do_not_poison_replies() {
    let (_root, service) = setup("http://127.0.0.1:1/v1");
    let server = Server::start(service.clone()).unwrap();
    let client = server.endpoint().client(service.workspace().unwrap(), None);
    let mut owners = Vec::new();
    for _ in 0..8 {
        owners.push(client.own_jobs().await.unwrap());
    }
    assert!(client
        .own_jobs()
        .await
        .err()
        .unwrap()
        .to_string()
        .contains("eight"));
    assert!(
        client.available().await.unwrap(),
        "Ownership sockets must not consume all control capacity"
    );
    assert!(owners[0].submit(json!({"task":""})).await.is_err());
    let job = owners[0]
        .submit(json!({"task":"A valid task after a rejected request"}))
        .await
        .unwrap();
    assert!(!matches!(
        done(&service, &job["id"]).await.as_str(),
        "queued" | "running" | "cancelling"
    ));
    service
        .engine
        .delete_session(job["session_id"].as_str().unwrap())
        .unwrap();
    owners[0].close().await.unwrap();
    let replacement = client.own_jobs().await.unwrap();
    replacement.close().await.unwrap();
    for owner in owners {
        owner.close().await.unwrap();
    }
    server.close();
    server.wait_closed().await;
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn aborting_the_control_server_cancels_owned_jobs_before_engine_shutdown() {
    let model = support::server(|_, _| (json!({}), Duration::from_secs(60))).await;
    let (_root, service) = setup(&model.endpoint);
    let server = Server::start(service.clone()).unwrap();
    let client = server.endpoint().client(service.workspace().unwrap(), None);
    let owner = client.own_jobs().await.unwrap();
    let running = owner
        .submit(json!({"task":"Running owner task"}))
        .await
        .unwrap();
    let queued = owner
        .submit(json!({"task":"Queued owner task","queue":true}))
        .await
        .unwrap();
    server.close();
    server.wait_closed().await;
    assert_eq!(done(&service, &running["id"]).await, "cancelled");
    assert_eq!(done(&service, &queued["id"]).await, "cancelled");
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn unread_submission_replies_and_protocol_failures_cannot_orphan_accepted_tasks() {
    let model = support::server(|_, _| (json!({}), Duration::from_secs(60))).await;
    let (root, service) = setup(&model.endpoint);
    let server = Server::start(service.clone()).unwrap();
    let other = root.path().join("other");
    fs::create_dir(&other).unwrap();
    for fault in ["unread-reply", "malformed-frame", "other-project"] {
        let mut owner = raw_owner(&server, &service).await;
        send(&mut owner, json!({"job":{"task":fault}})).await;
        // Observe durable acceptance through the engine, deliberately leaving
        // the submission reply unread on the ownership socket.
        let job = tokio::time::timeout(Duration::from_secs(4), async {
            loop {
                if let Some(job) = service
                    .engine
                    .store()
                    .jobs(20)
                    .unwrap()
                    .into_iter()
                    .find(|j| j["task"] == fault)
                {
                    break job;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        match fault {
            "malformed-frame" => {
                owner.write_u32(1).await.unwrap();
                owner.write_all(b"{").await.unwrap();
            }
            "other-project" => {
                send(
                    &mut owner,
                    json!({"job":{"workspace":other,"task":"Must not start"}}),
                )
                .await
            }
            _ => {}
        }
        if fault != "unread-reply" {
            assert_eq!(read(&mut owner).await["result"]["id"], job["id"]);
            assert!(read(&mut owner).await["error"].is_string());
        }
        drop(owner);
        assert_eq!(done(&service, &job["id"]).await, "cancelled");
        assert!(!service
            .engine
            .store()
            .jobs(20)
            .unwrap()
            .iter()
            .any(|j| j["task"] == "Must not start"));
    }
    server.close();
    server.wait_closed().await;
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn workflow_ownership_cancels_only_its_jobs_and_rejects_non_task_commands() {
    let model = support::server(|_, _| (json!({}), Duration::from_secs(60))).await;
    let (_root, service) = setup(&model.endpoint);
    let workspace = service.workspace().unwrap();
    let skills = workspace.join(".shadowcode/skills/owned-review");
    fs::create_dir_all(&skills).unwrap();
    fs::write(skills.join("SKILL.md"),"---\nname: owned-review\ndescription: Ownership fixture\nmode: review\n---\nReview the project carefully.\n").unwrap();
    let server = Server::start_with_mode(service.clone(), "tui").unwrap();
    let client = server.endpoint().client(workspace.clone(), None);
    let owner = client.own_jobs().await.unwrap();
    assert!(owner
        .submit_workflow(json!({"name":"model","args":"unexpected-model"}))
        .await
        .unwrap_err()
        .to_string()
        .contains("only model workflows"));
    let unrelated = service
        .engine
        .start(StartRequest {
            workspace,
            task: "Unrelated work".into(),
            session_id: None,
            model: None,
            mode: "plan".into(),
            queue: false,
        
            images: Vec::new(),
        })
        .await
        .unwrap();
    let skill = owner
        .submit_workflow(json!({"name":"skill","args":"owned-review inspect","queue":true}))
        .await
        .unwrap();
    let plan = owner
        .submit_workflow(json!({"name":"plan","args":"Design a fix","queue":true}))
        .await
        .unwrap();
    assert_eq!(skill["metadata"]["job"]["status"], "queued");
    assert_eq!(skill["metadata"]["job"]["mode"], "review");
    drop(owner);
    assert_eq!(
        done(&service, &skill["metadata"]["job"]["id"]).await,
        "cancelled"
    );
    assert_eq!(
        done(&service, &plan["metadata"]["job"]["id"]).await,
        "cancelled"
    );
    assert_eq!(
        service.engine.job(&unrelated.id).unwrap().unwrap().status,
        "running"
    );
    server.close();
    server.wait_closed().await;
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn terminal_history_pages_are_ordered_exclusive_and_metadata_is_small() {
    use shadowcode_core::service::Request;
    let (_root, service) = setup("http://127.0.0.1:1/v1");
    let store = service.engine.store();
    let session = store
        .create_session(&service.workspace().unwrap(), "fixture", "History")
        .unwrap();
    let sid = session["id"].as_str().unwrap();
    for i in 0..1030 {
        store
            .add_event(
                "user.message",
                &json!({"text":format!("row {i}")}),
                Some(sid),
                None,
            )
            .unwrap();
    }
    let call = |path: String| {
        service.dispatch(Request {
            method: "GET".into(),
            path,
            body: Value::Null,
        })
    };
    let summary = call(format!("/api/sessions/{sid}?summary=true"))
        .await
        .unwrap();
    assert!(summary.get("events").is_none());
    assert_eq!(summary["title"], "History");
    let mut cursor = store.event_cursor(sid).unwrap() + 1;
    let mut seen = Vec::new();
    loop {
        let page = call(format!(
            "/api/sessions/{sid}/events?before={cursor}&limit=127"
        ))
        .await
        .unwrap();
        let rows = page["events"].as_array().unwrap();
        if rows.is_empty() {
            break;
        }
        let ids: Vec<i64> = rows.iter().map(|r| r["id"].as_i64().unwrap()).collect();
        assert!(ids.windows(2).all(|p| p[0] < p[1]));
        assert!(ids.iter().all(|id| *id < cursor));
        cursor = ids[0];
        seen.extend(ids);
    }
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), 1030);
    for bad in ["0", "-1", "junk"] {
        assert!(call(format!("/api/sessions/{sid}/events?before={bad}"))
            .await
            .is_err());
    }
    service.engine.shutdown().await.unwrap();
}
