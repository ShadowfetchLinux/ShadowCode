mod support;
use serde_json::{json, Value};
use shadowcode_core::{
    config::Config,
    engine::StartRequest,
    paths::AppPaths,
    service::{Request, Service},
    workflows::{self, Definition},
    workspace::Workspace,
};
use std::{fs, time::Duration};

fn setup(endpoint: &str) -> (tempfile::TempDir, Service) {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    fs::create_dir(&project).unwrap();
    let paths = AppPaths::isolated(&root.path().join("profile")).unwrap();
    Config::patch(&paths,json!({"model":{"name":"fixture","provider":"local","endpoint":endpoint,"context_limit":16384},"trusted_workspaces":[project],"permissions":{"approve_shell":false},"agent":{"retry_attempts":0}})).unwrap();
    (root, Service::open(paths, Some(project)).unwrap())
}
fn put(service: &Service, path: &str, contents: &str) {
    let file = service.workspace().unwrap().join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, contents).unwrap();
}
async fn call(service: &Service, method: &str, path: &str, body: Value) -> anyhow::Result<Value> {
    service
        .dispatch(Request {
            method: method.into(),
            path: path.into(),
            body,
        })
        .await
}
async fn command(service: &Service, name: &str, args: &str, extra: Value) -> anyhow::Result<Value> {
    let mut body = json!({"name":name,"args":args});
    for (key, value) in extra.as_object().unwrap() {
        body[key] = value.clone();
    }
    call(service, "POST", "/api/commands/run", body).await
}
fn response(text: &str) -> Value {
    json!({"choices":[{"message":{"role":"assistant","content":text},"finish_reason":"stop"}]})
}
async fn wait(service: &Service, job: &Value) {
    let done = tokio::time::timeout(
        Duration::from_secs(8),
        service.engine.wait(job["id"].as_str().unwrap()),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(done.status, "completed", "{}", done.summary);
}
#[test]
fn front_matter_and_literal_arguments_have_bounded_expansion() {
    let def=Definition::parse(".agents/skills/audit/SKILL.md","skill","---\nname: audit\nalias: inspect\ndescription: Check a change\nmode: review\n---\nInspect $ARGUMENTS and {{args}}.","hash").unwrap();
    let guidance = def
        .guidance("$(touch unexpected) $ARGUMENTS {{args}}")
        .unwrap();
    assert_eq!(guidance.info.name, "audit");
    assert_eq!(guidance.info.mode, "review");
    assert_eq!(
        guidance
            .instructions
            .matches("$(touch unexpected) $ARGUMENTS {{args}}")
            .count(),
        2
    );
    assert!(def.guidance(&"x".repeat(32001)).is_err());
    for header in [
        "name: 1",
        "mode: false",
        "description: []",
        "alias: {}",
        "mode: admin",
        "allowed-tools: exec",
        "hooks: {}",
        "user-invocable: false",
        "user-invocable: nope",
    ] {
        assert!(
            Definition::parse("a.md", "skill", &format!("---\n{header}\n---\nBody"), "").is_err(),
            "{header}"
        );
    }
    assert!(Definition::parse("a.md", "skill", &"x".repeat(64001), "").is_err());
    let expansion = Definition::parse("a.md", "skill", &"$ARGUMENTS ".repeat(100), "").unwrap();
    assert!(expansion.guidance(&"x".repeat(32000)).is_err());
}
#[tokio::test]
async fn discovery_reports_ambiguity_invalid_metadata_and_confined_paths() {
    let (root, service) = setup("http://127.0.0.1:9/v1");
    put(
        &service,
        ".shadow/commands/check.yaml",
        "Inspect the build.",
    );
    put(
        &service,
        ".shadow/skills/first.md",
        "---\nname: audit\n---\nFirst",
    );
    put(&service, ".agents/skills/audit/SKILL.md", "Second");
    put(
        &service,
        ".shadowcode/skills/invalid.md",
        "---\nmode: admin\n---\nInvalid",
    );
    put(&service, ".shadow/skills/plan.md", "Custom planning");
    let outside = root.path().join("private.md");
    fs::write(&outside, "Never import this").unwrap();
    std::os::unix::fs::symlink(
        outside,
        service
            .workspace()
            .unwrap()
            .join(".shadow/skills/escape.md"),
    )
    .unwrap();
    let catalog = workflows::discover(&Workspace::open(&service.workspace().unwrap()).unwrap());
    assert!(catalog
        .resolve("audit", None)
        .unwrap_err()
        .to_string()
        .contains("ambiguous"));
    assert_eq!(
        catalog.resolve("check", None).unwrap().content,
        "Inspect the build."
    );
    assert!(catalog.resolve("escape", None).is_err());
    assert!(catalog
        .issues
        .iter()
        .any(|issue| issue.contains("invalid.md")));
    let commands = call(&service, "GET", "/api/commands", Value::Null)
        .await
        .unwrap();
    let rows = commands["commands"].as_array().unwrap();
    assert_eq!(rows.iter().filter(|row| row["name"] == "plan").count(), 1);
    assert!(rows.iter().any(|row| row["name"] == "skill plan"));
    assert!(!rows.iter().any(|row| row["name"] == "audit"));
}
#[tokio::test]
async fn selected_skill_reaches_model_with_provenance_and_preserves_read_only_mode() {
    let model = support::server(|_, _| (response("Reviewed."), Duration::ZERO)).await;
    let (_root, service) = setup(&model.endpoint);
    put(
        &service,
        ".agents/skills/audit/SKILL.md",
        "---\nmode: code\n---\nEXPLICIT_SKILL: inspect $ARGUMENTS",
    );
    put(
        &service,
        ".shadow/skills/unselected.md",
        "UNSELECTED_PRIVATE_INSTRUCTION",
    );
    let result = command(
        &service,
        "skill",
        "audit README.md",
        json!({"purpose":"reviewer"}),
    )
    .await
    .unwrap();
    let job = &result["metadata"]["job"];
    wait(&service, job).await;
    assert_eq!(job["mode"], "review");
    assert_eq!(job["task"], "/skill audit README.md");
    assert_eq!(job["routing"]["purpose"], "reviewer");
    assert_eq!(job["workflow"]["path"], ".agents/skills/audit/SKILL.md");
    let requests = model.requests.lock().unwrap().clone();
    let prompt = requests[0]["messages"][0]["content"].as_str().unwrap();
    assert!(prompt.contains("EXPLICIT_SKILL: inspect README.md"));
    assert!(!prompt.contains("UNSELECTED_PRIVATE_INSTRUCTION"));
    let toolnames: Vec<_> = requests[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap())
        .collect();
    assert!(!toolnames.contains(&"write_file"));
    assert!(!toolnames.contains(&"exec"));
    let events = service
        .engine
        .store()
        .recent_events(job["session_id"].as_str().unwrap(), 100)
        .unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| event["type"] == "workflow.selected")
            .count(),
        1
    );
    assert!(!events
        .iter()
        .any(|event| event["type"] == "command.completed"));
    service.engine.shutdown().await.unwrap();
}
#[tokio::test]
async fn queued_workflow_freezes_source_and_project_notes_are_loaded() {
    let model = support::server(|index, _| {
        (
            response("Reviewed."),
            if index == 0 {
                Duration::from_millis(300)
            } else {
                Duration::ZERO
            },
        )
    })
    .await;
    let (_root, service) = setup(&model.endpoint);
    put(
        &service,
        ".shadow/skills/audit.md",
        "---\nmode: plan\n---\nFROZEN_ORIGINAL",
    );
    command(
        &service,
        "memory",
        "Use cargo test for this project.",
        json!({}),
    )
    .await
    .unwrap();
    let first = service
        .engine
        .start(StartRequest {
            workspace: service.workspace().unwrap(),
            task: "Describe approach".into(),
            session_id: None,
            model: None,
            mode: "plan".into(),
            queue: false,
        })
        .await
        .unwrap();
    let result = command(&service, "audit", "", json!({"queue":true}))
        .await
        .unwrap();
    let job = &result["metadata"]["job"];
    assert_eq!(job["mode"], "plan");
    put(&service, ".shadow/skills/audit.md", "CHANGED_AFTER_QUEUE");
    service.engine.wait(&first.id).await.unwrap();
    wait(&service, job).await;
    let requests = model.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 2);
    let prompt = requests[1]["messages"][0]["content"].as_str().unwrap();
    assert!(prompt.contains("FROZEN_ORIGINAL"));
    assert!(!prompt.contains("CHANGED_AFTER_QUEUE"));
    assert!(prompt.contains("Use cargo test for this project."));
    service.engine.shutdown().await.unwrap();
}
#[tokio::test]
async fn command_cards_persist_terminal_failures_and_session_actions_work() {
    let (_root, service) = setup("http://127.0.0.1:9/v1");
    let session = call(&service, "POST", "/api/sessions", json!({}))
        .await
        .unwrap();
    let sid = session["id"].as_str().unwrap();
    let result = command(
        &service,
        "run",
        "printf native-command; exit 7",
        json!({"session_id":sid}),
    )
    .await
    .unwrap();
    assert_eq!(result["kind"], "error");
    assert!(result["body"].as_str().unwrap().contains("native-command"));
    let events = service.engine.store().recent_events(sid, 100).unwrap();
    assert!(events
        .iter()
        .any(|event| event["type"] == "terminal.completed"
            && event["payload"]["result"]["exit_code"] == 7));
    assert!(events
        .iter()
        .any(|event| event["type"] == "command.completed" && event["payload"]["result"] == result));
    service
        .engine
        .store()
        .add_event(
            "agent.completed",
            &json!({"summary":"Completed response"}),
            Some(sid),
            None,
        )
        .unwrap();
    command(
        &service,
        "pin",
        "Useful response",
        json!({"session_id":sid}),
    )
    .await
    .unwrap();
    let branch = command(
        &service,
        "branch",
        "Alternate approach",
        json!({"session_id":sid}),
    )
    .await
    .unwrap();
    let branchid = branch["metadata"]["session_id"].as_str().unwrap();
    assert_ne!(sid, branchid);
    let resumed = command(&service, "resume", sid, json!({})).await.unwrap();
    assert_eq!(resumed["metadata"]["session_id"], sid);
    assert!(command(&service, "unknown", "", json!({})).await.is_err());
    service.engine.shutdown().await.unwrap();
}
#[tokio::test]
async fn untrusted_and_cross_project_requests_have_no_model_or_shell_side_effects() {
    let model = support::server(|_, _| (response("Unexpected"), Duration::ZERO)).await;
    let (root, service) = setup(&model.endpoint);
    let first = call(&service, "POST", "/api/sessions", json!({}))
        .await
        .unwrap();
    let other = root.path().join("other");
    fs::create_dir(&other).unwrap();
    call(
        &service,
        "POST",
        "/api/sessions",
        json!({"workspace":other}),
    )
    .await
    .unwrap();
    assert!(command(
        &service,
        "run",
        "touch unexpected",
        json!({"session_id":first["id"]})
    )
    .await
    .unwrap_err()
    .to_string()
    .contains("different workspace"));
    assert!(command(&service, "plan", "Inspect project", json!({}))
        .await
        .unwrap_err()
        .to_string()
        .contains("Trust"));
    assert!(!other.join("unexpected").exists());
    assert!(model.requests.lock().unwrap().is_empty());
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn undo_restores_the_latest_checkpoint_and_preserves_earlier_changes() {
    let (_root, service) = setup("http://127.0.0.1:9/v1");
    let session = call(&service, "POST", "/api/sessions", json!({}))
        .await
        .unwrap();
    let sid = session["id"].as_str().unwrap();
    let store = service.engine.store();
    let ws = Workspace::open(&service.workspace().unwrap()).unwrap();
    let mut ids = Vec::new();
    for path in ["first.txt", "second.txt"] {
        let id = store.create_task(sid, path).unwrap();
        shadowcode_core::checkpoint::record(
            &store,
            &ws,
            &id,
            path,
            &ws.snapshot(path).unwrap(),
            Some(b"changed"),
        )
        .unwrap();
        ws.write(path, b"changed", Some("missing")).unwrap();
        store
            .finish_task(&id, "completed", "Done", &json!({}))
            .unwrap();
        ids.push(id);
    }
    command(&service, "undo", "", json!({})).await.unwrap();
    assert!(ws.path.join("first.txt").exists());
    assert!(!ws.path.join("second.txt").exists());
    command(&service, "rollback", &ids[0], json!({}))
        .await
        .unwrap();
    assert!(!ws.path.join("first.txt").exists());
}
#[tokio::test]
async fn skill_editor_validates_before_writing_and_rejects_stale_edits() {
    let (_root, service) = setup("http://127.0.0.1:9/v1");
    let path = "/api/workspace/skills";
    assert!(call(
        &service,
        "PUT",
        path,
        json!({"name":"audit","content":"---\nmode: invalid\n---\nInspect"})
    )
    .await
    .is_err());
    assert!(!service
        .workspace()
        .unwrap()
        .join(".shadow/skills/audit.md")
        .exists());
    let content = "---\nmode: review\n---\nInspect";
    call(
        &service,
        "PUT",
        path,
        json!({"name":"audit","content":content,"expected_hash":"missing"}),
    )
    .await
    .unwrap();
    let skills = call(&service, "GET", path, Value::Null).await.unwrap();
    assert_eq!(skills["skills"][0]["raw_content"], content);
    let hash = skills["skills"][0]["hash"].as_str().unwrap();
    put(&service, ".shadow/skills/audit.md", "External change");
    assert!(call(
        &service,
        "PUT",
        path,
        json!({"name":"audit","content":"Overwrite","expected_hash":hash})
    )
    .await
    .is_err());
    assert_eq!(
        fs::read_to_string(service.workspace().unwrap().join(".shadow/skills/audit.md")).unwrap(),
        "External change"
    );
}
