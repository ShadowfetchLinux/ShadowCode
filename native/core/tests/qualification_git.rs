//! Real disposable Git worktrees: dirty, staged, untracked, rename, binary,
//! conflict, detached HEAD, linked worktree, unicode, and metacharacters.
//! Destructive tools must not casually erase pre-existing work.
use serde_json::json;
use shadowcode_core::{
    approvals::ApprovalHub,
    config::{Config, PermissionLevel},
    events::TaskEvents,
    models::ToolCall,
    store::Store,
    tools::ToolExecutor,
    workspace::Workspace,
};
use std::{fs, process::Command, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

fn git(cwd: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .args(["-c", "user.name=Qual", "-c", "user.email=qual@example.test"])
        .args(args)
        .current_dir(cwd)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

fn fixture(level: PermissionLevel) -> (tempfile::TempDir, ToolExecutor) {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    fs::create_dir(&project).unwrap();
    git(&project, &["init", "-b", "main"]);
    git(&project, &["config", "user.name", "Qual"]);
    git(&project, &["config", "user.email", "qual@example.test"]);
    fs::write(project.join("keep-me.txt"), "pre-existing committed\n").unwrap();
    git(&project, &["add", "keep-me.txt"]);
    git(&project, &["commit", "-m", "base"]);
    fs::write(project.join("dirty.txt"), "dirty working tree\n").unwrap();
    fs::write(project.join("staged.txt"), "will be staged\n").unwrap();
    git(&project, &["add", "staged.txt"]);
    fs::write(project.join("untracked.txt"), "untracked survivor\n").unwrap();
    fs::write(project.join("file with spaces.txt"), "spaces\n").unwrap();
    fs::write(project.join("雪.txt"), "unicode\n").unwrap();
    fs::write(project.join("--leading-dash.txt"), "dash\n").unwrap();
    fs::write(project.join("blob.bin"), [0u8, 1, 2, 255]).unwrap();
    fs::write(project.join("rename-src.txt"), "renamed body\n").unwrap();
    git(&project, &["add", "rename-src.txt"]);
    git(&project, &["commit", "-m", "rename source"]);
    git(&project, &["mv", "rename-src.txt", "rename-dest.txt"]);
    let store = Arc::new(Store::open(&root.path().join("db")).unwrap());
    let session_id = store.create_session(&project, "mock", "").unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let task_id = store.create_task(&session_id, "git-qual").unwrap();
    let (sender, _) = tokio::sync::broadcast::channel(32);
    let mut config = Config::default();
    config.permissions.level = level;
    config.permissions.approve_shell = true;
    let tools = ToolExecutor::new(
        Arc::new(Workspace::open(&project).unwrap()),
        config,
        ApprovalHub::default(),
        TaskEvents {
            store,
            session_id,
            task_id,
            sender,
        },
        CancellationToken::new(),
    )
    .unwrap();
    (root, tools)
}

async fn call(tools: &ToolExecutor, name: &str) -> shadowcode_core::tools::ToolResult {
    tools
        .execute(ToolCall {
            id: shadowcode_core::id(),
            name: name.into(),
            arguments: json!({}),
        })
        .await
        .unwrap()
}

fn survivors(project: &std::path::Path) {
    for name in [
        "keep-me.txt",
        "dirty.txt",
        "staged.txt",
        "untracked.txt",
        "file with spaces.txt",
        "雪.txt",
        "--leading-dash.txt",
        "blob.bin",
        "rename-dest.txt",
    ] {
        assert!(
            project.join(name).exists(),
            "{name} must survive denied destructive Git"
        );
    }
}

#[tokio::test]
async fn workspace_level_denies_git_clean_and_reset_without_destroying_work() {
    let (root, tools) = fixture(PermissionLevel::Workspace);
    let project = tools.workspace.path.clone();
    let clean = call(&tools, "git_clean").await;
    let reset = call(&tools, "git_reset").await;
    assert!(!clean.success, "{}", clean.error);
    assert!(!reset.success, "{}", reset.error);
    assert!(
        clean.error.contains("elevated") || clean.error.contains("Destructive"),
        "{}",
        clean.error
    );
    survivors(&project);
    let safety = shadowcode_core::autonomy::parse_git_status(
        "## main",
        " M dirty.txt\nA  staged.txt\n?? untracked.txt\n?? --leading-dash.txt\nR  rename-src.txt -> rename-dest.txt\n?? blob.bin\n",
    );
    assert!(safety.dirty && safety.staged && safety.untracked && safety.binary_or_huge);
    assert!(!safety.unusual_names.is_empty());
    assert!(safety.checkpoint_required);
    drop(root);
}

#[tokio::test]
async fn elevated_denied_approval_does_not_run_git_clean() {
    let (root, tools) = fixture(PermissionLevel::Elevated);
    let project = tools.workspace.path.clone();
    let worker = tools.clone();
    let task = tokio::spawn(async move { call(&worker, "git_clean").await });
    let record = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Some(approval) = tools.approvals.list(None).pop() {
                break approval;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("git_clean must ask");
    tools
        .approvals
        .decide(&record.id, &tools.events.session_id, false)
        .unwrap();
    let result = task.await.unwrap();
    assert!(!result.success);
    assert_ne!(
        shadowcode_core::autonomy::tool_status(result.success, &result.output, &result.error),
        shadowcode_core::autonomy::ToolStatus::Success
    );
    survivors(&project);
    drop(root);
}

#[tokio::test]
async fn detached_conflict_and_worktree_are_not_auto_recovered() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    fs::create_dir(&project).unwrap();
    git(&project, &["init", "-b", "main"]);
    git(&project, &["config", "user.name", "Qual"]);
    git(&project, &["config", "user.email", "qual@example.test"]);
    fs::write(project.join("a.txt"), "one\n").unwrap();
    git(&project, &["add", "a.txt"]);
    git(&project, &["commit", "-m", "one"]);
    fs::write(project.join("a.txt"), "two\n").unwrap();
    git(&project, &["add", "a.txt"]);
    git(&project, &["commit", "-m", "two"]);
    let sha = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "HEAD~1"])
            .current_dir(&project)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    git(&project, &["checkout", sha.trim()]);
    let detached = shadowcode_core::autonomy::parse_git_status("## HEAD (no branch)", "");
    assert!(detached.detached);
    assert_eq!(detached.risk, "detached");
    git(&project, &["checkout", "-B", "side"]);
    fs::write(project.join("a.txt"), "side\n").unwrap();
    git(&project, &["add", "a.txt"]);
    git(&project, &["commit", "-m", "side"]);
    git(&project, &["checkout", "main"]);
    let linked = root.path().join("linked");
    git(
        &project,
        &[
            "worktree",
            "add",
            "--detach",
            linked.to_str().unwrap(),
            "HEAD",
        ],
    );
    let merge = Command::new("git")
        .args(["merge", "side"])
        .current_dir(&project)
        .status()
        .unwrap();
    assert!(!merge.success(), "expected a conflict");
    let conflict = shadowcode_core::autonomy::parse_git_status("## main", "UU a.txt\n");
    assert!(conflict.conflicted);
    assert_eq!(conflict.risk, "conflict");
    assert_eq!(
        shadowcode_core::autonomy::worktree_recovery_advice("worktree path was moved")
            ["guess_paths"],
        false
    );
    assert!(linked.join("a.txt").exists(), "linked worktree must remain");
    drop(root);
}
