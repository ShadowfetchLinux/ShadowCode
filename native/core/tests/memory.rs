mod support;
use serde_json::{json, Value};
use shadowcode_core::{
    config::Config,
    engine::StartRequest,
    paths::AppPaths,
    service::{Request, Service},
    store::Store,
};
use std::{fs, os::unix::fs::symlink, sync::Arc, time::Duration};

fn fixture() -> (tempfile::TempDir, Service, String, String) {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    fs::create_dir(&project).unwrap();
    let paths = AppPaths::isolated(&root.path().join("profile")).unwrap();
    Config::patch(&paths, json!({"trusted_workspaces":[project]})).unwrap();
    let service = Service::open(paths, Some(project.clone())).unwrap();
    let store = service.engine.store();
    let sid = store.create_session(&project, "fixture", "").unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let tid = store.create_task(&sid, "Earlier task").unwrap();
    store
        .finish_task(&tid, "completed", "Earlier result", &json!({}))
        .unwrap();
    (root, service, sid, tid)
}
async fn api(service: &Service, path: &str, body: Value) -> anyhow::Result<Value> {
    service
        .dispatch(Request {
            method: "POST".into(),
            path: path.into(),
            body,
        })
        .await
}
async fn note(service: &Service, tid: &str, text: &str) -> Value {
    api(
        service,
        "/api/memory",
        json!({"action":"append","scope":"task","task_id":tid,"note":text}),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn task_notes_preserve_legacy_files_and_survive_restart_with_atomic_edits() {
    let (root, service, _sid, tid) = fixture();
    let paths = service.engine.paths().clone();
    let legacy = paths.state.join("tasks").join(&tid).join("memory.md");
    fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    fs::write(&legacy, "Legacy note 雪\n").unwrap();
    let read = api(&service, "/api/memory", json!({"task_id":tid}))
        .await
        .unwrap();
    assert_eq!(read["task"], "Legacy note 雪\n");
    let updated = note(&service, &tid, "New note").await;
    assert!(updated["task"]
        .as_str()
        .unwrap()
        .contains("Legacy note 雪\n- New note"));
    assert_eq!(fs::read_to_string(&legacy).unwrap(), "Legacy note 雪\n");
    assert!(api(&service,"/api/memory",json!({"action":"replace","scope":"task","task_id":tid,"note":"lost edit","expected_hash":read["task_hash"]})).await.is_err());
    service.engine.shutdown().await.unwrap();
    drop(service);
    let service = Service::open(paths, Some(root.path().join("project"))).unwrap();
    let read = api(&service, "/api/memory", json!({"task_id":tid}))
        .await
        .unwrap();
    assert_eq!(read["task"], updated["task"]);
    let cleared=api(&service,"/api/memory",json!({"action":"replace","scope":"task","task_id":tid,"note":"","expected_hash":read["task_hash"]})).await.unwrap();
    assert_eq!(cleared["task"], "");
    // Empty native notes remain authoritative instead of resurrecting legacy text.
    assert_eq!(
        api(&service, "/api/memory", json!({"task_id":tid}))
            .await
            .unwrap()["task"],
        ""
    );
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn memory_respects_project_scope_permissions_live_work_and_size_limits() {
    let (root, service, _sid, tid) = fixture();
    let elsewhere = root.path().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    let store = service.engine.store();
    let other = store.create_session(&elsewhere, "fixture", "").unwrap();
    let other_tid = store
        .create_task(other["id"].as_str().unwrap(), "Private")
        .unwrap();
    for body in [
        json!({"task_id":other_tid}),
        json!({"scope":"task"}),
        json!({"task_id":"../../secret"}),
        json!({"session_id":other["id"]}),
        json!({"task_id":shadowcode_core::id()}),
    ] {
        assert!(api(&service, "/api/memory", body).await.is_err());
    }
    let result = note(&service, &tid, "Only this project").await;
    assert!(api(
        &service,
        "/api/memory",
        json!({"action":"append","scope":"task","task_id":tid,"note":"x".repeat(16001)})
    )
    .await
    .is_err());
    assert_eq!(
        api(&service, "/api/memory", json!({"task_id":tid}))
            .await
            .unwrap()["task"],
        result["task"]
    );
    let reservation = service
        .engine
        .reserve_workspace(&service.workspace().unwrap())
        .unwrap();
    assert!(api(
        &service,
        "/api/memory",
        json!({"action":"append","task_id":tid,"scope":"task","note":"while busy"})
    )
    .await
    .is_err());
    drop(reservation);
    for permission in [
        json!({"permissions":{"level":"read_only"}}),
        json!({"permissions":{"level":"workspace"},"trusted_workspaces":[]}),
    ] {
        Config::patch(service.engine.paths(), permission).unwrap();
        assert!(api(
            &service,
            "/api/memory",
            json!({"action":"append","scope":"task","task_id":tid,"note":"forbidden"})
        )
        .await
        .is_err());
        assert!(api(&service, "/api/memory", json!({"task_id":tid}))
            .await
            .is_ok());
    }
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn task_memory_continues_branches_and_exports_without_source_coupling() {
    let (_root, service, sid, tid) = fixture();
    note(
        &service,
        &tid,
        &format!(
            "Use the documented offline fixture. {} End-of-full-note.",
            "雪".repeat(2000)
        ),
    )
    .await;
    let selected = service
        .fork_selection(service.workspace().unwrap(), Some(sid.clone()))
        .unwrap();
    assert_eq!(
        api(
            &selected,
            "/api/commands/run",
            json!({"name":"memory","args":""})
        )
        .await
        .unwrap()["metadata"]["task_id"],
        tid
    );
    let branch = api(
        &service,
        &format!("/api/sessions/{sid}/branch"),
        json!({"title":"Independent"}),
    )
    .await
    .unwrap();
    let bid = branch["id"].as_str().unwrap();
    for format in ["json", "markdown"] {
        let result = service
            .dispatch(Request {
                method: "GET".into(),
                path: format!("/api/sessions/{sid}/export?format={format}"),
                body: Value::Null,
            })
            .await
            .unwrap();
        assert!(result["content"]
            .as_str()
            .unwrap()
            .contains("End-of-full-note."));
    }
    service.engine.delete_session(&sid).unwrap();
    assert!(service.engine.store().task_notes(&tid).unwrap().is_none());
    let model=support::server(|_,_|(json!({"choices":[{"message":{"role":"assistant","content":"Notes inspected."},"finish_reason":"stop"}]}),Duration::ZERO)).await;
    Config::patch(service.engine.paths(),json!({"model":{"provider":"local","name":"fixture","endpoint":model.endpoint,"context_limit":32768}})).unwrap();
    let job = service
        .engine
        .start(StartRequest {
            workspace: service.workspace().unwrap(),
            task: "Explain the recorded note.".into(),
            session_id: Some(bid.into()),
            model: None,
            mode: "plan".into(),
            queue: false,
        
            images: Vec::new(),
        })
        .await
        .unwrap();
    let done = tokio::time::timeout(Duration::from_secs(5), service.engine.wait(&job.id))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(done.status, "completed", "{done:?}");
    let requests = model.requests.lock().unwrap().clone();
    assert!(requests[0]["messages"][0]["content"]
        .as_str()
        .unwrap()
        .contains("documented offline fixture"));
    assert!(requests[0]["messages"][0]["content"]
        .as_str()
        .unwrap()
        .contains("not verified evidence"));
    drop(requests);
    let export = service
        .dispatch(Request {
            method: "GET".into(),
            path: format!("/api/sessions/{bid}/export?format=json"),
            body: Value::Null,
        })
        .await
        .unwrap();
    assert!(export["content"]
        .as_str()
        .unwrap()
        .contains("documented offline fixture"));
    assert!(export["content"]
        .as_str()
        .unwrap()
        .contains("End-of-full-note."));
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn legacy_notes_reject_symlinks_special_files_and_oversized_content() {
    let (root, service, _sid, tid) = fixture();
    let dir = service.engine.paths().state.join("tasks").join(&tid);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("memory.md");
    let secret = root.path().join("outside");
    fs::write(&secret, "private").unwrap();
    symlink(&secret, &file).unwrap();
    assert!(api(&service, "/api/memory", json!({"task_id":tid}))
        .await
        .is_err());
    fs::remove_file(&file).unwrap();
    let file_c = std::ffi::CString::new(file.as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(file_c.as_ptr(), 0o600) }, 0);
    assert!(api(&service, "/api/memory", json!({"task_id":tid}))
        .await
        .is_err());
    fs::remove_file(&file).unwrap();
    fs::write(&file, "x".repeat(16001)).unwrap();
    assert!(api(&service, "/api/memory", json!({"task_id":tid}))
        .await
        .is_err());
    assert_eq!(fs::read_to_string(&secret).unwrap(), "private");
    fs::remove_file(&file).unwrap();
    fs::remove_dir(&dir).unwrap();
    let other_dir = service.engine.paths().state.join("another-task");
    fs::create_dir(&other_dir).unwrap();
    fs::write(other_dir.join("memory.md"), "Other task's private notes").unwrap();
    symlink(&other_dir, &dir).unwrap();
    assert!(
        api(&service, "/api/memory", json!({"task_id":tid}))
            .await
            .is_err(),
        "Intermediate symlinks may not redirect one task to another task's notes"
    );
    service.engine.shutdown().await.unwrap();
}

#[test]
fn concurrent_task_appends_do_not_lose_notes_and_migration_keeps_a_backup() {
    let root = tempfile::tempdir().unwrap();
    let paths = AppPaths::isolated(root.path()).unwrap();
    let project = root.path().join("project");
    fs::create_dir(&project).unwrap();
    let store = Store::open(&paths.database()).unwrap();
    let sid = store.create_session(&project, "fixture", "").unwrap();
    let tid = store
        .create_task(sid["id"].as_str().unwrap(), "task")
        .unwrap();
    drop(store);
    rusqlite::Connection::open(paths.database())
        .unwrap()
        .execute_batch("DROP TABLE task_notes; PRAGMA user_version=23;")
        .unwrap();
    let store = Arc::new(Store::open(&paths.database()).unwrap());
    assert!(fs::read_dir(&paths.state)
        .unwrap()
        .filter_map(Result::ok)
        .any(|p| p.file_name().to_string_lossy().contains("pre-native-")));
    std::thread::scope(|scope| {
        for index in 0..24 {
            let store = store.clone();
            let tid = &tid;
            let project = &project;
            scope.spawn(move || {
                store
                    .write_task_note(project, tid, "legacy\n", &format!("note-{index}"), None)
                    .unwrap();
            });
        }
    });
    let notes = store.task_notes(&tid).unwrap().unwrap();
    assert_eq!(notes.matches("legacy").count(), 1);
    for index in 0..24 {
        assert!(notes.lines().any(|line| line == format!("- note-{index}")));
    }
    assert_eq!(
        store
            .recent_events(sid["id"].as_str().unwrap(), 100)
            .unwrap()
            .iter()
            .filter(|e| e["type"] == "memory.updated")
            .count(),
        24
    );
}
