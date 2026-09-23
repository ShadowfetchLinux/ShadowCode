//! Vendor CLI backend tests. Protocol cases use hand-written fixtures.
//! Spawn tests use a fake binary; they never call real vendor CLIs.
use serde_json::{json, Value};
use shadowcode_core::cli_agent::{
    adapter_for, catalog_models, doctor, resolve_vendor, vendor_model, CliAdapter, CliAgentsConfig,
    LaunchOptions, Update, Vendor, MAX_MALFORMED_LINES,
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    time::Duration,
};
use tokio_util::sync::CancellationToken;

fn launch(root: &Path) -> LaunchOptions {
    LaunchOptions {
        binary: "codex".into(),
        workspace: root.to_path_buf(),
        model: "default".into(),
        read_only: false,
        resume: None,
    }
}

fn feed(adapter: &mut dyn CliAdapter, lines: &[&str]) -> (Vec<String>, Vec<Update>) {
    let mut send = Vec::new();
    let mut updates = Vec::new();
    for line in lines {
        let step = adapter.on_line(line).unwrap();
        send.extend(step.send);
        updates.extend(step.updates);
    }
    (send, updates)
}

fn rpc(id: u64, method: &str, params: Value) -> String {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}).to_string()
}
fn rpc_result(id: u64, result: Value) -> String {
    json!({"jsonrpc":"2.0","id":id,"result":result}).to_string()
}
fn note(method: &str, params: Value) -> String {
    json!({"jsonrpc":"2.0","method":method,"params":params}).to_string()
}

#[test]
fn vendor_catalog_and_config_validation() {
    let config = CliAgentsConfig::default();
    let models = catalog_models(&config);
    assert_eq!(models.len(), 5);
    assert!(models.iter().all(|m| m["metadata"]["vendor_agent"] == true));
    assert_eq!(resolve_vendor("cli:codex").unwrap().provider, "cli:codex");
    assert_eq!(resolve_vendor("cli:cursor:auto").unwrap().name, "auto");
    assert_eq!(resolve_vendor("cli:antigravity").unwrap().provider, "cli:antigravity");
    assert_eq!(vendor_model(Vendor::Grok, None).api_key_env, "UNUSED");
    let mut bad = config.clone();
    bad.codex_binary.clear();
    assert!(bad.validate().is_err());
    bad = CliAgentsConfig::default();
    bad.approval_timeout_sec = 1;
    assert!(bad.validate().is_err());
    let mut off = CliAgentsConfig::default();
    off.claude_enabled = false;
    assert!(!off.vendor_enabled(Vendor::Claude));
    assert!(off.vendor_enabled(Vendor::Codex));
    assert!(off.vendor_enabled(Vendor::Cursor));
    assert_eq!(catalog_models(&off).len(), 4);
}

#[test]
fn codex_app_server_streams_tools_and_approves() {
    let root = tempfile::tempdir().unwrap();
    let mut adapter = adapter_for(Vendor::Codex, false);
    let start = adapter.on_start(&launch(root.path()));
    assert!(start[0].contains("initialize"));
    adapter.prompt("hello", &[]).unwrap();
    let (send, updates) = feed(
        &mut *adapter,
        &[
            &rpc_result(1, json!({"protocolVersion":2})),
            &rpc_result(2, json!({"thread":{"id":"thr-1"}})),
            &rpc_result(3, json!({"turn":{"id":"turn-1"}})),
            &note(
                "item/agentMessage/delta",
                json!({"itemId":"m1","delta":"Hello "}),
            ),
            &note(
                "item/agentMessage/delta",
                json!({"itemId":"m1","delta":"world"}),
            ),
            &note(
                "item/started",
                json!({"item":{"id":"c1","type":"commandExecution","command":"ls"}}),
            ),
            &note(
                "item/completed",
                json!({"item":{"id":"c1","type":"commandExecution","status":"completed","exitCode":0,"aggregatedOutput":"ok","command":"ls"}}),
            ),
            &note(
                "item/completed",
                json!({"item":{"id":"f1","type":"fileChange","status":"completed","changes":[{"path":"src/main.rs","kind":{"type":"update"}}]}}),
            ),
            &rpc(
                99,
                "item/commandExecution/requestApproval",
                json!({"command":"rm -rf /tmp/demo","reason":"outside sandbox","itemId":"c2"}),
            ),
        ],
    );
    assert!(send.iter().any(|l| l.contains("initialized")));
    assert!(send.iter().any(|l| l.contains("thread/start")));
    assert!(send.iter().any(|l| l.contains("turn/start")));
    assert!(updates.iter().any(|u| matches!(u, Update::Text(t) if t == "Hello ")));
    assert!(updates.iter().any(|u| matches!(u, Update::Text(t) if t == "world")));
    assert!(updates.iter().any(
        |u| matches!(u, Update::ToolStarted { name, .. } if name == "codex.command_execution")
    ));
    assert!(updates.iter().any(
        |u| matches!(u, Update::FilesChanged { paths, .. } if paths == &["src/main.rs".to_string()])
    ));
    let approval = updates
        .iter()
        .find_map(|u| match u {
            Update::Approval(p) => Some(p.clone()),
            _ => None,
        })
        .expect("approval");
    assert_eq!(approval.kind, "command");
    assert!(approval.command.contains("rm -rf"));
    let reply = adapter.approve(&approval.request_id, true).unwrap();
    assert!(reply[0].contains("\"decision\":\"accept\""));
    let denied = {
        let more = adapter
            .on_line(&rpc(
                100,
                "item/fileChange/requestApproval",
                json!({"reason":"write","itemId":"f2"}),
            ))
            .unwrap();
        let id = more
            .updates
            .iter()
            .find_map(|u| match u {
                Update::Approval(p) => Some(p.request_id.clone()),
                _ => None,
            })
            .unwrap();
        adapter.approve(&id, false).unwrap()
    };
    assert!(denied[0].contains("\"decision\":\"decline\""));
    let (_, done) = feed(
        &mut *adapter,
        &[&note(
            "turn/completed",
            json!({"turn":{"id":"turn-1","status":"completed"}}),
        )],
    );
    assert!(matches!(
        done.last(),
        Some(Update::TurnCompleted { interrupted: false, .. })
    ));
}

#[test]
fn codex_refuses_to_refresh_vendor_tokens() {
    let mut adapter = adapter_for(Vendor::Codex, false);
    let step = adapter
        .on_line(&rpc(7, "account/chatgptAuthTokens/refresh", json!({})))
        .unwrap();
    assert!(step.send[0].contains("does not hold or refresh"));
    assert!(step.updates.iter().any(|u| matches!(u, Update::Warning(t) if t.contains("login"))));
}

#[test]
fn codex_exec_fallback_maps_jsonl() {
    let root = tempfile::tempdir().unwrap();
    let mut adapter = adapter_for(Vendor::Codex, true);
    let (bin, args) = adapter.command(&launch(root.path()));
    assert_eq!(bin, "codex");
    assert!(args.contains(&"exec".into()));
    assert!(args.contains(&"--json".into()));
    adapter.prompt("hi", &[]).unwrap();
    adapter.on_start(&launch(root.path()));
    let (_, updates) = feed(
        &mut *adapter,
        &[
            r#"{"type":"item.completed","item":{"id":"m","type":"agent_message","text":"done"}}"#,
            r#"{"type":"item.started","item":{"id":"c","type":"command_execution","command":"echo"}}"#,
            r#"{"type":"item.completed","item":{"id":"c","type":"command_execution","command":"echo","exit_code":0,"aggregated_output":"hi"}}"#,
            r#"{"type":"turn.completed","usage":{"input_tokens":3,"output_tokens":2}}"#,
        ],
    );
    assert!(updates.iter().any(|u| matches!(u, Update::Text(t) if t == "done")));
    assert!(updates.iter().any(|u| matches!(u, Update::Usage { input: 3, output: 2 })));
    assert!(adapter.one_shot());
    assert!(adapter.approve("x", true).is_err());
}

#[test]
fn grok_acp_permission_round_trip_and_cancel() {
    let root = tempfile::tempdir().unwrap();
    let mut adapter = adapter_for(Vendor::Grok, false);
    let (bin, args) = adapter.command(&LaunchOptions {
        binary: "grok".into(),
        workspace: root.path().to_path_buf(),
        model: "grok-4".into(),
        read_only: false,
        resume: None,
    });
    assert_eq!(bin, "grok");
    assert_eq!(args, vec!["agent", "--model", "grok-4", "stdio"]);
    adapter.on_start(&launch(root.path()));
    adapter.prompt("edit the file", &[]).unwrap();
    let (send, _) = feed(
        &mut *adapter,
        &[
            &rpc_result(1, json!({"protocolVersion":1})),
            &rpc_result(2, json!({"sessionId":"sess-1"})),
        ],
    );
    assert!(send.iter().any(|l| l.contains("session/prompt")));
    let step = adapter
        .on_line(&rpc(
            44,
            "session/request_permission",
            json!({
                "options":[
                    {"optionId":"allow","kind":"allow_once"},
                    {"optionId":"deny","kind":"reject_once"}
                ],
                "toolCall":{"title":"run ls","kind":"execute","rawInput":{"command":"ls -la"},"toolCallId":"t1"}
            }),
        ))
        .unwrap();
    let prompt = step.updates.iter().find_map(|u| match u {
        Update::Approval(p) => Some(p.clone()),
        _ => None,
    }).unwrap();
    assert_eq!(prompt.kind, "command");
    let allow = adapter.approve(&prompt.request_id, true).unwrap();
    assert!(allow[0].contains("\"optionId\":\"allow\""));
    let denied = adapter
        .on_line(&rpc(
            45,
            "session/request_permission",
            json!({
                "options":[
                    {"optionId":"allow","kind":"allow_once"},
                    {"optionId":"deny","kind":"reject_once"}
                ],
                "toolCall":{"title":"write","kind":"edit","rawInput":{},"toolCallId":"t2"}
            }),
        ))
        .unwrap();
    let id = match &denied.updates[0] {
        Update::Approval(p) => p.request_id.clone(),
        _ => panic!("expected approval"),
    };
    let reject = adapter.approve(&id, false).unwrap();
    assert!(reject[0].contains("\"optionId\":\"deny\""));
    let fs = adapter
        .on_line(&rpc(46, "fs/write_text_file", json!({"path":"x"})))
        .unwrap();
    assert!(fs.send[0].contains("no fs/terminal"));
    let chunk = adapter
        .on_line(&note(
            "session/update",
            json!({"update":{"sessionUpdate":"agent_message_chunk","content":{"text":"hi"}}}),
        ))
        .unwrap();
    assert!(matches!(chunk.updates[0], Update::Text(ref t) if t == "hi"));
    let interrupt = adapter.interrupt();
    assert!(interrupt[0].contains("session/cancel"));
    let prompt_id = send
        .iter()
        .find(|l| l.contains("session/prompt"))
        .and_then(|l| serde_json::from_str::<Value>(l).ok())
        .and_then(|v| v["id"].as_u64())
        .unwrap();
    let stop = adapter
        .on_line(&rpc_result(prompt_id, json!({"stopReason":"cancelled"})))
        .unwrap();
    assert!(matches!(
        stop.updates[0],
        Update::TurnCompleted { interrupted: true, .. }
    ));
}

#[test]
fn claude_stream_json_approval_and_interrupt() {
    let root = tempfile::tempdir().unwrap();
    let mut adapter = adapter_for(Vendor::Claude, false);
    let (_, args) = adapter.command(&LaunchOptions {
        binary: "claude".into(),
        workspace: root.path().to_path_buf(),
        model: "default".into(),
        read_only: true,
        resume: None,
    });
    assert!(args.contains(&"--output-format".into()));
    assert!(args.contains(&"stream-json".into()));
    assert!(args.contains(&"--permission-prompts".into()));
    assert!(args.contains(&"plan".into()));
    adapter.prompt("hello", &[]).unwrap();
    adapter.on_start(&launch(root.path()));
    let (_, updates) = feed(
        &mut *adapter,
        &[
            r#"{"type":"system","subtype":"init","session_id":"s"}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"Hi"}}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"tu1","name":"Edit","input":{"file_path":"a.rs"}}]}}"#,
            r#"{"type":"control_request","request_id":"req-1","request":{"subtype":"can_use_tool","tool_name":"Edit","input":{"file_path":"a.rs"}}}"#,
        ],
    );
    assert!(updates.iter().any(|u| matches!(u, Update::Text(t) if t == "Hi")));
    assert!(updates.iter().any(|u| matches!(u, Update::ToolStarted { name, .. } if name == "claude.Edit")));
    let approval = updates.iter().find_map(|u| match u {
        Update::Approval(p) => Some(p.clone()),
        _ => None,
    }).unwrap();
    assert_eq!(approval.kind, "file_change");
    let allow = adapter.approve(&approval.request_id, true).unwrap();
    assert!(allow[0].contains("\"behavior\":\"allow\""));
    let deny = adapter
        .on_line(r#"{"type":"control_request","request_id":"req-2","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"rm -rf /"}}}"#)
        .unwrap();
    let id = match &deny.updates[0] {
        Update::Approval(p) => p.request_id.clone(),
        _ => panic!(),
    };
    let denied = adapter.approve(&id, false).unwrap();
    assert!(denied[0].contains("\"behavior\":\"deny\""));
    let interrupt = adapter.interrupt();
    assert!(interrupt[0].contains("interrupt"));
    let (_, done) = feed(
        &mut *adapter,
        &[r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"tu1","content":"wrote"}]}}"#,
          r#"{"type":"result","subtype":"success","usage":{"input_tokens":1,"output_tokens":2},"result":"ignored"}"#],
    );
    assert!(done.iter().any(|u| matches!(u, Update::FilesChanged { paths, .. } if paths == &["a.rs".to_string()])));
    assert!(done.iter().any(|u| matches!(u, Update::TurnCompleted { interrupted: false, .. })));
}

#[test]
fn malformed_lines_are_warnings_not_fatals() {
    for vendor in Vendor::ALL {
        for fallback in [false, vendor == Vendor::Codex] {
        if vendor != Vendor::Codex && fallback {
            continue;
        }
        let mut adapter = adapter_for(vendor, fallback);
        for line in ["", "not-json", "[]", "123", "{\"no\":\"method\"}"] {
            let step = adapter.on_line(line).unwrap();
            if line.is_empty() {
                assert!(step.updates.is_empty());
            } else {
                assert!(
                    step.updates
                        .iter()
                        .any(|u| matches!(u, Update::Warning(_))),
                    "{vendor:?} fallback={fallback} {line}"
                );
            }
        }
        }
    }
    assert!(MAX_MALFORMED_LINES >= 8);
}

#[tokio::test]
async fn doctor_three_states_never_print_credentials() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let bin = root.path().join("bin");
    fs::create_dir_all(home.join(".codex")).unwrap();
    fs::create_dir_all(home.join(".grok")).unwrap();
    fs::create_dir_all(&bin).unwrap();
    // Fake CLIs answer their documented status commands from the fixture
    // home; ShadowCode never reads the credential file itself.
    write_script(
        &bin.join("codex"),
        "#!/bin/sh\nif [ \"$1\" = login ]; then if [ -f \"$HOME/.codex/auth.json\" ]; then echo 'Logged in using ChatGPT'; exit 0; else echo 'Not logged in'; exit 1; fi; fi\necho 'codex 0.0.0-fake'\n",
    );
    write_script(
        &bin.join("grok"),
        "#!/bin/sh\nif [ \"$1\" = models ]; then if [ -f \"$HOME/.grok/auth.json\" ]; then echo 'You are logged in with grok.com.'; echo '  * grok-4.7 (default)'; exit 0; else echo 'Not logged in. Run `grok login`.'; exit 1; fi; fi\necho 'grok 0.0.0-fake'\n",
    );
    write_script(
        &bin.join("claude"),
        "#!/bin/sh\nif [ \"$1\" = auth ]; then echo '{\"loggedIn\":false}'; else echo 'claude 0.0.0-fake'; fi\n",
    );
    let path = bin.as_os_str();
    let config = CliAgentsConfig::default();
    let missing = doctor::check_vendor(
        Vendor::Codex,
        &config,
        &home,
        Some(root.path().join("empty").as_os_str()),
    )
    .await;
    assert_eq!(missing["state"], "not_installed");
    assert_eq!(missing["status"], "info");
    let logged_out = doctor::check_vendor(Vendor::Codex, &config, &home, Some(path)).await;
    assert_eq!(logged_out["state"], "not_logged_in");
    fs::write(
        home.join(".codex/auth.json"),
        r#"{"access_token":"SECRET-TOKEN-VALUE-DO-NOT-PRINT"}"#,
    )
    .unwrap();
    let ready = doctor::check_vendor(Vendor::Codex, &config, &home, Some(path)).await;
    assert_eq!(ready["state"], "ready");
    let dump = ready.to_string();
    assert!(!dump.contains("SECRET-TOKEN-VALUE-DO-NOT-PRINT"));
    assert!(!dump.contains("access_token"));
    let grok = doctor::check_vendor(Vendor::Grok, &config, &home, Some(path)).await;
    assert_eq!(grok["state"], "not_logged_in");
    let claude = doctor::check_vendor(Vendor::Claude, &config, &home, Some(path)).await;
    assert_eq!(claude["state"], "not_logged_in");
    let mut disabled = config.clone();
    disabled.claude_enabled = false;
    let off = doctor::check_vendor(Vendor::Claude, &disabled, &home, Some(path)).await;
    assert_eq!(off["state"], "disabled");
}

fn write_script(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    let mut perm = fs::metadata(path).unwrap().permissions();
    perm.set_mode(0o755);
    fs::set_permissions(path, perm).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn fake_binary_spawn_approval_and_cancel() {
    use shadowcode_core::{
        approvals::ApprovalHub, cli_agent::runner, events::TaskEvents, paths::AppPaths,
        store::Store,
    };
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("project");
    fs::create_dir(&workspace).unwrap();
    let fake = root.path().join("fake-codex");
    write_script(&fake, FAKE_CODEX);
    let paths = AppPaths::isolated(&root.path().join("profile")).unwrap();
    let store = std::sync::Arc::new(Store::open(&paths.database()).unwrap());
    let session = store.create_session(&workspace, "cli:codex", "").unwrap();
    let session_id = session["id"].as_str().unwrap().to_owned();
    let task_id = store.create_task(&session_id, "hello").unwrap();
    let (sender, mut rx) = tokio::sync::broadcast::channel(64);
    let events = TaskEvents {
        store: store.clone(),
        session_id: session_id.clone(),
        task_id: task_id.clone(),
        sender,
    };
    let approvals = ApprovalHub::default();
    let cancel = CancellationToken::new();
    let steer = shadowcode_core::steering::SteerControl::default();
    let mut config = CliAgentsConfig::default();
    config.approval_timeout_sec = 15;
    config.stall_timeout_sec = 30;
    let hub = approvals.clone();
    let approve = tokio::spawn(async move {
        for _ in 0..80 {
            tokio::time::sleep(Duration::from_millis(25)).await;
            if let Some(record) = hub.list(None).into_iter().next() {
                let _ = hub.decide(&record.id, &record.session_id, true);
                return;
            }
        }
        panic!("vendor approval never appeared");
    });
    let result = tokio::time::timeout(
        Duration::from_secs(20),
        runner::run(runner::Request {
            vendor: Vendor::Codex,
            options: LaunchOptions {
                binary: fake.to_string_lossy().into_owned(),
                workspace: workspace.clone(),
                model: "default".into(),
                read_only: false,
                resume: None,
            },
            config: &config,
            prompt: "hello".into(),
            images: Vec::new(),
            session_id: session_id.clone(),
            task_id: task_id.clone(),
            job_id: "j".into(),
            events: &events,
            approvals: &approvals,
            cancel: cancel.clone(),
            steer: &steer,
        }),
    )
    .await
    .expect("spawn approval run timed out")
    .unwrap();
    approve.await.unwrap();
    assert!(result.text.contains("Hello from fake Codex"));
    let mut kinds = Vec::new();
    while let Ok(event) = rx.try_recv() {
        if let Some(kind) = event["type"].as_str() {
            kinds.push(kind.to_owned());
        }
    }
    assert!(kinds.iter().any(|k| k == "approval.requested"));
    assert!(kinds.iter().any(|k| k == "approval.resolved"));
    assert!(kinds.iter().any(|k| k == "model.stream"));

    let cancel = CancellationToken::new();
    let killer = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(40)).await;
        killer.cancel();
    });
    let slow = root.path().join("slow-codex");
    write_script(&slow, FAKE_SLOW_CODEX);
    let err = tokio::time::timeout(
        Duration::from_secs(15),
        runner::run(runner::Request {
            vendor: Vendor::Codex,
            options: LaunchOptions {
                binary: slow.to_string_lossy().into_owned(),
                workspace,
                model: "default".into(),
                read_only: false,
                resume: None,
            },
            config: &config,
            prompt: "slow".into(),
            images: Vec::new(),
            session_id,
            task_id: "t2".into(),
            job_id: "j2".into(),
            events: &events,
            approvals: &approvals,
            cancel,
            steer: &steer,
        }),
    )
    .await
    .expect("cancel run timed out")
    .err()
    .unwrap();
    assert!(format!("{err:#}").contains("cancelled"));
}

const FAKE_CODEX: &str = r#"#!/usr/bin/env python3
import json, sys
def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()
if "--version" in sys.argv:
    print("fake-codex 0.0.0")
    raise SystemExit(0)
if "--help" in sys.argv:
    print("app-server\nexec")
    raise SystemExit(0)
pending = None
for raw in sys.stdin:
    msg = json.loads(raw)
    method = msg.get("method")
    mid = msg.get("id")
    if method == "initialize":
        send({"jsonrpc":"2.0","id":mid,"result":{"protocolVersion":2}})
    elif method == "thread/start":
        send({"jsonrpc":"2.0","id":mid,"result":{"thread":{"id":"thr"}}})
    elif method == "turn/start":
        send({"jsonrpc":"2.0","id":mid,"result":{"turn":{"id":"tn"}}})
        send({"jsonrpc":"2.0","id":7,"method":"item/commandExecution/requestApproval","params":{"command":"echo approved","reason":"test"}})
        pending = True
    elif "result" in msg and pending:
        pending = False
        send({"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"itemId":"m","delta":"Hello from fake Codex"}})
        send({"jsonrpc":"2.0","method":"turn/completed","params":{"turn":{"id":"tn","status":"completed"}}})
"#;

const FAKE_SLOW_CODEX: &str = r#"#!/usr/bin/env python3
import json, sys, time
def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()
if "--help" in sys.argv:
    print("app-server")
    raise SystemExit(0)
for raw in sys.stdin:
    msg = json.loads(raw)
    if msg.get("method") == "initialize":
        send({"jsonrpc":"2.0","id":msg["id"],"result":{}})
    elif msg.get("method") == "thread/start":
        send({"jsonrpc":"2.0","id":msg["id"],"result":{"thread":{"id":"t"}}})
    elif msg.get("method") == "turn/start":
        time.sleep(30)
"#;

#[test]
fn cursor_acp_command_and_cancel() {
    let root = tempfile::tempdir().unwrap();
    let mut adapter = adapter_for(Vendor::Cursor, false);
    let (bin, args) = adapter.command(&LaunchOptions {
        binary: "cursor-agent".into(),
        workspace: root.path().to_path_buf(),
        model: "auto".into(),
        read_only: false,
        resume: None,
    });
    assert_eq!(bin, "cursor-agent");
    assert_eq!(args, vec!["acp"]);
    adapter.on_start(&LaunchOptions {
        model: "gpt-5.5[context=272k,reasoning=medium,fast=false]".into(),
        ..launch(root.path())
    });
    adapter.prompt("hello", &[]).unwrap();
    let (send, updates) = feed(
        &mut *adapter,
        &[
            &rpc_result(1, json!({"protocolVersion":1,"authMethods":[{"id":"cursor_login"}]})),
            &rpc_result(2, json!({})),
            &rpc_result(3, json!({"sessionId":"cur-1","modes":{"availableModes":[{"id":"agent"},{"id":"plan"}]}})),
        ],
    );
    assert!(send.iter().any(|l| l.contains("\"authenticate\"") && l.contains("cursor_login")));
    assert!(send.iter().any(|l| l.contains("session/set_model") && l.contains("gpt-5.5[context=272k")));
    assert!(send.iter().any(|l| l.contains("session/prompt")));
    assert!(updates.iter().any(|u| matches!(u, Update::NativeSession { id } if id == "cur-1")));
    let interrupt = adapter.interrupt();
    assert!(interrupt[0].contains("session/cancel"));
    let prompt_id = send
        .iter()
        .find(|l| l.contains("session/prompt"))
        .and_then(|l| serde_json::from_str::<Value>(l).ok())
        .and_then(|v| v["id"].as_u64())
        .unwrap();
    let stop = adapter
        .on_line(&rpc_result(prompt_id, json!({"stopReason":"cancelled"})))
        .unwrap();
    assert!(matches!(
        stop.updates[0],
        Update::TurnCompleted { interrupted: true, .. }
    ));
}

#[test]
fn antigravity_documented_stream_json_protocol() {
    let root = tempfile::tempdir().unwrap();
    let mut adapter = adapter_for(Vendor::Antigravity, false);
    let (bin, args) = adapter.command(&LaunchOptions {
        binary: "agy".into(),
        workspace: root.path().to_path_buf(),
        model: "gemini-3.8-flash-high".into(),
        read_only: true,
        resume: Some("conv-9".into()),
    });
    assert_eq!(bin, "agy");
    // `--print=` must be last so no later flag is swallowed as the prompt.
    assert_eq!(args.last().map(String::as_str), Some("--print="));
    assert!(args.windows(2).any(|w| w == ["--output-format", "stream-json"]));
    assert!(args.windows(2).any(|w| w == ["--input-format", "stream-json"]));
    assert!(args.windows(2).any(|w| w == ["--mode", "plan"]));
    assert!(args.windows(2).any(|w| w == ["--conversation", "conv-9"]));
    assert!(adapter.prompt("hi", &[sample_image()]).is_err(), "images are refused");
    adapter.prompt("Reply OK", &[]).unwrap();
    let first = adapter.on_start(&launch(root.path()));
    assert!(first[0].contains("\"event\":\"user\""));
    assert!(first[0].contains("\"type\":\"text\""));
    // Frames recorded from agy 1.2.9 plus a tool step.
    let (_, updates) = feed(
        &mut *adapter,
        &[
            r#"{"event":"init","conversation_id":"0114b7b4","init":{"model":"gemini-3.8-flash-low","cwd":"/tmp","tools":["run_command"]}}"#,
            r#"{"event":"step_update","step_update":{"conversation_id":"0114b7b4","step_index":0,"state":"DONE","step_type":"user_input"}}"#,
            r#"{"event":"step_update","step_update":{"conversation_id":"0114b7b4","step_index":1,"state":"ACTIVE","step_type":"tool","tool_name":"write_to_file","tool_info":{"name":"write_to_file","parameters":{"file_path":"a.rs"}}}}"#,
            r#"{"event":"step_update","step_update":{"conversation_id":"0114b7b4","step_index":1,"state":"DONE","step_type":"tool","tool_name":"write_to_file","tool_info":{"name":"write_to_file","parameters":{"file_path":"a.rs"},"output":"written"}}}"#,
            r#"{"event":"step_update","step_update":{"conversation_id":"0114b7b4","step_index":2,"state":"ACTIVE","step_type":"agent_response","text_delta":"OK"}}"#,
            r#"{"event":"step_update","step_update":{"conversation_id":"0114b7b4","step_index":2,"state":"DONE","step_type":"agent_response","text_delta":"\n","usage":{"input_tokens":11903,"output_tokens":22}}}"#,
            r#"{"event":"result","result":{"conversation_id":"0114b7b4","status":"SUCCESS","response":"OK\n","duration_seconds":2.2,"num_turns":1,"usage":{"input_tokens":11903,"output_tokens":22,"thinking_tokens":21,"cache_read_tokens":0,"total_tokens":11925}}}"#,
        ],
    );
    assert!(updates.iter().any(|u| matches!(u, Update::NativeSession { id } if id == "0114b7b4")));
    assert!(updates.iter().any(|u| matches!(u, Update::ToolStarted { name, .. } if name == "antigravity.write_to_file")));
    assert!(updates.iter().any(|u| matches!(u, Update::ToolCompleted { success: true, .. })));
    assert!(updates.iter().any(|u| matches!(u, Update::FilesChanged { paths, .. } if paths == &vec!["a.rs".to_string()])));
    assert!(updates.iter().any(|u| matches!(u, Update::Text(t) if t == "OK")));
    assert!(updates.iter().any(|u| matches!(u, Update::Usage { input: 11903, output: 22 })));
    // Streamed text is not repeated from the result frame.
    assert!(updates.iter().any(|u| matches!(u, Update::TurnCompleted { text: None, interrupted: false })));
    assert_eq!(adapter.native_session().as_deref(), Some("0114b7b4"));
    assert!(adapter.interrupt().is_empty(), "no documented interrupt frame");
    let (_, failed) = feed(
        &mut *adapter,
        &[r#"{"event":"result","result":{"conversation_id":"0114b7b4","status":"ERROR","error":"quota"}}"#],
    );
    assert!(failed.iter().any(|u| matches!(u, Update::TurnFailed(e) if e.contains("quota"))));
}

#[test]
fn picker_routes_by_stable_id_not_display_name() {
    use shadowcode_core::cli_agent::picker::{target_id, vendor_target, Availability};
    use shadowcode_core::cli_agent::usage::UsageSnapshot;
    let cursor = vendor_target(
        Vendor::Cursor,
        "sonnet",
        "Claude 3.5 Sonnet",
        Availability::Ready,
        "Ready",
        UsageSnapshot::unavailable("Cursor"),
        false,
    );
    let claude = vendor_target(
        Vendor::Claude,
        "sonnet",
        "Claude 3.5 Sonnet",
        Availability::Ready,
        "Ready",
        UsageSnapshot::unavailable("Claude Code"),
        false,
    );
    assert_ne!(cursor.id, claude.id);
    assert_eq!(cursor.id, target_id(Vendor::Cursor, "sonnet"));
    assert_eq!(resolve_vendor(&cursor.id).unwrap().provider, "cli:cursor");
    assert_eq!(resolve_vendor(&claude.id).unwrap().provider, "cli:claude");
}

fn sample_image() -> shadowcode_core::cli_agent::PromptImage {
    shadowcode_core::cli_agent::PromptImage {
        mime: "image/png".into(),
        absolute_path: Path::new("/tmp/shot.png").to_path_buf(),
        data_base64: "aW1n".into(),
    }
}

#[test]
fn vendor_image_bytes_use_official_fields() {
    let root = tempfile::tempdir().unwrap();
    let image = sample_image();
    let mut codex = adapter_for(Vendor::Codex, false);
    let _ = codex.on_start(&launch(root.path()));
    let _ = codex.prompt("look", std::slice::from_ref(&image)).unwrap();
    let (send, _) = feed(
        &mut *codex,
        &[
            &rpc_result(1, json!({"protocolVersion":2})),
            &rpc_result(2, json!({"thread":{"id":"thr-img"}})),
        ],
    );
    let turn = send.iter().find(|l| l.contains("turn/start")).unwrap();
    assert!(turn.contains("localImage"));
    assert!(turn.contains("/tmp/shot.png"));
    assert!(!turn.contains("aW1n"));

    let mut claude = adapter_for(Vendor::Claude, false);
    let _ = claude.on_start(&launch(root.path()));
    let line = claude.prompt("look", std::slice::from_ref(&image)).unwrap();
    assert!(line[0].contains("\"type\":\"image\""));
    assert!(line[0].contains("base64"));
    assert!(line[0].contains("aW1n"));

    let mut cursor = adapter_for(Vendor::Cursor, false);
    let _ = cursor.on_start(&launch(root.path()));
    let _ = cursor.prompt("look", std::slice::from_ref(&image)).unwrap();
    let (send, _) = feed(
        &mut *cursor,
        &[
            &rpc_result(1, json!({"protocolVersion":1})),
            &rpc_result(2, json!({"sessionId":"s1"})),
        ],
    );
    let prompt = send.iter().find(|l| l.contains("session/prompt")).unwrap();
    assert!(prompt.contains("\"type\":\"image\""));
    assert!(prompt.contains("image/png"));
    assert!(prompt.contains("aW1n"));

    let mut agy = adapter_for(Vendor::Antigravity, false);
    assert!(agy.prompt("look", std::slice::from_ref(&image)).is_err());
    assert!(Vendor::Codex.accepts_images());
    assert!(Vendor::Claude.accepts_images());
    assert!(Vendor::Cursor.accepts_images());
    assert!(!Vendor::Antigravity.accepts_images());
}
