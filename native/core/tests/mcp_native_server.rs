#![cfg(unix)]
mod support;
use rmcp::{model::*, service::RunningService, RoleClient, ServiceExt};
use serde_json::{json, Value};
use shadowcode_core::{
    config::Config,
    control,
    mcp::server::{self, Access},
    paths::AppPaths,
    service::Service,
};
use std::{fs, path::PathBuf, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;
struct Fixture {
    _root: tempfile::TempDir,
    project: PathBuf,
    other: PathBuf,
    paths: AppPaths,
    service: Service,
    control: control::Server,
}
struct Connection {
    peer: RunningService<RoleClient, ClientConfig>,
    worker: JoinHandle<anyhow::Result<()>>,
    cancel: CancellationToken,
}
impl Fixture {
    fn new(endpoint: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        let other = root.path().join("other");
        fs::create_dir(&project).unwrap();
        fs::create_dir(&other).unwrap();
        let paths = AppPaths::isolated(&root.path().join("profile")).unwrap();
        Config::patch(&paths,json!({"model":{"provider":"local","endpoint":endpoint,"name":"fixture","default":"fixture","context_limit":16384},"trusted_workspaces":[project,other],"agent":{"max_steps":4,"model_retries":0}})).unwrap();
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
    async fn connect(&self, access: Access) -> Connection {
        let (peer, server) = tokio::io::duplex(65536);
        let (reader, writer) = tokio::io::split(server);
        let cancel = CancellationToken::new();
        let worker = tokio::spawn(server::serve_io(
            self.paths.clone(),
            self.project.clone(),
            access,
            reader,
            writer,
            cancel.clone(),
        ));
        let peer = ClientConfig::new(
            Default::default(),
            Implementation::new("official-sdk-test", "1"),
        )
        .serve(peer)
        .await
        .unwrap();
        Connection {
            peer,
            worker,
            cancel,
        }
    }
    async fn close(&self) {
        self.control.close();
        self.control.wait_closed().await;
        self.service.engine.shutdown().await.unwrap();
    }
}
impl Connection {
    async fn call(&self, name: &str, args: Value) -> Value {
        match self
            .peer
            .call_tool_once(
                CallToolRequestParams::new(name.to_owned())
                    .with_arguments(args.as_object().unwrap().clone()),
            )
            .await
            .unwrap()
        {
            CallToolResponse::Complete(value) => {
                json!({"error":value.is_error.unwrap_or(false),"value":value.structured_content.unwrap()})
            }
            _ => panic!("Unexpected MCP continuation"),
        }
    }
    async fn close(mut self) {
        self.peer.close().await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), self.worker)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}
fn response(text: &str, calls: Value) -> Value {
    json!({"choices":[{"message":{"role":"assistant","content":text,"tool_calls":calls},"finish_reason":"stop"}]})
}
fn tool(name: &str, args: Value) -> Value {
    json!({"id":"call-one","type":"function","function":{"name":name,"arguments":args.to_string()}})
}
async fn finished(f: &Fixture, id: &str) -> Value {
    json!(
        tokio::time::timeout(Duration::from_secs(5), f.service.engine.wait(id))
            .await
            .unwrap()
            .unwrap()
    )
}
async fn approval(c: &Connection, id: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let state = c.call("shadow_jobs", json!({"job_id":id})).await;
            if let Some(value) = state["value"]["approvals"].as_array().unwrap().first() {
                return value.clone();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn official_sdk_lists_native_tools_resources_prompts_and_confines_project_reads() {
    let f = Fixture::new("http://127.0.0.1:1/v1");
    let store = f.service.engine.store();
    store
        .create_session(&f.project, "fixture", "visible-session")
        .unwrap();
    for _ in 0..120 {
        store
            .create_session(&f.other, "fixture", "other-project-secret")
            .unwrap();
    }
    let c = f.connect(Access::default()).await;
    let catalog = c.peer.list_tools(None).await.unwrap();
    assert_eq!(catalog.tools.len(), 17);
    assert!(
        catalog
            .tools
            .iter()
            .find(|t| t.name == "shadow_run")
            .unwrap()
            .annotations
            .as_ref()
            .unwrap()
            .read_only_hint
            == Some(false)
    );
    let sessions = c.call("shadow_sessions", json!({"limit":1})).await;
    assert_eq!(sessions["value"]["sessions"].as_array().unwrap().len(), 1);
    assert!(!sessions.to_string().contains("other-project-secret"));
    assert_eq!(
        c.call("shadow_status", json!({})).await["value"]["status"]["workspace"],
        json!(f.project)
    );
    assert_eq!(
        f.service.workspace().unwrap(),
        f.other,
        "MCP must not navigate the desktop"
    );
    assert_eq!(
        c.call("shadow_status", json!({"workspace":f.other})).await["error"],
        true
    );
    assert!(c
        .peer
        .call_tool_once(
            CallToolRequestParams::new("shadow_sessions")
                .with_arguments(json!({"limit":101}).as_object().unwrap().clone())
        )
        .await
        .is_err());
    assert!(c
        .peer
        .call_tool_once(
            CallToolRequestParams::new("shadow_status")
                .with_arguments(json!({"unknown":true}).as_object().unwrap().clone())
        )
        .await
        .is_err());
    let resources = c.peer.list_resources(None).await.unwrap();
    assert_eq!(resources.resources.len(), 4);
    let read = c
        .peer
        .read_resource(ReadResourceRequestParams::new("shadow://memory"))
        .await
        .unwrap();
    assert!(json!(read).to_string().contains("No notes saved"));
    assert!(c
        .peer
        .read_resource(ReadResourceRequestParams::new("shadow://project/etc"))
        .await
        .is_err());
    assert_eq!(c.peer.list_prompts(None).await.unwrap().prompts.len(), 2);
    let prompt = c
        .peer
        .get_prompt(
            GetPromptRequestParams::new("delegate").with_arguments(
                json!({"task":"Inspect README"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    assert!(json!(prompt).to_string().contains("Inspect README"));
    assert!(c
        .peer
        .get_prompt(GetPromptRequestParams::new("missing"))
        .await
        .is_err());
    c.close().await;
    f.close().await;
}

#[tokio::test]
async fn official_sdk_queries_project_sqlite_with_read_only_server_access() {
    let f = Fixture::new("http://127.0.0.1:1/v1");
    let db = rusqlite::Connection::open(f.project.join("data.db")).unwrap();
    db.execute_batch("CREATE TABLE items(value TEXT); INSERT INTO items VALUES('sqlite native');")
        .unwrap();
    drop(db);
    let c = f.connect(Access::default()).await;
    let tables = c.call("shadow_sqlite", json!({"path":"data.db"})).await;
    assert_eq!(tables["error"], false, "{tables}");
    assert_eq!(tables["value"]["tables"], json!(["items"]));
    let query = c.call("shadow_sqlite",json!({"path":"data.db","sql":"SELECT value FROM items WHERE value=?","params":["sqlite native"],"limit":1})).await;
    assert_eq!(query["value"]["rows"][0]["value"], "sqlite native");
    for args in [
        json!({"path":"data.db","sql":"DROP TABLE items"}),
        json!({"path":"data.db","workspace":f.other}),
        json!({"path":"../other/data.db"}),
    ] {
        assert_eq!(c.call("shadow_sqlite", args).await["error"], true);
    }
    assert_eq!(f.service.workspace().unwrap(), f.other);
    c.close().await;
    f.close().await;
}

#[tokio::test]
async fn native_server_write_grants_never_override_trust_or_project_permissions() {
    let f = Fixture::new("http://127.0.0.1:1/v1");
    let c = f.connect(Access::default()).await;
    for (name, args) in [
        (
            "shadow_memory",
            json!({"action":"append","note":"not allowed"}),
        ),
        (
            "shadow_goal",
            json!({"action":"create","instruction":"not allowed"}),
        ),
        (
            "shadow_run",
            json!({"task":"not allowed","permission_level":"elevated"}),
        ),
    ] {
        assert_eq!(c.call(name, args).await["error"], true);
    }
    c.close().await;
    let c = f
        .connect(Access {
            allow_write: true,
            allow_approvals: false,
        })
        .await;
    assert_eq!(
        c.call(
            "shadow_memory",
            json!({"action":"append","note":"native note 雪"})
        )
        .await["error"],
        false
    );
    assert!(
        fs::read_to_string(f.project.join(".shadow/memory/project.md"))
            .unwrap()
            .contains("native note 雪")
    );
    let goal = c
        .call(
            "shadow_goal",
            json!({"action":"create","instruction":"Verify the feature"}),
        )
        .await;
    assert_eq!(goal["error"], false, "{goal}");
    let gid = goal["value"]["id"].as_str().unwrap();
    assert_eq!(
        c.call(
            "shadow_goal",
            json!({"action":"advance","goal_id":gid,"status":"done","detail":"verified by fixture"})
        )
        .await["error"],
        false
    );
    assert_eq!(
        c.call("shadow_goal", json!({"action":"abandon","goal_id":gid}))
            .await["error"],
        false
    );
    let outside = store_goal(&f, &f.other);
    assert_eq!(
        c.call("shadow_goal", json!({"action":"get","goal_id":outside}))
            .await["error"],
        true
    );
    Config::patch(&f.paths, json!({"permissions":{"level":"read_only"}})).unwrap();
    assert_eq!(
        c.call(
            "shadow_memory",
            json!({"action":"append","note":"readonly"})
        )
        .await["error"],
        true
    );
    Config::patch(
        &f.paths,
        json!({"permissions":{"level":"workspace"},"trusted_workspaces":[]}),
    )
    .unwrap();
    assert_eq!(
        c.call(
            "shadow_memory",
            json!({"action":"append","note":"untrusted"})
        )
        .await["error"],
        true
    );
    c.close().await;
    f.close().await;
}
fn store_goal(f: &Fixture, path: &std::path::Path) -> String {
    f.service
        .engine
        .store()
        .create_goal(
            path,
            "other",
            &shadowcode_core::store::MilestoneSpec::default_plan(),
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .into()
}

#[tokio::test]
async fn native_mcp_task_permission_ceiling_prevents_writes_and_preserves_configuration() {
    let provider = support::server(|_, payload| {
        assert!(payload["tools"]
            .as_array()
            .unwrap()
            .iter()
            .all(|t| t["function"]["name"] != "exec"));
        (
            response(
                "Attempt write",
                json!([tool(
                    "write_file",
                    json!({"path":"forbidden.txt","content":"bad","expected_hash":"missing"})
                )]),
            ),
            Duration::ZERO,
        )
    })
    .await;
    let f = Fixture::new(&provider.endpoint);
    let c = f.connect(Access::default()).await;
    let result = c
        .call("shadow_run", json!({"task":"Inspect the project"}))
        .await;
    assert_eq!(result["error"], false, "{result}");
    let id = result["value"]["job"]["id"].as_str().unwrap();
    let done = finished(&f, id).await;
    assert_eq!(done["mode"], "plan");
    assert!(!provider.requests.lock().unwrap().is_empty());
    assert!(!f.project.join("forbidden.txt").exists());
    assert_eq!(
        Config::load(&f.paths, Some(&f.project))
            .unwrap()
            .permissions
            .level,
        shadowcode_core::config::PermissionLevel::Workspace
    );
    let observer = f
        .connect(Access {
            allow_write: true,
            allow_approvals: true,
        })
        .await;
    assert_eq!(
        observer
            .call("shadow_jobs", json!({"job_id":id,"cancel":true}))
            .await["error"],
        true
    );
    observer.close().await;
    c.close().await;
    f.close().await;
}

#[tokio::test]
async fn native_mcp_approvals_are_explicit_and_owned_and_disconnect_cleans_up() {
    for grant in [false, true] {
        let provider = support::server(|index, _| {
            (
                if index == 0 {
                    response(
                        "Run a check",
                        json!([tool(
                            "exec",
                            json!({"command":"printf server-approved > result.txt"})
                        )]),
                    )
                } else {
                    response("Verified", json!([]))
                },
                Duration::ZERO,
            )
        })
        .await;
        let f = Fixture::new(&provider.endpoint);
        let c = f
            .connect(Access {
                allow_write: true,
                allow_approvals: grant,
            })
            .await;
        let result = c
            .call(
                "shadow_run",
                json!({"task":"Run one check","permission_level":"workspace"}),
            )
            .await;
        assert_eq!(result["error"], false, "{result}");
        let id = result["value"]["job"]["id"].as_str().unwrap();
        let pending = approval(&c, id).await;
        assert!(pending["command"].as_str().unwrap().contains("result.txt"));
        let observer = f
            .connect(Access {
                allow_write: true,
                allow_approvals: true,
            })
            .await;
        assert_eq!(
            observer
                .call(
                    "shadow_approve",
                    json!({"approval_id":pending["id"],"decision":"approve"})
                )
                .await["error"],
            true
        );
        observer.close().await;
        let approved = c
            .call(
                "shadow_approve",
                json!({"approval_id":pending["id"],"decision":"approve"}),
            )
            .await;
        assert_eq!(approved["error"], !grant, "{approved}");
        if grant {
            let _ = finished(&f, id).await;
            assert_eq!(
                fs::read_to_string(f.project.join("result.txt")).unwrap(),
                "server-approved"
            );
        }
        let id = id.to_owned();
        c.close().await;
        assert!(!active_status(&json!(f
            .service
            .engine
            .job(&id)
            .unwrap()
            .unwrap())));
        assert!(f.service.engine.approvals().list(None).is_empty());
        assert!(f.service.engine.store().sessions("", 5).is_ok());
        f.close().await;
    }
}
fn active_status(job: &Value) -> bool {
    matches!(
        job["status"].as_str(),
        Some("queued" | "running" | "cancelling")
    )
}

#[tokio::test]
async fn native_server_task_notes_use_exact_project_tasks_and_write_grants() {
    let f = Fixture::new("http://127.0.0.1:1/v1");
    let store = f.service.engine.store();
    let session = store.create_session(&f.project, "fixture", "").unwrap();
    let tid = store
        .create_task(session["id"].as_str().unwrap(), "Earlier task")
        .unwrap();
    let outside = store.create_session(&f.other, "fixture", "").unwrap();
    let other_tid = store
        .create_task(outside["id"].as_str().unwrap(), "Other project")
        .unwrap();
    let readonly = f.connect(Access::default()).await;
    let writer = f
        .connect(Access {
            allow_write: true,
            allow_approvals: false,
        })
        .await;
    for action in ["append", "replace"] {
        assert_eq!(
            readonly
                .call(
                    "shadow_memory",
                    json!({"action":action,"scope":"task","task_id":tid,"note":"not allowed"})
                )
                .await["error"],
            true
        );
    }
    assert_eq!(
        writer
            .call(
                "shadow_memory",
                json!({"action":"read","scope":"task","task_id":other_tid})
            )
            .await["error"],
        true
    );
    assert_eq!(
        writer
            .call(
                "shadow_memory",
                json!({"action":"append","scope":"task","note":"missing ID"})
            )
            .await["error"],
        true
    );
    let saved=writer.call("shadow_memory",json!({"action":"append","scope":"task","task_id":tid,"note":"Keep the offline fixture."})).await;
    assert_eq!(saved["error"], false, "{saved}");
    let read = readonly
        .call("shadow_memory", json!({"action":"read","task_id":tid}))
        .await;
    assert_eq!(read["value"]["task"], "- Keep the offline fixture.\n");
    assert_eq!(writer.call("shadow_memory",json!({"action":"replace","scope":"task","task_id":tid,"note":"New note","expected_hash":"stale"})).await["error"],true);
    let edited=writer.call("shadow_memory",json!({"action":"replace","scope":"task","task_id":tid,"note":"New note","expected_hash":read["value"]["task_hash"]})).await;
    assert_eq!(edited["value"]["task"], "New note");
    writer.close().await;
    readonly.close().await;
    f.close().await;
}

#[tokio::test]
async fn native_mcp_inspection_and_test_jobs_use_real_permissions_without_a_model() {
    let f = Fixture::new("http://127.0.0.1:1/v1");
    fs::write(
        f.project.join("Cargo.toml"),
        "[package]\nname='fixture'\nversion='0.1.0'\n",
    )
    .unwrap();
    let readonly = f.connect(Access::default()).await;
    let map = readonly.call("shadow_understand", json!({})).await;
    assert_eq!(map["error"], false);
    assert_eq!(map["value"]["saved"], false);
    assert_eq!(
        readonly
            .call("shadow_understand", json!({"save":true}))
            .await["error"],
        true
    );
    assert_eq!(
        readonly
            .call("shadow_test", json!({"command":"touch forbidden"}))
            .await["error"],
        true
    );
    assert!(!f.project.join("forbidden").exists());
    let doctor = readonly.call("shadow_doctor", json!({})).await;
    assert_eq!(doctor["error"], false);
    assert_eq!(doctor["value"]["report"]["runtime"], "rust");
    assert!(json!(readonly
        .peer
        .read_resource(ReadResourceRequestParams::new("shadow://project"))
        .await
        .unwrap())
    .to_string()
    .contains("Cargo.toml"));
    assert!(json!(readonly
        .peer
        .get_prompt(GetPromptRequestParams::new("understand"))
        .await
        .unwrap())
    .to_string()
    .contains("shadow_understand"));
    let writer = f
        .connect(Access {
            allow_write: true,
            allow_approvals: true,
        })
        .await;
    assert_eq!(
        writer.call("shadow_understand", json!({"save":true})).await["value"]["saved"],
        true
    );
    let auto = writer.call("shadow_test", json!({})).await;
    assert_eq!(auto["error"], false, "{auto}");
    let id = auto["value"]["job"]["id"].as_str().unwrap();
    let pending = approval(&writer, id).await;
    assert_eq!(pending["command"], "cargo test");
    writer
        .call(
            "shadow_approve",
            json!({"approval_id":pending["id"],"decision":"deny"}),
        )
        .await;
    assert_eq!(finished(&f, id).await["status"], "failed");
    assert!(!f.project.join("target").exists());
    let started = writer
        .call("shadow_test", json!({"command":"printf mcp-native-test"}))
        .await;
    let id = started["value"]["job"]["id"].as_str().unwrap();
    let pending = approval(&writer, id).await;
    assert_eq!(
        readonly
            .call(
                "shadow_approve",
                json!({"approval_id":pending["id"],"decision":"approve"})
            )
            .await["error"],
        true
    );
    writer
        .call(
            "shadow_approve",
            json!({"approval_id":pending["id"],"decision":"approve"}),
        )
        .await;
    let result = finished(&f, id).await;
    assert_eq!(result["status"], "completed");
    assert_eq!(result["result"]["command"]["stdout"], "mcp-native-test");
    assert_eq!(result["usage"]["total_tokens"], 0);
    writer.close().await;
    readonly.close().await;
    f.close().await;
}

#[tokio::test]
async fn malformed_oversized_flooded_or_abandoned_mcp_initialization_is_bounded() {
    for bytes in [
        b"private-not-json\n".to_vec(),
        vec![b'x'; 1_048_577],
        vec![b'\n'; 129],
        b"{\"partial\":".to_vec(),
    ] {
        let f = Fixture::new("http://127.0.0.1:1/v1");
        let (mut peer, server) = tokio::io::duplex(2_000_000);
        let (r, w) = tokio::io::split(server);
        let worker = tokio::spawn(server::serve_io(
            f.paths.clone(),
            f.project.clone(),
            Access::default(),
            r,
            w,
            CancellationToken::new(),
        ));
        peer.write_all(&bytes).await.unwrap();
        peer.shutdown().await.unwrap();
        let result = tokio::time::timeout(Duration::from_secs(3), worker)
            .await
            .unwrap()
            .unwrap();
        assert!(result.is_err());
        assert!(!format!("{:?}", result).contains("private-not-json"));
        let mut response = String::new();
        peer.read_to_string(&mut response).await.unwrap();
        assert!(response.is_empty());
        f.close().await;
    }
    let f = Fixture::new("http://127.0.0.1:1/v1");
    let c = f.connect(Access::default()).await;
    c.cancel.cancel();
    c.peer.waiting().await.unwrap();
    c.worker.await.unwrap().unwrap();
    f.close().await;
}

#[tokio::test]
async fn abandoned_native_mcp_server_cancels_only_its_owned_jobs() {
    let provider =
        support::server(|_, _| (response("later", json!([])), Duration::from_secs(60))).await;
    let f = Fixture::new(&provider.endpoint);
    let unrelated = f
        .service
        .engine
        .start(shadowcode_core::engine::StartRequest {
            workspace: f.other.clone(),
            task: "Keep this unrelated task".into(),
            session_id: None,
            model: None,
            mode: "plan".into(),
            queue: false,
        })
        .await
        .unwrap();
    let c = f.connect(Access::default()).await;
    let result = c
        .call("shadow_run", json!({"task":"This belongs to MCP"}))
        .await;
    let id = result["value"]["job"]["id"].as_str().unwrap().to_owned();
    c.worker.abort();
    assert!(c.worker.await.is_err());
    let done = finished(&f, &id).await;
    assert_eq!(done["status"], "cancelled");
    assert_eq!(
        f.service.engine.job(&unrelated.id).unwrap().unwrap().status,
        "running"
    );
    drop(c.peer);
    f.close().await;
}
