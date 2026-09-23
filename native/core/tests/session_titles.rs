//! A conversation is named after its first request unless the user named it:
//! empty titles and the window's placeholders ("New task", and "Welcome"
//! from earlier onboarding) are replaced; a real title is kept.
use serde_json::json;
use shadowcode_core::store::Store;

#[test]
fn first_request_names_placeholder_conversations_only() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(&root.path().join("state.db")).unwrap();
    let workspace = root.path().join("project");
    for (title, expected) in [
        ("", "Create hello.txt"),
        ("New task", "Create hello.txt"),
        ("Welcome", "Create hello.txt"),
        ("Parser rewrite", "Parser rewrite"),
    ] {
        let session = store.create_session(&workspace, "local", title).unwrap();
        let sid = session["id"].as_str().unwrap();
        store
            .create_job(&json!({
                "id": shadowcode_core::id(),
                "workspace": workspace,
                "session_id": sid,
                "task_id": format!("task-{}", shadowcode_core::id()),
                "task": "Create hello.txt",
                "status": "queued",
            }))
            .unwrap();
        let saved = store.session(sid).unwrap().unwrap();
        assert_eq!(saved["title"], expected, "title {title:?}");
    }
}
