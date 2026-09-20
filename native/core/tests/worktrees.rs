use anyhow::Result;
use serde_json::{json, Value};
use shadowcode_core::{
    config::Config,
    paths::AppPaths,
    service::{Request, Service},
    worktrees,
};
use std::{fs, path::Path, process::Command};
use tokio_util::sync::CancellationToken;
fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "user.name=Worktree Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn repository(root: &Path) {
    fs::create_dir(root).unwrap();
    git(root, &["init", "-q"]);
    fs::write(root.join("tracked.txt"), "committed\n").unwrap();
    git(root, &["add", "tracked.txt"]);
    git(root, &["commit", "-qm", "Base"]);
}
async fn call(service: &Service, method: &str, body: Value) -> Result<Value> {
    service
        .dispatch(Request {
            method: method.into(),
            path: "/api/worktrees".into(),
            body,
        })
        .await
}
#[tokio::test]
async fn isolated_checkout_preserves_dirty_source_and_disables_checkout_hooks() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    repository(&project);
    let paths = AppPaths::isolated(&root.path().join("profile")).unwrap();
    fs::write(project.join("tracked.txt"), "staged change\n").unwrap();
    git(&project, &["add", "tracked.txt"]);
    fs::write(project.join("tracked.txt"), "unstaged change\n").unwrap();
    fs::write(project.join("untracked.txt"), "keep me\n").unwrap();
    let before = git(&project, &["status", "--porcelain=v1"]);
    let head = git(&project, &["rev-parse", "HEAD"]);
    let staged = git(&project, &["show", ":tracked.txt"]);
    let hooks = project.join(".git/hooks");
    let hook = hooks.join("post-checkout");
    let marker = root.path().join("hook-executed");
    fs::write(&hook, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    Config::patch(&paths, json!({"trusted_workspaces":[project]})).unwrap();
    let service = Service::open(paths.clone(), Some(project.clone())).unwrap();
    let record = call(&service, "POST", json!({"reference":"HEAD"}))
        .await
        .unwrap();
    let checkout = Path::new(record["path"].as_str().unwrap());
    assert_eq!(record["state"], "ready");
    assert_eq!(record["base_commit"], head);
    assert_eq!(
        fs::read_to_string(checkout.join("tracked.txt")).unwrap(),
        "committed\n"
    );
    assert!(!checkout.join("untracked.txt").exists());
    assert!(!marker.exists());
    assert_eq!(git(&project, &["status", "--porcelain=v1"]), before);
    assert_eq!(git(&project, &["show", ":tracked.txt"]), staged);
    assert_eq!(
        fs::read_to_string(project.join("untracked.txt")).unwrap(),
        "keep me\n"
    );
    assert_eq!(
        git(checkout, &["branch", "--show-current"]),
        record["branch"].as_str().unwrap()
    );
    fs::write(checkout.join("tracked.txt"), "isolated edit\n").unwrap();
    assert_eq!(
        fs::read_to_string(project.join("tracked.txt")).unwrap(),
        "unstaged change\n"
    );
    let listed = call(&service, "GET", Value::Null).await.unwrap();
    assert_eq!(listed["worktrees"][0]["id"], record["id"]);
    assert_eq!(service.workspace().unwrap(), project);
    service.engine.shutdown().await.unwrap();
}
#[tokio::test]
async fn invalid_refs_untrusted_read_only_and_nested_projects_do_not_create_worktrees() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    repository(&project);
    let paths = AppPaths::isolated(&root.path().join("profile")).unwrap();
    let service = Service::open(paths.clone(), Some(project.clone())).unwrap();
    assert!(call(&service, "POST", json!({}))
        .await
        .unwrap_err()
        .to_string()
        .contains("Trust"));
    Config::patch(
        &paths,
        json!({"trusted_workspaces":[project],"permissions":{"level":"read_only"}}),
    )
    .unwrap();
    assert!(call(&service, "POST", json!({}))
        .await
        .unwrap_err()
        .to_string()
        .contains("read-only"));
    Config::patch(&paths, json!({"permissions":{"level":"workspace"}})).unwrap();
    for reference in ["missing-ref", "--help", "HEAD\nHEAD", ""] {
        assert!(call(&service, "POST", json!({"reference":reference}))
            .await
            .is_err());
    }
    let child = project.join("child");
    fs::create_dir(&child).unwrap();
    assert!(
        worktrees::create(&paths, &child, "HEAD", CancellationToken::new())
            .await
            .unwrap_err()
            .to_string()
            .contains("repository root")
    );
    assert!(worktrees::list(&paths, &project).unwrap().is_empty());
    assert_eq!(
        git(&project, &["worktree", "list", "--porcelain"])
            .matches("worktree ")
            .count(),
        1
    );
    let _reservation = service.engine.reserve_workspace(&project).unwrap();
    assert!(call(&service, "POST", json!({})).await.is_err());
    drop(_reservation);
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn cancelled_checkout_leaves_a_recovery_record_and_source_intact() {
    use std::time::Duration;
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    repository(&project);
    fs::write(project.join(".gitattributes"), "tracked.txt filter=slow\n").unwrap();
    git(&project, &["add", ".gitattributes"]);
    git(&project, &["commit", "-qm", "Checkout fixture"]);
    let marker = root.path().join("filter-started");
    git(
        &project,
        &[
            "config",
            "filter.slow.smudge",
            &format!("touch '{}'; sleep 60; cat", marker.display()),
        ],
    );
    let paths = AppPaths::isolated(&root.path().join("profile")).unwrap();
    let cancel = CancellationToken::new();
    let worker_paths = paths.clone();
    let worker_project = project.clone();
    let worker_cancel = cancel.clone();
    let worker = tokio::spawn(async move {
        worktrees::create(&worker_paths, &worker_project, "HEAD", worker_cancel).await
    });
    tokio::time::timeout(Duration::from_secs(10), async {
        while !marker.exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    cancel.cancel();
    let failure = tokio::time::timeout(Duration::from_secs(5), worker)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert!(failure.to_string().contains("Recovery record"));
    let records = worktrees::list(&paths, &project).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].state, "needs_attention");
    assert_eq!(
        fs::read_to_string(project.join("tracked.txt")).unwrap(),
        "committed\n"
    );
    assert_eq!(git(&project, &["status", "--porcelain=v1"]), "");
}

async fn operation(service: &Service, path: &str, body: Value) -> Result<Value> {
    service
        .dispatch(Request {
            method: "POST".into(),
            path: path.into(),
            body,
        })
        .await
}
#[tokio::test]
async fn removal_refuses_changes_ignored_files_stale_review_and_busy_tasks_preserves_commits() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    repository(&project);
    fs::write(project.join(".gitignore"), "ignored.txt\n").unwrap();
    git(&project, &["add", ".gitignore"]);
    git(&project, &["commit", "-qm", "Ignore fixture"]);
    let paths = AppPaths::isolated(&root.path().join("profile")).unwrap();
    Config::patch(&paths, json!({"trusted_workspaces":[project]})).unwrap();
    let service = Service::open(paths.clone(), Some(project.clone())).unwrap();
    let created = call(&service, "POST", json!({})).await.unwrap();
    let id = created["id"].as_str().unwrap();
    let checkout = Path::new(created["path"].as_str().unwrap());
    git(
        &project,
        &[
            "worktree",
            "lock",
            "--reason",
            "Keep this checkout",
            checkout.to_str().unwrap(),
        ],
    );
    let locked = operation(&service, "/api/worktrees/inspect", json!({"id":id}))
        .await
        .unwrap();
    assert_eq!(locked["can_remove"], false);
    assert!(locked["reason"].as_str().unwrap().contains("locked"));
    git(
        &project,
        &["worktree", "unlock", checkout.to_str().unwrap()],
    );
    let initial = operation(&service, "/api/worktrees/inspect", json!({"id":id}))
        .await
        .unwrap();
    assert_eq!(initial["can_remove"], true);
    for file in ["tracked.txt", "untracked.txt", "ignored.txt"] {
        fs::write(checkout.join(file), "local data\n").unwrap();
        let view = operation(&service, "/api/worktrees/inspect", json!({"id":id}))
            .await
            .unwrap();
        assert_eq!(view["can_remove"], false);
        assert!(operation(
            &service,
            "/api/worktrees/remove",
            json!({"id":id,"hash":view["hash"]})
        )
        .await
        .is_err());
        assert_eq!(
            fs::read_to_string(checkout.join(file)).unwrap(),
            "local data\n"
        );
        if file == "tracked.txt" {
            git(checkout, &["restore", "tracked.txt"]);
        } else {
            fs::remove_file(checkout.join(file)).unwrap();
        }
    }
    fs::write(checkout.join("tracked.txt"), "isolated commit\n").unwrap();
    git(checkout, &["add", "tracked.txt"]);
    git(checkout, &["commit", "-qm", "Keep isolated commit"]);
    let commit = git(checkout, &["rev-parse", "HEAD"]);
    assert!(operation(
        &service,
        "/api/worktrees/remove",
        json!({"id":id,"hash":initial["hash"]})
    )
    .await
    .unwrap_err()
    .to_string()
    .contains("changed"));
    let latest = operation(&service, "/api/worktrees/inspect", json!({"id":id}))
        .await
        .unwrap();
    let reservation = service.engine.reserve_workspace(checkout).unwrap();
    assert!(operation(
        &service,
        "/api/worktrees/remove",
        json!({"id":id,"hash":latest["hash"]})
    )
    .await
    .is_err());
    drop(reservation);
    Config::patch(&paths, json!({"trusted_workspaces":[project,checkout]})).unwrap();
    let config = Config::load(&paths, Some(checkout)).unwrap();
    let process = service
        .engine
        .background()
        .start(checkout, &config, None, "worktree-server", "sleep 60")
        .unwrap();
    assert!(operation(
        &service,
        "/api/worktrees/remove",
        json!({"id":id,"hash":latest["hash"]})
    )
    .await
    .unwrap_err()
    .to_string()
    .contains("background"));
    service.engine.background().stop(&process.id).await.unwrap();
    let removed = operation(
        &service,
        "/api/worktrees/remove",
        json!({"id":id,"hash":latest["hash"]}),
    )
    .await
    .unwrap();
    assert_eq!(removed["state"], "removed");
    assert!(!checkout.exists());
    assert_eq!(
        git(
            &project,
            &["rev-parse", created["branch"].as_str().unwrap()]
        ),
        commit
    );
    assert_eq!(
        fs::read_to_string(project.join("tracked.txt")).unwrap(),
        "committed\n"
    );
    assert!(worktrees::list(&paths, &project).unwrap().is_empty());
    assert!(paths
        .data
        .join("managed-worktrees/records/archive")
        .join(format!("{id}.json"))
        .exists());
    service.engine.shutdown().await.unwrap();
}
#[tokio::test]
async fn worktree_inspection_rejects_detached_heads_symlinks_and_forged_paths() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    repository(&project);
    let paths = AppPaths::isolated(&root.path().join("profile")).unwrap();
    let record = worktrees::create(&paths, &project, "HEAD", CancellationToken::new())
        .await
        .unwrap();
    git(&record.path, &["checkout", "--detach", "-q"]);
    let detached = worktrees::inspect(&paths, &project, &record.id, CancellationToken::new())
        .await
        .unwrap();
    assert!(!detached.can_remove);
    assert!(detached.reason.contains("Detached"));
    let moved = record.path.with_extension("saved");
    fs::rename(&record.path, &moved).unwrap();
    symlink(&project, &record.path).unwrap();
    assert!(
        worktrees::inspect(&paths, &project, &record.id, CancellationToken::new())
            .await
            .is_err()
    );
    fs::remove_file(&record.path).unwrap();
    fs::rename(moved, &record.path).unwrap();
    let mut forged = record.clone();
    forged.path = project.clone();
    let file = paths
        .data
        .join("managed-worktrees/records")
        .join(format!("{}.json", record.id));
    fs::write(file, serde_json::to_vec(&forged).unwrap()).unwrap();
    assert!(
        worktrees::inspect(&paths, &project, &record.id, CancellationToken::new())
            .await
            .unwrap_err()
            .to_string()
            .contains("identity changed")
    );
    assert!(project.join("tracked.txt").exists());
}
