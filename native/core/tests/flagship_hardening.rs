use serde_json::json;
use shadowcode_core::{
    config::Config,
    parallel,
    paths::AppPaths,
    service::{Request, Service},
    symbol_index,
};
use std::{fs, path::Path, process::Command};
fn git(path: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgSign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .current_dir(path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}
fn init(path: &Path) {
    fs::create_dir_all(path).unwrap();
    git(path, &["init", "-q"]);
    fs::write(path.join("same.txt"), "base\n").unwrap();
    git(path, &["add", "."]);
    git(path, &["commit", "-qm", "base"]);
}
#[test]
fn combined_worker_conflicts_are_detected_and_cleanup_preserves_all_work() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    let root = temp.path().join("workers");
    init(&repo);
    let lead = git(&repo, &["rev-parse", "HEAD"]);
    fs::write(repo.join("lead-only.txt"), "uncommitted").unwrap();
    parallel::prepare(&repo, "first\nsecond\nthird", &root).unwrap();
    assert!(parallel::prepare(&repo, "another", &root).is_err());
    assert!(
        parallel::active_plan(&repo, &temp.path().join("other-profile"))
            .unwrap()
            .is_none()
    );
    let plan = parallel::active_plan(&repo, &root).unwrap().unwrap();
    assert!(plan.workers[1].item.prompt.contains("third"));
    for (index, w) in plan.workers.iter().enumerate() {
        fs::write(
            w.worktree_path.join("same.txt"),
            format!("worker {index}\n"),
        )
        .unwrap();
        git(&w.worktree_path, &["commit", "-qam", "change"]);
        parallel::mark_worker_status(&repo, &root, &w.item.id, "finished").unwrap();
    }
    let result = parallel::verify(&repo, &root).unwrap();
    assert_eq!(result["verify_status"], "conflicts");
    assert_eq!(result["conflicts"][0]["worker"], "w2");
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]), lead);
    assert_eq!(
        fs::read_to_string(repo.join("lead-only.txt")).unwrap(),
        "uncommitted"
    );
    fs::write(
        plan.workers[1].worktree_path.join("do-not-delete.txt"),
        "work",
    )
    .unwrap();
    assert!(parallel::cleanup(&repo, &root).is_err());
    assert!(plan.workers[0].worktree_path.exists());
    fs::remove_file(plan.workers[1].worktree_path.join("do-not-delete.txt")).unwrap();
    parallel::cleanup(&repo, &root).unwrap();
    assert!(parallel::active_plan(&repo, &root).unwrap().is_none());
    for w in plan.workers {
        assert!(!w.worktree_path.exists());
        assert!(git(&repo, &["show", &format!("{}:same.txt", w.branch)]).contains("worker"));
    }
}
#[test]
fn index_is_private_confined_and_tracks_deletions_and_unicode() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    fs::write(
        temp.path().join("outside.rs"),
        "pub fn external_secret() {}",
    )
    .unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(temp.path().join("outside.rs"), repo.join("escape.rs")).unwrap();
        std::os::unix::fs::symlink(temp.path(), repo.join(".shadow")).unwrap();
    }
    let long = format!("pub fn unicode({}: &str) {{}}\n", "é".repeat(150));
    fs::write(repo.join("a.rs"),format!("{long}pub fn greet_extra() {{}}\npub fn greet() {{}}\npub fn call() {{greet(); let _ = greet;}}\n")).unwrap();
    assert!(symbol_index::ensure_index(&repo, &["../outside.rs".into()], false).is_err());
    #[cfg(unix)]
    assert!(symbol_index::ensure_index(&repo, &["escape.rs".into()], false).is_err());
    let all = symbol_index::query_definitions(&repo, "", 80).unwrap();
    assert!(!all.to_string().contains("external_secret"));
    assert!(!temp.path().join("symbol-index.sqlite").exists());
    assert_eq!(
        symbol_index::query_definitions(&repo, "greet", 1).unwrap()["definitions"][0]["name"],
        "greet"
    );
    assert_eq!(
        symbol_index::callers_for(&repo, "greet", 8).unwrap()["count"],
        1
    );
    fs::remove_file(repo.join("a.rs")).unwrap();
    assert_eq!(
        symbol_index::query_definitions(&repo, "greet", 8).unwrap()["ok"],
        false
    );
}
#[tokio::test]
async fn parallel_mutations_require_trust_and_respect_busy_workers_and_readonly() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    init(&repo);
    let paths = AppPaths::isolated(&temp.path().join("profile")).unwrap();
    let service = Service::open(paths.clone(), Some(repo.clone())).unwrap();
    let call = |path: &str, body| Request {
        method: "POST".into(),
        path: path.into(),
        body,
    };
    assert!(service
        .dispatch(call("/api/parallel/prepare", json!({"goal":"test"})))
        .await
        .unwrap_err()
        .to_string()
        .contains("Trust"));
    Config::patch(&paths, json!({"trusted_workspaces":[repo]})).unwrap();
    let plan = service
        .dispatch(call("/api/parallel/prepare", json!({"goal":"test"})))
        .await
        .unwrap();
    let worker = Path::new(
        plan["plan"]["workers"][0]["worktree_path"]
            .as_str()
            .unwrap(),
    );
    let reservation = service.engine.reserve_workspace(worker).unwrap();
    assert!(service
        .dispatch(call("/api/parallel/cleanup", json!({})))
        .await
        .is_err());
    drop(reservation);
    Config::patch(&paths, json!({"permissions":{"level":"read_only"}})).unwrap();
    assert!(service
        .dispatch(call("/api/parallel/cleanup", json!({})))
        .await
        .unwrap_err()
        .to_string()
        .contains("read-only"));
    Config::patch(&paths, json!({"permissions":{"level":"workspace"}})).unwrap();
    service
        .dispatch(call("/api/parallel/cleanup", json!({})))
        .await
        .unwrap();
}
#[test]
fn residency_and_guardian_configuration_are_validated() {
    for value in ["5m", "30m", "1h", "-1", "0", "500ms"] {
        let mut cfg = Config::default();
        cfg.model.keep_alive = value.into();
        cfg.validate().unwrap();
    }
    for value in ["forever", "-2", "", "99999999999999h"] {
        let mut cfg = Config::default();
        cfg.model.keep_alive = value.into();
        assert!(cfg.validate().is_err());
    }
    let cfg = Config {
        guardian: json!({"interval_sec":1}),
        ..Default::default()
    };
    assert!(cfg.validate().is_err());
}

#[test]
fn ollama_residency_uses_numeric_sentinels_and_preserves_saved_model_values() {
    use shadowcode_core::{config::ModelConfig, model_registry, models::ModelClient};
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::isolated(temp.path()).unwrap();
    for (value, expected) in [("-1", json!(-1)), ("0", json!(0)), ("30m", json!("30m"))] {
        let model = ModelConfig {
            provider: "ollama".into(),
            name: "fixture".into(),
            endpoint: "http://localhost:11434".into(),
            keep_alive: value.into(),
            ..Default::default()
        };
        let client = ModelClient::new(model, &paths).unwrap();
        assert_eq!(client.request_body(&[], &[], 32)["keep_alive"], expected);
        let restored=model_registry::from_row(&json!({"id":"fixture","name":"fixture","provider":"ollama","endpoint":"http://localhost:11434","metadata":{"keep_alive":value}})).unwrap();
        assert_eq!(restored.keep_alive, value);
    }
}
#[test]
fn suggested_package_test_keeps_the_package_argument() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("Cargo.toml"), "").unwrap();
    assert_eq!(
        shadowcode_core::autonomy::narrow_verify_command(
            "Verify cargo test -p my-core",
            temp.path()
        )
        .as_deref(),
        Some("cargo test -p my-core")
    );
}
#[test]
fn configured_git_filter_is_never_executed_by_preparation() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    init(&repo);
    fs::write(repo.join(".gitattributes"), "same.txt filter=unsafe\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "attributes"]);
    let marker = temp.path().join("unexpected");
    let cmd = format!("touch {}; cat", marker.display());
    git(&repo, &["config", "filter.unsafe.smudge", &cmd]);
    git(&repo, &["config", "filter.unsafe.clean", &cmd]);
    assert!(parallel::prepare(&repo, "test", &temp.path().join("workers")).is_err());
    assert!(!marker.exists());
}
