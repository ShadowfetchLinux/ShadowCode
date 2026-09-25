//! Flagship completion smoke: redaction, steering, worktree cap, fork, sandbox,
//! and symbol tools on fixtures.
use serde_json::json;
use shadowcode_core::{
    guardian, parallel, paths::AppPaths, redaction, sandbox, steering, store::Store, symbol_index,
};
use std::{collections::BTreeMap, fs, process::Command};

#[test]
fn redaction_blocks_secret_paths_and_tokens() {
    assert!(redaction::is_secret_path(".env"));
    let r = redaction::redact_text(&format!(
        "token={}{}",
        "ghp_", "abcdefghijklmnopqrstuvwxyz012345"
    ));
    assert!(r.redacted);
    assert!(r.text.contains("[redacted secret]"));
}

#[test]
fn steering_pause_resume_hash_and_rewind_note() {
    let control = steering::SteerControl::default();
    let mut hashes = BTreeMap::new();
    hashes.insert("src/lib.rs".into(), "aaa".into());
    control.pause(hashes).unwrap();
    control
        .set_instruction("prefer Option over unwrap")
        .unwrap();
    control.note_rewind(&["src/lib.rs".into()]).unwrap();
    control.resume().unwrap();
    let mut current = BTreeMap::new();
    current.insert("src/lib.rs".into(), "bbb".into());
    let note = control.consume_resume(&current).unwrap().unwrap();
    assert!(note.contains("prefer Option"));
    assert!(note.contains("changed while paused"));
    assert!(note.contains("File checkpoint restored"));
}

#[test]
fn parallel_cap_and_non_git_disabled() {
    let root = tempfile::tempdir().unwrap();
    let disabled = parallel::prepare(root.path(), "a and b", &root.path().join("wt")).unwrap();
    assert_eq!(disabled["enabled"], false);
    let repo = root.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    assert!(Command::new("git")
        .args(["init", "-q"])
        .current_dir(&repo)
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args(["config", "user.email", "t@example.invalid"])
        .current_dir(&repo)
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args(["config", "user.name", "t"])
        .current_dir(&repo)
        .status()
        .unwrap()
        .success());
    fs::write(repo.join("f.txt"), "x\n").unwrap();
    assert!(Command::new("git")
        .args(["add", "f.txt"])
        .current_dir(&repo)
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args(["commit", "-qm", "i"])
        .current_dir(&repo)
        .status()
        .unwrap()
        .success());
    let plan = parallel::prepare(&repo, "docs and tests", &root.path().join("checkouts")).unwrap();
    assert_eq!(plan["ok"], true);
    assert!(plan["plan"]["workers"].as_array().unwrap().len() <= parallel::MAX_WORKERS);
    let _ = parallel::cleanup(&repo, &root.path().join("checkouts"));
}

#[test]
fn fork_session_keeps_original() {
    let root = tempfile::tempdir().unwrap();
    let paths = AppPaths::isolated(root.path()).unwrap();
    let store = Store::open(&paths.database()).unwrap();
    let ws = root.path().join("project");
    fs::create_dir_all(&ws).unwrap();
    let session = store.create_session(&ws, "mock", "Original").unwrap();
    let sid = session["id"].as_str().unwrap().to_owned();
    store
        .add_event("user.message", &json!({"text":"hello"}), Some(&sid), None)
        .unwrap();
    store
        .add_event("agent.message", &json!({"text":"world"}), Some(&sid), None)
        .unwrap();
    let events = store.events_after(&sid, 0, None, 100).unwrap();
    let eid = events[0]["id"].as_i64().unwrap();
    let fork = store.fork_session_from_event(&sid, eid, "Forked").unwrap();
    assert_eq!(fork["original_intact"], true);
    assert_eq!(fork["original"]["id"], sid);
    assert_ne!(fork["fork"]["id"], sid);
    let original = store.session(&sid).unwrap().unwrap();
    assert_eq!(original["title"], "Original");
}

#[test]
fn sandbox_scratch_and_hidden_home_args() {
    let base = tempfile::tempdir().unwrap();
    let scratch = sandbox::create_scratch(base.path()).unwrap();
    assert!(scratch.path.is_dir());
    let args = sandbox::profile_args(
        &base.path().join("ws"),
        "echo hi",
        false,
        Some(&scratch.path),
    );
    assert!(args
        .windows(3)
        .any(|w| w[0] == "--bind" && w[2] == "/shadowcode-scratch"));
    // Home is an empty tmpfs; only allowed toolchain folders come back.
    if std::path::Path::new("/home").is_dir() {
        assert!(args.windows(2).any(|w| w == ["--tmpfs", "/home"]));
        assert!(!args
            .windows(3)
            .any(|w| w == ["--ro-bind", "/home", "/home"]));
    }
    assert!(args.iter().any(|a| a == "--unshare-net"));
    sandbox::discard_scratch(&scratch.path).unwrap();
    let cow = sandbox::probe_workspace_cow();
    assert_eq!(cow["kernel_proof"], false);
}

#[test]
fn sandbox_fallback_is_honest_without_bubblewrap() {
    // PATH is process-global and tests in this binary share one process: run
    // the mutation in an isolated child so a concurrent test never observes an
    // empty PATH while resolving a subprocess (e.g. git in parallel::prepare).
    if std::env::var_os("SHADOWCODE_SANDBOX_FALLBACK_CHILD").is_none() {
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "sandbox_fallback_is_honest_without_bubblewrap",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("SHADOWCODE_SANDBOX_FALLBACK_CHILD", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    // Fallback: Doctor reports unavailable when bubblewrap cannot be found.
    let previous = std::env::var_os("PATH");
    std::env::set_var("PATH", "");
    let fallback = sandbox::probe_workspace_cow();
    let doctor = sandbox::doctor_checks();
    match previous {
        Some(value) => std::env::set_var("PATH", value),
        None => std::env::remove_var("PATH"),
    }
    assert_eq!(fallback["mode"], "unavailable");
    assert_eq!(fallback["shell_available"], false);
    assert_eq!(
        doctor.iter().find(|c| c["id"] == "bubblewrap").unwrap()["status"],
        "info"
    );
}

#[test]
fn symbol_index_fixture_tools() {
    let root = tempfile::tempdir().unwrap();
    let src = root.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("lib.rs"),
        "pub fn add(a: i32, b: i32) -> i32 { a + b }\npub fn use_add() { let _ = add(1, 2); }\n",
    )
    .unwrap();
    fs::write(
        src.join("math.ts"),
        "export function mul(a: number, b: number): number { return a * b; }\nexport function useMul() { return mul(2, 3); }\n",
    )
    .unwrap();
    let status = symbol_index::ensure_index(
        root.path(),
        &["src/lib.rs".into(), "src/math.ts".into()],
        false,
    )
    .unwrap();
    assert!(status["symbols_total"].as_i64().unwrap() >= 4);
    let sig = symbol_index::get_type_signature(root.path(), "add").unwrap();
    assert!(sig["ok"].as_bool().unwrap());
    let callers = symbol_index::callers_for(root.path(), "add", 8).unwrap();
    assert!(callers["count"].as_u64().unwrap() >= 1);
    let ts_defs = symbol_index::query_definitions(root.path(), "mul", 8).unwrap();
    assert!(!ts_defs["definitions"].as_array().unwrap().is_empty());
    let ts_refs = symbol_index::query_references(root.path(), "mul", 16).unwrap();
    assert!(!ts_refs["references"].as_array().unwrap().is_empty());
}

#[test]
fn guardian_default_off_and_readonly_when_enabled() {
    assert!(!guardian::GuardianConfig::default().enabled);
    let cfg = guardian::GuardianConfig {
        enabled: true,
        interval_sec: 60,
        allow_prepare_patch: false,
    };
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("README"), "x").unwrap();
    let result = guardian::Guardian::default()
        .run_health_check(&cfg, root.path())
        .unwrap();
    assert_eq!(result["wrote_main_tree"], false);
}
