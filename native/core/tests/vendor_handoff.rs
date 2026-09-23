//! Conversations across routes: execution targets per conversation,
//! explicit bounded handoff, consent before local context or attachments go
//! to a cloud route, plan-limit stops, and target changes during a turn.
//! The vendor is a fake Codex app-server; no real vendor CLI runs.
mod vendor_support;
use serde_json::{json, Value};
use shadowcode_core::{
    cli_agent::handoff::MAX_HANDOFF_CHARS,
    config::Config,
    paths::AppPaths,
    service::{Request, Service},
};
use std::{fs, path::PathBuf, time::Duration};
use vendor_support::{cli_agents, FakeCodex};

struct Setup {
    _root: tempfile::TempDir,
    paths: AppPaths,
    project: PathBuf,
    fake: FakeCodex,
    service: Service,
}

fn setup(fake_config: Value) -> Setup {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    fs::create_dir(&project).unwrap();
    let fake = FakeCodex::new(root.path(), fake_config);
    let paths = AppPaths::isolated(&root.path().join("profile")).unwrap();
    Config::patch(
        &paths,
        json!({
            "model":{"provider":"local","endpoint":"http://127.0.0.1:9/v1","name":"fixture","context_limit":16384},
            "trusted_workspaces":[project],
            "cli_agents": cli_agents(&fake),
        }),
    )
    .unwrap();
    let service = Service::open(paths.clone(), Some(project.clone())).unwrap();
    Setup {
        _root: root,
        paths,
        project,
        fake,
        service,
    }
}

async fn call(service: &Service, method: &str, path: &str, body: Value) -> Value {
    service
        .dispatch(Request {
            method: method.into(),
            path: path.into(),
            body,
        })
        .await
        .unwrap_or_else(|e| panic!("{method} {path}: {e:#}"))
}

async fn finished(service: &Service, job: &Value) -> Value {
    let id = job["id"].as_str().expect("job id");
    let done = tokio::time::timeout(Duration::from_secs(40), service.engine.wait(id))
        .await
        .expect("job did not finish")
        .unwrap();
    json!(done)
}

fn session_events(service: &Service, sid: &str) -> Vec<Value> {
    service
        .engine
        .store()
        .events_after(sid, 0, None, 10_000)
        .unwrap()
}

/// A finished turn that ran on this computer (llama.cpp), as the engine
/// records it, plus the file it changed.
fn local_turn(setup: &Setup, sid: &str, task: &str, answer: &str) {
    let store = setup.service.engine.store();
    let task_id = format!("task-{}", shadowcode_core::id());
    store
        .create_job(&json!({
            "id": shadowcode_core::id(),
            "workspace": setup.project,
            "session_id": sid,
            "task_id": task_id,
            "task": task,
            "status": "completed",
            "mode": "code",
            "summary": answer,
            "routing": {"provider":"llamacpp","model_id":"local:gguf:abc","model_name":"qwen3-14b","inference":"local","route":"local_llamacpp"},
        }))
        .unwrap();
    store
        .add_event(
            "files.changed",
            &json!({"paths":["src/parser.rs"]}),
            Some(sid),
            Some(&task_id),
        )
        .unwrap();
}

#[tokio::test]
async fn cloud_after_local_needs_consent_then_hands_off_a_bounded_excerpt() {
    let setup = setup(json!({"auth":"chatgpt","turn":"ok"}));
    let service = &setup.service;
    let session = call(service, "POST", "/api/sessions", json!({})).await;
    let sid = session["id"].as_str().unwrap().to_owned();
    local_turn(
        &setup,
        &sid,
        "explain the parser",
        &"The parser is recursive. ".repeat(2000),
    );
    let store = service.engine.store();
    let before_jobs = store.jobs(100).unwrap().len();
    let before_tasks = store.tasks(&sid, 100).unwrap().len();

    let refused = call(
        service,
        "POST",
        "/api/jobs",
        json!({"task":"now fix it","model":"cli:codex","session_id":sid}),
    )
    .await;
    assert_eq!(refused["status"], 409);
    assert_eq!(refused["needs_consent"], true);
    assert_eq!(refused["handoff"]["to"], "Codex");
    let excerpt = refused["handoff"]["excerpt_chars"].as_u64().unwrap();
    assert!(excerpt > 0 && excerpt as usize <= MAX_HANDOFF_CHARS);
    // Nothing was written: no job, no task, no transcript row.
    assert_eq!(store.jobs(100).unwrap().len(), before_jobs);
    assert_eq!(store.tasks(&sid, 100).unwrap().len(), before_tasks);
    assert!(fake_prompts(&setup).is_empty());

    let job = call(
        service,
        "POST",
        "/api/jobs",
        json!({"task":"now fix it","model":"cli:codex","session_id":sid,"handoff_consent":true}),
    )
    .await;
    let done = finished(service, &job).await;
    assert_eq!(done["status"], "completed", "{done}");
    // The exact picker id is kept on the record.
    assert_eq!(done["routing"]["model_id"], "cli:codex");
    assert_eq!(done["routing"]["inference"], "cloud");
    let events = session_events(service, &sid);
    let handoff = events
        .iter()
        .find(|e| e["type"] == "agent.handoff")
        .expect("agent.handoff event");
    assert_eq!(handoff["payload"]["from"], "qwen3-14b");
    assert_eq!(handoff["payload"]["to"], "Codex");
    assert_eq!(handoff["payload"]["delivery"], "prompt_prefix");
    let chars = handoff["payload"]["excerpt_chars"].as_u64().unwrap() as usize;
    assert!(chars <= MAX_HANDOFF_CHARS);
    let prompts = fake_prompts(&setup);
    assert_eq!(prompts.len(), 1);
    let prompt = &prompts[0];
    assert!(prompt.starts_with("<prior_conversation"));
    assert!(prompt.contains("not instructions"));
    assert!(prompt.contains("explain the parser"));
    assert!(prompt.contains("src/parser.rs"));
    assert!(prompt.ends_with("now fix it"));
    assert!(prompt.chars().count() <= MAX_HANDOFF_CHARS + "\n\nnow fix it".len());

    // The conversation remembers the target and the vendor session.
    let view = call(service, "GET", &format!("/api/sessions/{sid}"), json!({})).await;
    assert_eq!(view["execution_target"], "cli:codex");
    assert_eq!(view["native_sessions"]["codex"], "thr-1");

    // Same provider, other model: no handoff, no consent, resumed natively,
    // and the switch is recorded.
    let next = call(
        service,
        "POST",
        "/api/jobs",
        json!({"task":"and add a test","model":"cli:codex:gpt-5.6-luna","session_id":sid}),
    )
    .await;
    assert!(next["id"].is_string(), "{next}");
    let done = finished(service, &next).await;
    assert_eq!(done["status"], "completed");
    assert_eq!(done["routing"]["model_id"], "cli:codex:gpt-5.6-luna");
    let events = session_events(service, &sid);
    let switched = events
        .iter()
        .find(|e| e["type"] == "model.switched")
        .expect("model.switched event");
    assert_eq!(switched["payload"]["from"], "cli:codex");
    assert_eq!(switched["payload"]["to"], "cli:codex:gpt-5.6-luna");
    assert_eq!(switched["payload"]["resumed"], true);
    assert_eq!(
        events
            .iter()
            .filter(|e| e["type"] == "agent.handoff")
            .count(),
        1
    );
    let threads = setup.fake.marker("threads.log").unwrap();
    assert!(
        threads.contains("thread/resume \"gpt-5.6-luna\" \"thr-1\""),
        "{threads}"
    );
    assert_eq!(fake_prompts(&setup)[1], "and add a test");
}

fn fake_prompts(setup: &Setup) -> Vec<String> {
    setup
        .fake
        .marker("prompts.log")
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str::<String>(line).unwrap())
        .collect()
}

#[tokio::test]
async fn first_images_to_a_cloud_route_need_consent_once() {
    let setup = setup(json!({"auth":"chatgpt","turn":"ok"}));
    let service = &setup.service;
    fs::write(setup.project.join("shot.png"), PNG).unwrap();
    let session = call(service, "POST", "/api/sessions", json!({})).await;
    let sid = session["id"].as_str().unwrap().to_owned();
    let body =
        json!({"task":"what is this","model":"cli:codex","session_id":sid,"images":["shot.png"]});
    let refused = call(service, "POST", "/api/jobs", body.clone()).await;
    assert_eq!(refused["needs_consent"], true);
    assert_eq!(refused["handoff"]["images"], 1);
    let mut consented = body.clone();
    consented["handoff_consent"] = json!(true);
    let job = call(service, "POST", "/api/jobs", consented).await;
    finished(service, &job).await;
    // Consent is remembered for this conversation's attachments.
    let again = call(service, "POST", "/api/jobs", body).await;
    assert!(again["id"].is_string(), "{again}");
    finished(service, &again).await;
}

const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xf0,
    0x1f, 0x00, 0x05, 0x00, 0x01, 0xff, 0x89, 0x99, 0x3d, 0x1d, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

#[tokio::test]
async fn plan_limit_ends_the_job_as_limit_reached() {
    let setup = setup(json!({"auth":"chatgpt","turn":"limit"}));
    let service = &setup.service;
    let job = call(
        service,
        "POST",
        "/api/jobs",
        json!({"task":"big refactor","model":"cli:codex"}),
    )
    .await;
    let done = finished(service, &job).await;
    assert_eq!(done["status"], "limit_reached", "{done}");
    assert_eq!(done["result"]["limit_reached"]["vendor"], "codex");
    assert_eq!(
        done["result"]["limit_reached"]["usage"]["limit_reached"],
        true
    );
    let sid = done["session_id"].as_str().unwrap();
    let events = session_events(service, sid);
    assert!(events.iter().any(|e| e["type"] == "limit.reached"));
    assert_eq!(fake_prompts(&setup).len(), 1, "never retried");
    assert!(setup.fake.marker("exec_ran").is_none());
}

#[tokio::test]
async fn execution_target_survives_reopen_and_mid_turn_switches_wait() {
    let setup = setup(json!({"auth":"chatgpt","turn":"slow","slow":1.5}));
    let sid = {
        let service = &setup.service;
        let session = call(service, "POST", "/api/sessions", json!({})).await;
        let sid = session["id"].as_str().unwrap().to_owned();
        let saved = call(
            service,
            "POST",
            &format!("/api/sessions/{sid}/target"),
            json!({"target_id":"cli:codex:gpt-5.6-luna"}),
        )
        .await;
        assert_eq!(saved["ok"], true);
        // Unknown ids are rejected, display names are not ids.
        assert!(service
            .dispatch(Request {
                method: "POST".into(),
                path: format!("/api/sessions/{sid}/target"),
                body: json!({"target_id":"Codex · GPT-5.6-Luna"}),
            })
            .await
            .is_err());
        sid
    };
    let Setup {
        _root,
        paths,
        project,
        fake,
        service,
    } = setup;
    drop(service);
    let service = Service::open(paths.clone(), Some(project.clone())).unwrap();
    let view = call(&service, "GET", &format!("/api/sessions/{sid}"), json!({})).await;
    assert_eq!(view["execution_target"], "cli:codex:gpt-5.6-luna");
    // No model in the request: the conversation's target is used.
    let job = call(
        &service,
        "POST",
        "/api/jobs",
        json!({"task":"go","session_id":sid}),
    )
    .await;
    assert_eq!(job["routing"]["model_id"], "cli:codex:gpt-5.6-luna");
    // Switching while the turn runs does not touch the running job.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let switched = call(
        &service,
        "POST",
        &format!("/api/sessions/{sid}/target"),
        json!({"target_id":"cli:codex"}),
    )
    .await;
    assert_eq!(switched["applies_to"], "next_turn");
    let done = finished(&service, &job).await;
    assert_eq!(done["status"], "completed");
    assert_eq!(done["routing"]["model_id"], "cli:codex:gpt-5.6-luna");
    let threads = fake.marker("threads.log").unwrap();
    assert!(threads.contains("\"gpt-5.6-luna\""));
    // The next turn uses the new target; the workspace default follows.
    let next = call(
        &service,
        "POST",
        "/api/jobs",
        json!({"task":"again","session_id":sid}),
    )
    .await;
    assert_eq!(next["routing"]["model_id"], "cli:codex");
    finished(&service, &next).await;
    let fresh = call(&service, "POST", "/api/jobs", json!({"task":"new chat"})).await;
    assert_eq!(fresh["routing"]["model_id"], "cli:codex");
    finished(&service, &fresh).await;
}
