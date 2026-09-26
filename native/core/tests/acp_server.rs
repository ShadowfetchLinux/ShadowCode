//! `shadowcode acp` driven by an in-process fake ACP client (the editor
//! side) over a duplex pipe, against a fake OpenAI-compatible model.
#![cfg(unix)]
mod support;
use serde_json::{json, Value};
use shadowcode_core::{
    acp_server::{self, AcpOptions},
    config::Config,
    control,
    paths::AppPaths,
    service::Service,
};
use std::{fs, path::PathBuf, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::mpsc,
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

const WAIT: Duration = Duration::from_secs(30);

fn reply(text: &str, calls: Value) -> Value {
    json!({"choices":[{"message":{"role":"assistant","content":text,"tool_calls":calls},"finish_reason":"stop"}]})
}
fn exec(command: &str) -> Value {
    json!([{"id":"call-1","type":"function","function":{"name":"exec","arguments":json!({"command":command}).to_string()}}])
}

/// Answers by the latest message so the order of turns does not matter.
fn model(_: usize, body: &Value) -> (Value, Duration) {
    let messages = body["messages"].as_array().unwrap();
    let last = messages.last().unwrap();
    let asked = |text: &str| {
        messages
            .iter()
            .any(|m| m["role"] == "user" && m["content"].to_string().contains(text))
    };
    let tools = messages.iter().filter(|m| m["role"] == "tool").count();
    if last["role"] == "tool" && asked("run two checks") && tools == 1 {
        return (
            reply("And the second.", exec("printf two > second.txt")),
            Duration::ZERO,
        );
    }
    if last["role"] == "tool" {
        let denied = last["content"].as_str().unwrap_or("").contains("denied");
        return (
            reply(
                if denied {
                    "Skipped the check."
                } else {
                    "Check finished."
                },
                json!([]),
            ),
            Duration::ZERO,
        );
    }
    let prompt = last["content"].to_string();
    if prompt.contains("run the check") || prompt.contains("run two checks") {
        return (
            reply("Running it.", exec("printf approved > marker.txt")),
            Duration::ZERO,
        );
    }
    if prompt.contains("take your time") {
        return (reply("Too late.", json!([])), Duration::from_secs(8));
    }
    if prompt.contains("echo context") {
        let seen = ["SECRET-CONTEXT-123", "UNSAVED-BUFFER-456"]
            .iter()
            .filter(|marker| prompt.contains(*marker))
            .copied()
            .collect::<Vec<_>>()
            .join(",");
        return (
            reply(&format!("Context: {seen}"), json!([])),
            Duration::ZERO,
        );
    }
    (reply("Hello from ShadowCode.", json!([])), Duration::ZERO)
}

struct Fixture {
    _root: tempfile::TempDir,
    project: PathBuf,
    untrusted: PathBuf,
    paths: AppPaths,
    engine: Option<(Service, control::Server)>,
    model: support::Server,
}
impl Fixture {
    /// `desktop`: an engine (as the desktop would) is already running.
    async fn new(desktop: bool) -> Self {
        let model = support::server(model).await;
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        let untrusted = root.path().join("untrusted");
        fs::create_dir(&project).unwrap();
        fs::create_dir(&untrusted).unwrap();
        fs::write(project.join("notes.txt"), "saved text").unwrap();
        let project = project.canonicalize().unwrap();
        let untrusted = untrusted.canonicalize().unwrap();
        let paths = AppPaths::isolated(&root.path().join("profile")).unwrap();
        Config::patch(
            &paths,
            json!({
                "model":{"provider":"local","endpoint":model.endpoint,"name":"fixture","default":"fixture","context_limit":16384},
                "trusted_workspaces":[project],
                "agent":{"max_steps":4,"model_retries":0},
                "cli_agents":{"enabled":false}
            }),
        )
        .unwrap();
        let engine = desktop.then(|| {
            let service = Service::open(paths.clone(), Some(project.clone())).unwrap();
            let server = control::Server::start_with_mode(service.clone(), "desktop").unwrap();
            (service, server)
        });
        Self {
            _root: root,
            project,
            untrusted,
            paths,
            engine,
            model,
        }
    }
    fn connect(&self, options: AcpOptions) -> Editor {
        // One pipe per direction, so dropping the editor's writer is EOF.
        let (editor_writer, reader) = tokio::io::duplex(1 << 20);
        let (writer, editor_reader) = tokio::io::duplex(1 << 20);
        let cancel = CancellationToken::new();
        let worker = tokio::spawn(acp_server::serve_io(
            self.paths.clone(),
            self.project.clone(),
            options,
            reader,
            writer,
            cancel.clone(),
        ));
        let (sender, incoming) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let mut lines = BufReader::new(editor_reader).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let value: Value = serde_json::from_str(&line).expect("agent wrote invalid JSON");
                assert_eq!(value["jsonrpc"], "2.0", "{value}");
                if sender.send(value).is_err() {
                    return;
                }
            }
        });
        Editor {
            writer: editor_writer,
            incoming,
            next: 0,
            worker,
            cancel,
        }
    }
    async fn close(self) {
        if let Some((service, server)) = &self.engine {
            server.close();
            server.wait_closed().await;
            service.engine.shutdown().await.unwrap();
        }
    }
}

/// The editor side of one connection.
struct Editor {
    writer: tokio::io::DuplexStream,
    incoming: mpsc::UnboundedReceiver<Value>,
    next: u64,
    worker: JoinHandle<anyhow::Result<()>>,
    cancel: CancellationToken,
}
/// What arrived before a response: notifications and agent requests.
#[derive(Default, Debug)]
struct Seen {
    updates: Vec<Value>,
    requests: Vec<Value>,
}
impl Seen {
    fn kinds(&self) -> Vec<String> {
        self.updates
            .iter()
            .map(|u| u["sessionUpdate"].as_str().unwrap_or("").to_owned())
            .collect()
    }
    fn text(&self, kind: &str) -> String {
        self.updates
            .iter()
            .filter(|u| u["sessionUpdate"] == kind)
            .map(|u| u["content"]["text"].as_str().unwrap_or(""))
            .collect()
    }
}
impl Editor {
    async fn raw(&mut self, line: &str) {
        self.writer.write_all(line.as_bytes()).await.unwrap();
        self.writer.write_all(b"\n").await.unwrap();
        self.writer.flush().await.unwrap();
    }
    async fn send(&mut self, value: Value) {
        self.raw(&value.to_string()).await;
    }
    async fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({"jsonrpc":"2.0","method":method,"params":params}))
            .await;
    }
    async fn next_message(&mut self) -> Value {
        tokio::time::timeout(WAIT, self.incoming.recv())
            .await
            .expect("agent went quiet")
            .expect("agent closed the connection")
    }
    /// Send a request and collect everything until its response, answering
    /// agent requests with `answer`.
    async fn call_with(
        &mut self,
        method: &str,
        params: Value,
        mut answer: impl FnMut(&Value) -> Value,
    ) -> (Value, Seen) {
        self.next += 1;
        let id = self.next;
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await;
        let mut seen = Seen::default();
        loop {
            let message = self.next_message().await;
            if message["id"] == id && message.get("method").is_none() {
                return (message, seen);
            }
            match message["method"].as_str() {
                Some("session/update") => seen.updates.push(message["params"]["update"].clone()),
                Some(_) if message.get("id").is_some() => {
                    let result = answer(&message);
                    let response = json!({"jsonrpc":"2.0","id":message["id"],"result":result});
                    seen.requests.push(message);
                    self.send(response).await;
                }
                _ => {}
            }
        }
    }
    async fn call(&mut self, method: &str, params: Value) -> (Value, Seen) {
        self.call_with(method, params, |request| {
            panic!("unexpected agent request {request}")
        })
        .await
    }
    async fn close(self) {
        drop(self.writer);
        tokio::time::timeout(WAIT, self.worker)
            .await
            .expect("agent did not stop at EOF")
            .unwrap()
            .unwrap();
        drop(self.cancel);
    }
}

fn select(option: &str) -> Value {
    json!({"outcome":{"outcome":"selected","optionId":option}})
}

async fn initialize(editor: &mut Editor, capabilities: Value) -> Value {
    let (response, _) = editor
        .call(
            "initialize",
            json!({"protocolVersion":1,"clientCapabilities":capabilities,"clientInfo":{"name":"fake-editor","version":"1"}}),
        )
        .await;
    response["result"].clone()
}

async fn new_session(editor: &mut Editor, cwd: &PathBuf) -> Value {
    let (response, _) = editor
        .call("session/new", json!({"cwd":cwd,"mcpServers":[]}))
        .await;
    assert!(response.get("error").is_none(), "{response}");
    response["result"].clone()
}

fn prompt(session: &str, text: &str) -> Value {
    json!({"sessionId":session,"prompt":[{"type":"text","text":text}]})
}

#[tokio::test]
async fn editor_drives_a_turn_permissions_cancel_and_replay_through_a_running_desktop() {
    let f = Fixture::new(true).await;
    let mut editor = f.connect(AcpOptions::default());

    // Protocol errors before and around initialization.
    editor.raw("{not json").await;
    let parse = editor.next_message().await;
    assert_eq!(parse["error"]["code"], -32700, "{parse}");
    let (early, _) = editor.call("session/new", json!({"cwd":f.project})).await;
    assert_eq!(early["error"]["code"], -32600, "{early}");

    let init = initialize(&mut editor, json!({"fs":{"readTextFile":false}})).await;
    assert_eq!(init["protocolVersion"], 1);
    assert_eq!(init["agentCapabilities"]["loadSession"], true);
    assert_eq!(
        init["agentCapabilities"]["promptCapabilities"]["image"],
        true
    );
    assert_eq!(
        init["agentCapabilities"]["promptCapabilities"]["embeddedContext"],
        true
    );
    assert_eq!(init["agentInfo"]["name"], "shadowcode");
    assert_eq!(init["authMethods"], json!([]));
    let (auth, _) = editor
        .call("authenticate", json!({"methodId":"none"}))
        .await;
    assert_eq!(auth["result"], json!({}));
    let (unknown, _) = editor.call("session/fly", json!({})).await;
    assert_eq!(unknown["error"]["code"], -32601);

    // Untrusted folders are refused with a clear, actionable error.
    let (refused, _) = editor
        .call("session/new", json!({"cwd":f.untrusted,"mcpServers":[]}))
        .await;
    assert_eq!(refused["error"]["code"], -32602, "{refused}");
    assert_eq!(refused["error"]["data"]["reason"], "untrusted_workspace");
    assert!(refused["error"]["message"]
        .as_str()
        .unwrap()
        .contains("acp --trust"));
    let (relative, _) = editor
        .call("session/new", json!({"cwd":"project","mcpServers":[]}))
        .await;
    assert_eq!(relative["error"]["code"], -32602);

    let created = new_session(&mut editor, &f.project).await;
    let sid = created["sessionId"].as_str().unwrap().to_owned();
    assert_eq!(created["modes"]["currentModeId"], "code");
    let ids: Vec<_> = created["modes"]["availableModes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["code", "plan", "ask"]);
    assert_eq!(created["configOptions"][0]["id"], "mode");
    assert_eq!(created["configOptions"][1]["id"], "model");
    assert_eq!(created["configOptions"][1]["category"], "model");
    assert_eq!(created["configOptions"][1]["currentValue"], "default");

    let (missing, _) = editor
        .call("session/prompt", prompt("no-such-session", "hi"))
        .await;
    assert_eq!(missing["error"]["code"], -32002, "{missing}");

    // A plain turn streams the answer and ends the turn.
    let (done, seen) = editor
        .call("session/prompt", prompt(&sid, "say hello"))
        .await;
    assert_eq!(done["result"]["stopReason"], "end_turn", "{done} {seen:?}");
    assert!(
        seen.text("agent_message_chunk")
            .contains("Hello from ShadowCode."),
        "{seen:?}"
    );
    assert!(seen
        .updates
        .iter()
        .any(|u| u["sessionUpdate"] == "session_info_update" && u["title"].is_string()));

    // A shell command asks the editor; allowing it runs it.
    let (done, seen) = editor
        .call_with("session/prompt", prompt(&sid, "run the check"), |request| {
            assert_eq!(request["method"], "session/request_permission");
            let params = &request["params"];
            assert_eq!(params["toolCall"]["kind"], "execute");
            assert!(params["toolCall"]["title"]
                .as_str()
                .unwrap()
                .contains("marker.txt"));
            let kinds: Vec<_> = params["options"]
                .as_array()
                .unwrap()
                .iter()
                .map(|o| o["kind"].as_str().unwrap())
                .collect();
            assert_eq!(kinds, ["allow_once", "allow_always", "reject_once"]);
            select("allow_once")
        })
        .await;
    assert_eq!(done["result"]["stopReason"], "end_turn", "{done} {seen:?}");
    assert_eq!(seen.requests.len(), 1);
    assert_eq!(
        fs::read_to_string(f.project.join("marker.txt")).unwrap(),
        "approved"
    );
    let call = seen
        .updates
        .iter()
        .find(|u| u["sessionUpdate"] == "tool_call" && u["kind"] == "execute")
        .expect("tool_call for exec");
    let call_id = call["toolCallId"].clone();
    assert_eq!(
        seen.requests[0]["params"]["toolCall"]["toolCallId"], call_id,
        "the permission refers to the announced tool call"
    );
    assert!(seen
        .updates
        .iter()
        .any(|u| u["sessionUpdate"] == "tool_call_update"
            && u["toolCallId"] == call_id
            && u["status"] == "completed"));
    assert!(seen.text("agent_message_chunk").contains("Check finished."));

    // Rejecting leaves the project untouched.
    fs::remove_file(f.project.join("marker.txt")).unwrap();
    let (done, seen) = editor
        .call_with(
            "session/prompt",
            prompt(&sid, "run the check again"),
            |_| select("reject_once"),
        )
        .await;
    assert_eq!(done["result"]["stopReason"], "end_turn", "{done} {seen:?}");
    assert_eq!(seen.requests.len(), 1);
    assert!(!f.project.join("marker.txt").exists());
    assert!(seen
        .updates
        .iter()
        .any(|u| u["sessionUpdate"] == "tool_call_update" && u["status"] == "failed"));
    assert!(f
        .engine
        .as_ref()
        .unwrap()
        .0
        .engine
        .approvals()
        .list(None)
        .is_empty());

    // Modes and config options are remembered for the session.
    let (plan, _) = editor
        .call(
            "session/set_config_option",
            json!({"sessionId":sid,"configId":"mode","value":"plan"}),
        )
        .await;
    assert_eq!(plan["result"]["configOptions"][0]["currentValue"], "plan");
    let (bad_mode, _) = editor
        .call("session/set_mode", json!({"sessionId":sid,"modeId":"yolo"}))
        .await;
    assert_eq!(bad_mode["error"]["code"], -32602);
    let (bad_model, _) = editor
        .call(
            "session/set_model",
            json!({"sessionId":sid,"modelId":"cli:nobody:nothing"}),
        )
        .await;
    assert_eq!(bad_model["error"]["code"], -32602, "{bad_model}");
    let (code, seen) = editor
        .call("session/set_mode", json!({"sessionId":sid,"modeId":"code"}))
        .await;
    assert_eq!(code["result"], json!({}));
    assert!(seen.kinds().contains(&"config_option_update".to_owned()));

    // Cancelling a slow turn stops the job and reports `cancelled`.
    editor.next += 1;
    let id = editor.next;
    editor
        .send(json!({"jsonrpc":"2.0","id":id,"method":"session/prompt","params":prompt(&sid, "take your time")}))
        .await;
    tokio::time::sleep(Duration::from_millis(700)).await;
    editor
        .notify("session/cancel", json!({"sessionId":sid}))
        .await;
    let cancelled = loop {
        let message = editor.next_message().await;
        if message["id"] == id {
            break message;
        }
    };
    assert_eq!(
        cancelled["result"]["stopReason"], "cancelled",
        "{cancelled}"
    );

    // Session list shows the conversation with an RFC 3339 time.
    let (listed, _) = editor.call("session/list", json!({"cwd":f.project})).await;
    let row = listed["result"]["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["sessionId"] == sid.as_str())
        .expect("session listed")
        .clone();
    assert_eq!(row["cwd"], json!(f.project));
    assert!(row["updatedAt"].as_str().unwrap().ends_with('Z'));
    editor.close().await;

    // A new connection (a restarted editor) replays the history.
    let mut editor = f.connect(AcpOptions::default());
    initialize(&mut editor, json!({})).await;
    let (wrong, _) = editor
        .call(
            "session/load",
            json!({"sessionId":sid,"cwd":f.untrusted,"mcpServers":[]}),
        )
        .await;
    assert_eq!(wrong["error"]["code"], -32602);
    let (gone, _) = editor
        .call(
            "session/load",
            json!({"sessionId":"0123456789abcdef","cwd":f.project,"mcpServers":[]}),
        )
        .await;
    assert_eq!(gone["error"]["code"], -32002, "{gone}");
    let (loaded, seen) = editor
        .call(
            "session/load",
            json!({"sessionId":sid,"cwd":f.project,"mcpServers":[]}),
        )
        .await;
    assert!(loaded.get("error").is_none(), "{loaded}");
    assert_eq!(loaded["result"]["modes"]["currentModeId"], "code");
    let users = seen.text("user_message_chunk");
    assert!(
        users.contains("say hello") && users.contains("run the check"),
        "{users}"
    );
    let agent = seen.text("agent_message_chunk");
    assert!(agent.contains("Hello from ShadowCode.") && agent.contains("Check finished."));
    assert!(seen
        .updates
        .iter()
        .any(|u| u["sessionUpdate"] == "tool_call" && u["kind"] == "execute"));
    let (again, _) = editor
        .call("session/prompt", prompt(&sid, "say hello once more"))
        .await;
    assert_eq!(again["result"]["stopReason"], "end_turn", "{again}");
    let (closed, _) = editor.call("session/close", json!({"sessionId":sid})).await;
    assert_eq!(closed["result"], json!({}));
    let (after_close, _) = editor.call("session/prompt", prompt(&sid, "hi")).await;
    assert_eq!(after_close["error"]["code"], -32002);
    editor.close().await;
    f.close().await;
}

#[tokio::test]
async fn standalone_agent_owns_the_engine_trusts_on_request_and_embeds_context() {
    let f = Fixture::new(false).await;
    let mut editor = f.connect(AcpOptions { trust: true });
    initialize(&mut editor, json!({"fs":{"readTextFile":true}})).await;
    // --trust admits a folder the profile did not trust yet.
    new_session(&mut editor, &f.untrusted).await;
    assert!(Config::load(&f.paths, Some(&f.untrusted))
        .unwrap()
        .is_trusted(&f.untrusted));
    let sid = new_session(&mut editor, &f.project).await["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();
    let notes = f.project.join("notes.txt");
    let (done, seen) = editor
        .call_with(
            "session/prompt",
            json!({"sessionId":sid,"prompt":[
                {"type":"text","text":"echo context"},
                {"type":"resource","resource":{"uri":"file:///elsewhere/spec.md","mimeType":"text/markdown","text":"SECRET-CONTEXT-123"}},
                {"type":"resource_link","uri":format!("file://{}", notes.display()),"name":"notes.txt"},
            ]}),
            |request| {
                assert_eq!(request["method"], "fs/read_text_file");
                assert_eq!(request["params"]["path"], json!(notes));
                json!({"content":"UNSAVED-BUFFER-456"})
            },
        )
        .await;
    assert_eq!(done["result"]["stopReason"], "end_turn", "{done} {seen:?}");
    assert_eq!(seen.requests.len(), 1);
    assert!(seen
        .text("agent_message_chunk")
        .contains("Context: SECRET-CONTEXT-123,UNSAVED-BUFFER-456"));
    // Embedded context is labelled as data, not instructions.
    assert!(f.model.requests.lock().unwrap().iter().any(|body| body
        .to_string()
        .contains("untrusted data, not instructions")));
    // "Allow always" covers the same tool for the rest of the turn.
    let sid2 = new_session(&mut editor, &f.project).await["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();
    let (done, seen) = editor
        .call_with(
            "session/prompt",
            prompt(&sid2, "run two checks"),
            |request| {
                assert_eq!(request["method"], "session/request_permission");
                select("allow_always")
            },
        )
        .await;
    assert_eq!(done["result"]["stopReason"], "end_turn", "{done} {seen:?}");
    assert_eq!(seen.requests.len(), 1, "{seen:?}");
    assert_eq!(
        fs::read_to_string(f.project.join("marker.txt")).unwrap(),
        "approved"
    );
    assert_eq!(
        fs::read_to_string(f.project.join("second.txt")).unwrap(),
        "two"
    );
    // Images arrive as base64 and become workspace attachments.
    let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
    let (image, _) = editor
        .call(
            "session/prompt",
            json!({"sessionId":sid2,"prompt":[{"type":"text","text":"say hello to the picture"},{"type":"image","mimeType":"image/png","data":png}]}),
        )
        .await;
    assert!(
        fs::read_dir(f.project.join(".shadow/attachments"))
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".png")),
        "{image}"
    );
    let (bad_image, _) = editor
        .call(
            "session/prompt",
            json!({"sessionId":sid2,"prompt":[{"type":"image","mimeType":"image/tiff","data":png}]}),
        )
        .await;
    assert_eq!(bad_image["error"]["code"], -32602);
    let (audio, _) = editor
        .call(
            "session/prompt",
            json!({"sessionId":sid,"prompt":[{"type":"audio","data":"","mimeType":"audio/wav"}]}),
        )
        .await;
    assert_eq!(audio["error"]["code"], -32602);
    let (empty, _) = editor
        .call("session/prompt", json!({"sessionId":sid,"prompt":[]}))
        .await;
    assert_eq!(empty["error"]["code"], -32602);
    editor.close().await;
    // The agent released the profile: another engine can open it now.
    let service = Service::open(f.paths.clone(), Some(f.project.clone())).unwrap();
    service.engine.shutdown().await.unwrap();
    drop(f);
}
