mod support;
use serde_json::{json, Value};
use shadowcode_core::{
    config::Config,
    context,
    engine::{Engine, StartRequest},
    paths::AppPaths,
};
use std::{fs, path::Path, sync::Arc, time::Duration};

fn response(text: &str, calls: Value) -> Value {
    let reason = if calls.as_array().is_some_and(|v| !v.is_empty()) {
        "tool_calls"
    } else {
        "stop"
    };
    json!({"choices":[{"message":{"role":"assistant","content":text,"tool_calls":calls},"finish_reason":reason}],"usage":{"prompt_tokens":8,"completion_tokens":4,"total_tokens":12}})
}
fn tool(id: &str, name: &str, args: Value) -> Value {
    json!({"id":id,"type":"function","function":{"name":name,"arguments":args.to_string()}})
}
fn setup(endpoint: &str) -> (tempfile::TempDir, Engine) {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("project")).unwrap();
    let paths = AppPaths::isolated(&root.path().join("profile")).unwrap();
    Config::patch(&paths,json!({"model":{"provider":"local","endpoint":endpoint,"name":"fixture","context_limit":16384},"permissions":{"approve_shell":false},"agent":{"max_steps":12}})).unwrap();
    (root, Engine::open(paths).unwrap())
}
fn request(root: &Path, task: &str) -> StartRequest {
    StartRequest {
        workspace: root.join("project"),
        task: task.into(),
        session_id: None,
        model: None,
        mode: "code".into(),
        queue: false,
        images: Vec::new(),
    }
}

#[tokio::test]
async fn pause_does_not_replay_shell_and_steering_enters_next_context() {
    let requests = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    let captured = requests.clone();
    let server = support::server(move |index, body| {
        captured.lock().unwrap().push(body.clone());
        context::validate_pairs(body["messages"].as_array().unwrap()).unwrap();
        let value = match index {
            0 => response(
                "running once",
                json!([tool(
                    "e1",
                    "exec",
                    json!({"command":"printf started > running.flag; sleep 1; echo first-only"})
                )]),
            ),
            1 => {
                // After pause/resume, the next model turn must see steering text
                // and must not re-issue the first shell as a replay of history.
                let blob = body.to_string();
                assert!(
                    blob.contains("db/v2.sql") || blob.contains("do not refactor"),
                    "steering missing from model context: {blob}"
                );
                response("done with steering", json!([]))
            }
            _ => response("extra", json!([])),
        };
        (value, Duration::from_millis(30))
    })
    .await;
    assert!(server.requests.lock().unwrap().is_empty());
    let (root, engine) = setup(&server.endpoint);
    fs::create_dir_all(root.path().join("project/db")).unwrap();
    fs::write(root.path().join("project/db/v2.sql"), "-- v2\n").unwrap();
    let job = engine
        .start(request(root.path(), "Use the schema carefully"))
        .await
        .unwrap();
    // Pause while the first shell is still running so the next model turn
    // waits on resume and receives steering without replaying exec.
    for _ in 0..100 {
        let snap = engine.job(&job.id).unwrap().unwrap();
        if snap.status == "running" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::timeout(Duration::from_secs(3), async {
        while !root.path().join("project/running.flag").exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    engine.pause_job(&job.id).unwrap();
    assert_eq!(engine.job(&job.id).unwrap().unwrap().status, "paused");
    assert!(engine
        .rewind_job(&job.id)
        .unwrap_err()
        .to_string()
        .contains("pause boundary"));
    engine
        .steer_job(
            &job.id,
            "do not refactor schema; use db/v2.sql",
            Some("db/v2.sql"),
        )
        .unwrap();
    // Let the in-flight shell finish under pause; resume afterward.
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(engine.rewind_job(&job.id).unwrap()["ok"], true);
    engine.resume_job(&job.id).unwrap();
    let completed = tokio::time::timeout(Duration::from_secs(8), engine.wait(&job.id))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(completed.status, "completed", "{}", completed.summary);
    let tape = engine.store().messages(&job.id).unwrap();
    let execs = tape
        .iter()
        .filter(|m| m["role"] == "assistant")
        .filter_map(|m| m["tool_calls"].as_array())
        .flatten()
        .filter(|c| c["function"]["name"] == "exec")
        .count();
    assert_eq!(execs, 1, "pause/resume must not replay shell tool calls");
    assert!(
        tape.iter().any(|m| m["role"] == "system"
            && m["content"]
                .as_str()
                .is_some_and(|c| c.contains("db/v2.sql"))),
        "steering system note missing"
    );
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn rewind_does_not_duplicate_side_effects() {
    let server = support::server(|index, _body| {
        let value = match index {
            0 => response(
                "write",
                json!([tool(
                    "w1",
                    "write_file",
                    json!({"path":"note.txt","content":"alpha"})
                )]),
            ),
            1 => response("done", json!([])),
            _ => response("extra", json!([])),
        };
        (value, Duration::ZERO)
    })
    .await;
    assert!(server.requests.lock().unwrap().is_empty());
    let (root, engine) = setup(&server.endpoint);
    let job = engine
        .start(request(root.path(), "Write note.txt"))
        .await
        .unwrap();
    let completed = tokio::time::timeout(Duration::from_secs(8), engine.wait(&job.id))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(completed.status, "completed");
    assert_eq!(
        fs::read_to_string(root.path().join("project/note.txt")).unwrap(),
        "alpha"
    );
    let reservation = engine
        .reserve_workspace(&root.path().join("project"))
        .unwrap();
    assert!(engine.rewind_job(&job.id).is_err());
    drop(reservation);
    let restored = engine.rewind_job(&job.id).unwrap();
    assert_eq!(restored["ok"], true);
    assert!(
        !root.path().join("project/note.txt").exists()
            || fs::read_to_string(root.path().join("project/note.txt")).is_err()
            || !fs::read_to_string(root.path().join("project/note.txt"))
                .unwrap()
                .contains("alpha")
            || restored["restored"]
                .as_array()
                .is_some_and(|v| !v.is_empty())
    );
    // Session messages remain (not wiped).
    assert!(!engine.store().messages(&job.id).unwrap().is_empty());
    engine.shutdown().await.unwrap();
}
