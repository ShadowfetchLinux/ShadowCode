mod support;
use serde_json::{json, Value};
use shadowcode_core::{
    config::{Config, PermissionLevel},
    engine::{Job, StartRequest},
    events::TaskEvents,
    hooks,
    models::ToolCall,
    paths::AppPaths,
    service::{Request, Service},
    tools::ToolExecutor,
    workspace::Workspace,
};
use std::{fs, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

fn fixture(endpoint: &str) -> (tempfile::TempDir, Service) {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    fs::create_dir(&project).unwrap();
    let paths = AppPaths::isolated(&root.path().join("profile")).unwrap();
    Config::patch(&paths,json!({"model":{"provider":"local","name":"fixture","endpoint":endpoint,"context_limit":16384},"trusted_workspaces":[project],"permissions":{"approve_shell":false},"agent":{"max_steps":12,"model_retries":0}})).unwrap();
    (root, Service::open(paths, Some(project)).unwrap())
}
fn put(service: &Service, path: &str, text: &str) {
    let file = service.workspace().unwrap().join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, text).unwrap();
}
async fn api(service: &Service, method: &str, path: &str, body: Value) -> anyhow::Result<Value> {
    service
        .dispatch(Request {
            method: method.into(),
            path: path.into(),
            body,
        })
        .await
}
async fn enable(
    service: &Service,
    name: &str,
    events: &[&str],
    command: &str,
    extra: Value,
) -> String {
    let path = format!(".shadowcode/hooks/{name}.yaml");
    let mut definition = json!({"name":name,"events":events,"command":command,"timeout_sec":3});
    for (key, value) in extra.as_object().unwrap() {
        definition[key] = value.clone();
    }
    put(
        service,
        &path,
        &serde_yaml_ng::to_string(&definition).unwrap(),
    );
    let workspace = Workspace::open(&service.workspace().unwrap()).unwrap();
    let hash = workspace.read(&path).unwrap().hash;
    api(
        service,
        "POST",
        "/api/hooks/activation",
        json!({"workspace":workspace.path,"path":path,"hash":hash,"enabled":true}),
    )
    .await
    .unwrap();
    path
}
fn tools(service: &Service, config: Option<Config>) -> ToolExecutor {
    let workspace = Arc::new(Workspace::open(&service.workspace().unwrap()).unwrap());
    let store = service.engine.store();
    let sid = store
        .create_session(&workspace.path, "fixture", "")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let task = store.create_task(&sid, "test").unwrap();
    let (sender, _) = tokio::sync::broadcast::channel(100);
    ToolExecutor::new(
        workspace,
        config.unwrap_or_else(|| Config::load(service.engine.paths(), None).unwrap()),
        service.engine.approvals(),
        TaskEvents {
            store,
            session_id: sid,
            task_id: task,
            sender,
        },
        CancellationToken::new(),
    )
    .unwrap()
}
async fn call(
    tools: &ToolExecutor,
    name: &str,
    arguments: Value,
) -> shadowcode_core::tools::ToolResult {
    tools
        .execute(ToolCall {
            id: shadowcode_core::id(),
            name: name.into(),
            arguments,
        })
        .await
        .unwrap()
}
fn request(service: &Service, mode: &str, queue: bool) -> StartRequest {
    StartRequest {
        workspace: service.workspace().unwrap(),
        task: "Carry out this fixture task".into(),
        session_id: None,
        model: None,
        mode: mode.into(),
        queue,
        images: Vec::new(),
        web: false,
    }
}
async fn wait(service: &Service, job: &Job) -> Job {
    tokio::time::timeout(Duration::from_secs(8), service.engine.wait(&job.id))
        .await
        .unwrap()
        .unwrap()
}
fn response(text: &str, calls: Value) -> Value {
    json!({"choices":[{"message":{"role":"assistant","content":text,"tool_calls":calls},"finish_reason":if calls.as_array().unwrap().is_empty(){"stop"}else{"tool_calls"}}]})
}
fn tool(name: &str, args: Value) -> Value {
    json!({"id":shadowcode_core::id(),"type":"function","function":{"name":name,"arguments":args.to_string()}})
}

#[tokio::test]
async fn discovery_is_inert_confined_and_activation_requires_current_content_and_project() {
    let (_root, service) = fixture("http://127.0.0.1:1/v1");
    put(
        &service,
        ".shadowcode/hooks/legacy.py",
        "open('unexpected','w').write('executed')",
    );
    put(
        &service,
        ".shadowcode/hooks/invalid.yaml",
        "name: invalid\nevents: [on_complete]\ncommand: true\nunknown: true\n",
    );
    put(
        &service,
        ".shadowcode/hooks/check.yaml",
        "name: check\nevents: [on_complete]\ncommand: 'touch unexpected'\n",
    );
    let catalog = api(&service, "GET", "/api/hooks", Value::Null)
        .await
        .unwrap();
    assert_eq!(catalog["hooks"].as_array().unwrap().len(), 1);
    assert_eq!(catalog["issues"].as_array().unwrap().len(), 2);
    assert!(!service.workspace().unwrap().join("unexpected").exists());
    assert_eq!(catalog["hooks"][0]["enabled"], false);
    let mut body = json!({"workspace":service.workspace().unwrap(),"path":".shadowcode/hooks/check.yaml","hash":catalog["hooks"][0]["hash"],"enabled":true});
    body["workspace"] = json!("/different-project");
    assert!(api(&service, "POST", "/api/hooks/activation", body.clone())
        .await
        .is_err());
    body["workspace"] = json!(service.workspace().unwrap());
    body["hash"] = json!("0".repeat(64));
    assert!(api(&service, "POST", "/api/hooks/activation", body.clone())
        .await
        .is_err());
    body["hash"] = catalog["hooks"][0]["hash"].clone();
    Config::patch(service.engine.paths(), json!({"trusted_workspaces":[]})).unwrap();
    assert!(api(&service, "POST", "/api/hooks/activation", body.clone())
        .await
        .is_err());
    Config::patch(
        service.engine.paths(),
        json!({"trusted_workspaces":[service.workspace().unwrap()]}),
    )
    .unwrap();
    api(&service, "POST", "/api/hooks/activation", body)
        .await
        .unwrap();
    assert!(!service.workspace().unwrap().join("unexpected").exists());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            "/etc/passwd",
            service
                .workspace()
                .unwrap()
                .join(".shadowcode/hooks/escape.yaml"),
        )
        .unwrap();
        let data = api(&service, "GET", "/api/hooks", Value::Null)
            .await
            .unwrap();
        assert_eq!(data["hooks"].as_array().unwrap().len(), 1);
    }
    put(
        &service,
        ".shadow/config/config.yaml",
        "hooks:\n  approved: []\n",
    );
    assert_eq!(
        Config::load(service.engine.paths(), Some(&service.workspace().unwrap()))
            .unwrap()
            .hooks
            .approved
            .len(),
        1
    );
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn edits_pass_literal_paths_run_once_per_patch_file_and_invalidate_stale_observations() {
    let (_root, service) = fixture("http://127.0.0.1:1/v1");
    enable(&service,"format",&["after_edit"],"printf '|formatted' >> \"$SHADOW_HOOK_PATH\"; printf '%s\\n' \"$SHADOW_HOOK_PATH\" >> hooks.log",json!({"path_suffix":".txt"})).await;
    let tools = tools(&service, None);
    let name = "$(touch unexpected).txt";
    let written = call(
        &tools,
        "write_file",
        json!({"path":name,"content":"literal","expected_hash":"missing"}),
    )
    .await;
    assert!(written.success, "{}", written.error);
    assert_eq!(
        fs::read_to_string(service.workspace().unwrap().join(name)).unwrap(),
        "literal|formatted"
    );
    assert!(!service.workspace().unwrap().join("unexpected").exists());
    let patch="*** Begin Patch\n*** Add File: a.txt\n+first\n*** Add File: b.txt\n+second\n*** Add File: skip.md\n+untouched\n*** End Patch";
    assert!(
        call(&tools, "apply_patch", json!({"patch":patch}))
            .await
            .success
    );
    assert_eq!(
        fs::read_to_string(service.workspace().unwrap().join("a.txt")).unwrap(),
        "first\n|formatted"
    );
    assert_eq!(
        fs::read_to_string(service.workspace().unwrap().join("b.txt")).unwrap(),
        "second\n|formatted"
    );
    assert_eq!(
        fs::read_to_string(service.workspace().unwrap().join("skip.md")).unwrap(),
        "untouched\n"
    );
    assert_eq!(
        fs::read_to_string(service.workspace().unwrap().join("hooks.log"))
            .unwrap()
            .lines()
            .count(),
        3
    );
    assert!(
        !call(
            &tools,
            "write_file",
            json!({"path":"a.txt","content":"blind replacement"})
        )
        .await
        .success,
        "Hook side effects require a fresh read"
    );
    assert!(
        shadowcode_core::checkpoint::restore(
            &tools.events.store,
            &tools.workspace,
            &tools.events.task_id
        )
        .is_err(),
        "Rewind must not overwrite shell-hook changes"
    );
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn failing_gates_block_commands_and_commits_before_the_side_effect() {
    let (_root, service) = fixture("http://127.0.0.1:1/v1");
    enable(
        &service,
        "a-gate",
        &["before_command", "before_commit"],
        "printf gate-failed >&2; exit 2",
        json!({}),
    )
    .await;
    enable(
        &service,
        "z-later",
        &["before_command", "before_commit"],
        "touch later",
        json!({}),
    )
    .await;
    let tools = tools(&service, None);
    let blocked = call(&tools, "exec", json!({"command":"touch unexpected"})).await;
    assert!(!blocked.success);
    assert!(blocked.error.contains("gate-failed"));
    assert!(!service.workspace().unwrap().join("unexpected").exists());
    assert!(!service.workspace().unwrap().join("later").exists());
    let worker = tools.clone();
    let task = tokio::spawn(async move {
        call(
            &worker,
            "git_commit",
            json!({"message":"must never execute"}),
        )
        .await
    });
    let pending = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Some(a) = service.engine.approvals().list(None).first() {
                break a.clone();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    service
        .engine
        .approvals()
        .decide(&pending.id, &pending.session_id, true)
        .unwrap();
    let blocked = task.await.unwrap();
    assert!(!blocked.success);
    assert!(
        blocked.error.contains("gate-failed"),
        "Git must be blocked by the gate even outside a repository"
    );
    assert!(!service.workspace().unwrap().join("later").exists());
    let oversized = call(&tools, "exec", json!({"command":"#".repeat(64000)})).await;
    assert!(!oversized.success);
    assert!(
        oversized.error.contains("Hook context exceeds"),
        "Never pass an incomplete command to a gate"
    );
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn changed_definitions_fail_closed_but_read_only_and_untrusted_modes_stay_inert() {
    let (_root, service) = fixture("http://127.0.0.1:1/v1");
    let path = enable(
        &service,
        "check",
        &["before_command", "on_complete"],
        "touch hook-ran",
        json!({}),
    )
    .await;
    let tools = tools(&service, None);
    put(
        &service,
        &path,
        "name: check\nevents: [before_command]\ncommand: 'touch replaced-hook'\n",
    );
    let blocked = call(&tools, "exec", json!({"command":"touch tool-ran"})).await;
    assert!(!blocked.success);
    assert!(blocked.error.contains("changed"));
    let mut config = Config::load(service.engine.paths(), None).unwrap();
    config.permissions.level = PermissionLevel::ReadOnly;
    assert!(hooks::Runner::load(&tools.workspace, &config)
        .unwrap()
        .is_empty());
    config.permissions.level = PermissionLevel::Workspace;
    config.trusted_workspaces.clear();
    assert!(hooks::Runner::load(&tools.workspace, &config)
        .unwrap()
        .is_empty());
    for name in ["hook-ran", "replaced-hook", "tool-ran"] {
        assert!(!service.workspace().unwrap().join(name).exists());
    }
    let catalog = api(&service, "GET", "/api/hooks", Value::Null)
        .await
        .unwrap();
    assert_eq!(catalog["hooks"][0]["enabled"], false);
    assert_eq!(catalog["approved"].as_array().unwrap().len(), 1);
    api(
        &service,
        "POST",
        "/api/hooks/activation",
        json!({"workspace":service.workspace().unwrap(),"path":path,"enabled":false}),
    )
    .await
    .unwrap();
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn completion_checks_feed_real_failures_back_for_bounded_model_repair() {
    let server = support::server(|index, body| {
        let response = match index {
            0 => response("I think it is ready", json!([])),
            1 => {
                assert!(
                    body.to_string().contains("Completion check failed")
                        || body.to_string().contains("completion check failed")
                );
                assert!(body.to_string().contains("check-required"));
                response(
                    "Repairing the missing result",
                    json!([tool(
                        "write_file",
                        json!({"path":"result.txt","content":"ready","expected_hash":"missing"})
                    )]),
                )
            }
            _ => response("The completion check can now pass", json!([])),
        };
        (response, Duration::ZERO)
    })
    .await;
    let (_root, service) = fixture(&server.endpoint);
    enable(
        &service,
        "check",
        &["on_complete"],
        "test -f result.txt || { printf check-required >&2; exit 2; }",
        json!({}),
    )
    .await;
    let job = service
        .engine
        .start(request(&service, "code", false))
        .await
        .unwrap();
    let done = wait(&service, &job).await;
    assert_eq!(done.status, "completed", "{}", done.summary);
    assert_eq!(server.requests.lock().unwrap().len(), 3);
    assert_eq!(
        fs::read_to_string(service.workspace().unwrap().join("result.txt")).unwrap(),
        "ready"
    );
    let last = service
        .engine
        .store()
        .last_task_event(&job.task_id, "hook.completed")
        .unwrap()
        .unwrap();
    assert_eq!(last["payload"]["status"], "passed");
    assert!(last["payload"]["process"]["pid"].as_u64().unwrap() > 1);
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn failed_completion_does_not_become_success_after_retry_limit() {
    let server = support::server(|_, _| (response("Done", json!([])), Duration::ZERO)).await;
    let (_root, service) = fixture(&server.endpoint);
    Config::patch(
        service.engine.paths(),
        json!({"agent":{"max_fix_retries":1}}),
    )
    .unwrap();
    enable(&service, "never", &["on_complete"], "exit 9", json!({})).await;
    let job = service
        .engine
        .start(request(&service, "code", false))
        .await
        .unwrap();
    let done = wait(&service, &job).await;
    assert_eq!(done.status, "failed");
    assert!(done.summary.contains("Completion lifecycle command failed"));
    assert_eq!(server.requests.lock().unwrap().len(), 2);
    let readonly = service
        .engine
        .start(request(&service, "plan", false))
        .await
        .unwrap();
    assert_eq!(wait(&service, &readonly).await.status, "completed");
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn noisy_completion_checks_leave_room_for_the_model_to_repair() {
    let server = support::server(|index, body| {
        let value = match index {
            0 => response("ready", json!([])),
            1 => {
                assert!(
                    body.to_string().len() < 30000,
                    "Hook output must leave room for repair"
                );
                assert!(body
                    .to_string()
                    .contains("configured completion check failed"));
                response(
                    "Repairing",
                    json!([tool(
                        "write_file",
                        json!({"path":"repaired", "content":"yes", "expected_hash":"missing"})
                    )]),
                )
            }
            _ => response("checked", json!([])),
        };
        (value, Duration::ZERO)
    })
    .await;
    let (_root, service) = fixture(&server.endpoint);
    for name in ["one", "two", "three", "four"] {
        enable(
            &service,
            name,
            &["on_complete"],
            "head -c 12000 /dev/zero | tr '\\0' x; test -f repaired",
            json!({}),
        )
        .await;
    }
    let job = service
        .engine
        .start(request(&service, "code", false))
        .await
        .unwrap();
    let done = wait(&service, &job).await;
    assert_eq!(done.status, "completed", "{}", done.summary);
    assert_eq!(server.requests.lock().unwrap().len(), 3);
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn hook_timeout_output_flood_and_cancellation_stop_owned_processes() {
    let (_root, service) = fixture("http://127.0.0.1:1/v1");
    enable(
        &service,
        "flood",
        &["on_complete"],
        "yes flood",
        json!({"timeout_sec":1}),
    )
    .await;
    let tools = tools(&service, None);
    let result = tools
        .fire_hooks(hooks::context(
            "on_complete",
            "",
            &Value::Null,
            &Value::Null,
            "",
        ))
        .await
        .unwrap();
    let process = result[0].process.as_ref().unwrap();
    assert!(process.timed_out && process.truncated && !process.ok);
    assert!(process.stdout.len() + process.stderr.len() <= 16000);
    enable(
        &service,
        "flood",
        &["on_complete"],
        "trap '' TERM; sleep 60 & echo $! > child.pid; echo $$ > parent.pid; wait",
        json!({"timeout_sec":120}),
    )
    .await;
    let cancelling = self::tools(&service, None);
    let worker = cancelling.clone();
    let future = tokio::spawn(async move {
        worker
            .fire_hooks(hooks::context(
                "on_complete",
                "",
                &Value::Null,
                &Value::Null,
                "",
            ))
            .await
            .unwrap()
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        while !service.workspace().unwrap().join("child.pid").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let parent = fs::read_to_string(service.workspace().unwrap().join("parent.pid")).unwrap();
    let child = fs::read_to_string(service.workspace().unwrap().join("child.pid")).unwrap();
    for pid in [&parent, &child] {
        assert!(pid.trim().parse::<u32>().unwrap() > 1);
        assert!(PathCheck::alive(pid));
    }
    cancelling.cancel.cancel();
    let result = tokio::time::timeout(Duration::from_secs(4), future)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result[0].status, "cancelled");
    for pid in [&parent, &child] {
        assert!(!PathCheck::alive(pid));
    }
    service.engine.shutdown().await.unwrap();
}
struct PathCheck;
impl PathCheck {
    fn alive(pid: &str) -> bool {
        fs::read_to_string(format!("/proc/{}/stat", pid.trim()))
            .is_ok_and(|s| !s.contains(") Z ") && !s.contains(") X "))
    }
}

#[tokio::test]
async fn engine_shutdown_cancels_completion_hooks_before_releasing_the_job() {
    let server = support::server(|_, _| (response("ready", json!([])), Duration::ZERO)).await;
    let (_root, service) = fixture(&server.endpoint);
    enable(
        &service,
        "shutdown",
        &["on_complete"],
        "trap '' TERM; sleep 60 & echo $! > child.pid; echo $$ > parent.pid; wait",
        json!({"timeout_sec":120}),
    )
    .await;
    let job = service
        .engine
        .start(request(&service, "code", false))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while !service.workspace().unwrap().join("parent.pid").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let parent = fs::read_to_string(service.workspace().unwrap().join("parent.pid")).unwrap();
    let child = fs::read_to_string(service.workspace().unwrap().join("child.pid")).unwrap();
    for pid in [&parent, &child] {
        assert!(pid.trim().parse::<u32>().unwrap() > 1);
        assert!(PathCheck::alive(pid));
    }
    tokio::time::timeout(Duration::from_secs(4), service.engine.shutdown())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(wait(&service, &job).await.status, "cancelled");
    for pid in [&parent, &child] {
        assert!(!PathCheck::alive(pid));
    }
    let events = service
        .engine
        .store()
        .events_after(&job.session_id, 0, None, 1000)
        .unwrap();
    assert!(events.iter().any(
        |event| event["type"] == "hook.completed" && event["payload"]["status"] == "cancelled"
    ));
    assert!(!events
        .iter()
        .any(|event| event["type"] == "verification.summary"));
}

#[tokio::test]
async fn queued_tasks_never_execute_a_replaced_hook_definition() {
    let server =
        support::server(|_, _| (response("ready", json!([])), Duration::from_secs(1))).await;
    let (_root, service) = fixture(&server.endpoint);
    let path = enable(
        &service,
        "check",
        &["on_complete"],
        "touch original",
        json!({}),
    )
    .await;
    let first = service
        .engine
        .start(request(&service, "code", false))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while server.requests.lock().unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let queued = service
        .engine
        .start(request(&service, "code", true))
        .await
        .unwrap();
    put(
        &service,
        &path,
        "name: check\nevents: [on_complete]\ncommand: 'touch replaced'\n",
    );
    service.engine.cancel(&first.id).await.unwrap();
    let done = wait(&service, &queued).await;
    assert_eq!(done.status, "failed");
    assert!(done.summary.contains("changed"));
    assert_eq!(server.requests.lock().unwrap().len(), 1);
    assert!(!service.workspace().unwrap().join("replaced").exists());
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn network_and_root_restrictions_still_apply_to_enabled_commands() {
    let (_root, service) = fixture("http://127.0.0.1:1/v1");
    Config::patch(
        service.engine.paths(),
        json!({"permissions":{"network":true}}),
    )
    .unwrap();
    enable(
        &service,
        "network",
        &["on_complete"],
        "curl http://127.0.0.1:1",
        json!({}),
    )
    .await;
    Config::patch(
        service.engine.paths(),
        json!({"permissions":{"network":false}}),
    )
    .unwrap();
    let tools = tools(&service, None);
    let outcomes = tools
        .fire_hooks(hooks::context(
            "on_complete",
            "",
            &Value::Null,
            &Value::Null,
            "",
        ))
        .await
        .unwrap();
    assert!(!outcomes[0].success);
    assert!(outcomes[0].process.is_none());
    assert!(outcomes[0].detail.contains("network"));
    put(
        &service,
        ".shadowcode/hooks/root.yaml",
        "name: root\nevents: [on_complete]\ncommand: sudo true\n",
    );
    let workspace = Workspace::open(&service.workspace().unwrap()).unwrap();
    let hash = workspace.read(".shadowcode/hooks/root.yaml").unwrap().hash;
    let error=api(&service,"POST","/api/hooks/activation",json!({"workspace":workspace.path,"path":".shadowcode/hooks/root.yaml","hash":hash,"enabled":true})).await.unwrap_err();
    assert!(error.to_string().contains("privileged"));
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_loop_fires_compaction_and_provider_error_hooks() {
    let server = support::server(|index, _| {
        (
            if index < 2 {
                response(
                    &(0..360)
                        .map(|n| format!("Historical observation {n}: inspected module {n}.\n"))
                        .collect::<String>(),
                    json!([]),
                )
            } else {
                json!({"error":{"message":"fixture failure"}})
            },
            Duration::ZERO,
        )
    })
    .await;
    let (_root, service) = fixture(&server.endpoint);
    enable(
        &service,
        "record",
        &["on_compaction", "on_error"],
        "printf '%s\\n' \"$SHADOW_HOOK_EVENT\" >> lifecycle.log",
        json!({}),
    )
    .await;
    let first = service
        .engine
        .start(request(&service, "code", false))
        .await
        .unwrap();
    let first_result = wait(&service, &first).await;
    assert_eq!(first_result.status, "completed", "{first_result:?}");
    let mut continuation = request(&service, "code", false);
    continuation.session_id = Some(first.session_id.clone());
    let middle = service.engine.start(continuation.clone()).await.unwrap();
    assert_eq!(wait(&service, &middle).await.status, "completed");
    let second = service.engine.start(continuation).await.unwrap();
    let result = wait(&service, &second).await;
    assert_eq!(result.status, "failed", "{result:?}");
    let log = fs::read_to_string(service.workspace().unwrap().join("lifecycle.log")).unwrap();
    assert_eq!(
        log.lines().collect::<Vec<_>>(),
        ["on_compaction", "on_error"]
    );
    // The third turn asks for a compaction summary first (that request fails
    // too, so the built-in digest is used), then makes its failing request.
    let requests = server.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 4);
    assert!(requests[2].get("tools").is_none(), "summary request");
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn errors_tests_and_compaction_are_audited_without_recursive_hooks() {
    let (_root, service) = fixture("http://127.0.0.1:1/v1");
    enable(
        &service,
        "record",
        &["on_error", "after_test", "on_compaction"],
        "printf '%s\\n' \"$SHADOW_HOOK_EVENT\" >> seen; exit 4",
        json!({}),
    )
    .await;
    let tools = tools(&service, None);
    let error = call(&tools, "read_file", json!({"path":"absent"})).await;
    assert!(!error.success);
    assert!(error.output["hooks"].is_array());
    let failed = call(&tools, "exec", json!({"command":"echo pytest; exit 1"})).await;
    assert!(!failed.success);
    tools
        .fire_hooks(hooks::context(
            "on_compaction",
            "",
            &Value::Null,
            &Value::Null,
            "summarized older turns",
        ))
        .await
        .unwrap();
    let seen = fs::read_to_string(service.workspace().unwrap().join("seen")).unwrap();
    assert_eq!(
        seen.lines().collect::<Vec<_>>(),
        ["on_error", "after_test", "on_error", "on_compaction"]
    );
    service.engine.shutdown().await.unwrap();
}
