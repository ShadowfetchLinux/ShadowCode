//! Durable, workspace-scoped preparation of at most two Git worktrees.
//! This prepares checkouts; it does not pretend to dispatch model workers.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::Mutex,
};
pub const MAX_WORKERS: usize = 2;
static LOCK: Mutex<()> = Mutex::new(());
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkItem {
    pub id: String,
    pub title: String,
    pub prompt: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkerSlot {
    pub item: WorkItem,
    pub worktree_path: PathBuf,
    pub branch: String,
    pub status: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParallelPlan {
    pub id: String,
    pub goal: String,
    pub source: PathBuf,
    pub lead_note: String,
    pub workers: Vec<WorkerSlot>,
    pub verify_status: String,
}
fn raw_git(cwd: &Path, args: &[&str]) -> Result<Output> {
    let configured = Command::new("git")
        .args([
            "config",
            "--name-only",
            "--get-regexp",
            r"^(filter|merge)\..*\.(smudge|clean|process|driver)$",
        ])
        .current_dir(cwd)
        .output()?;
    ensure!(
        configured.status.success() || configured.status.code() == Some(1),
        "Cannot inspect Git drivers"
    );
    let mut command = Command::new("git");
    command.args([
        "--no-pager",
        "--no-optional-locks",
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "core.fsmonitor=false",
        "-c",
        "commit.gpgSign=false",
    ]);
    // Never execute configured filters/drivers. Unused global filters (e.g. LFS)
    // do not prevent ordinary repositories from preparing worktrees.
    for key in String::from_utf8_lossy(&configured.stdout).lines() {
        let value = if key.ends_with(".process") {
            ""
        } else {
            "false"
        };
        command.arg("-c").arg(format!("{key}={value}"));
        if let Some((prefix, _)) = key.rsplit_once('.') {
            if prefix.starts_with("filter.") {
                command.arg("-c").arg(format!("{prefix}.required=true"));
            }
        }
    }
    command
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .current_dir(cwd)
        .output()
        .context("Git failed to start")
}
fn git(cwd: &Path, args: &[&str]) -> Result<String> {
    let out = raw_git(cwd, args)?;
    ensure!(
        out.status.success(),
        "Git operation failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}
pub fn is_git_repo(source: &Path) -> bool {
    git(source, &["rev-parse", "--is-inside-work-tree"]).is_ok_and(|v| v == "true")
}
fn plan_file(source: &Path, root: &Path) -> Result<PathBuf> {
    crate::paths::private_directory(root)?;
    let key = format!(
        "{:x}",
        Sha256::digest(source.canonicalize()?.as_os_str().as_encoded_bytes())
    );
    Ok(root.join(format!("plan-{key}.json")))
}
fn load(source: &Path, root: &Path) -> Result<Option<ParallelPlan>> {
    let path = plan_file(source, root)?;
    if !path.exists() {
        return Ok(None);
    }
    let plan: ParallelPlan = serde_json::from_slice(&fs::read(path)?)?;
    ensure!(
        plan.source == source.canonicalize()? && plan.workers.len() <= MAX_WORKERS,
        "Invalid parallel plan scope"
    );
    uuid::Uuid::parse_str(&plan.id).context("Invalid plan id")?;
    for (index, worker) in plan.workers.iter().enumerate() {
        let id = format!("w{}", index + 1);
        ensure!(
            worker.item.id == id
                && worker.branch == format!("shadowcode/parallel-{}-{id}", plan.id)
                && worker.worktree_path == root.canonicalize()?.join(format!("{}-{id}", plan.id)),
            "Invalid managed worktree record"
        );
    }
    Ok(Some(plan))
}
fn save(plan: &ParallelPlan, root: &Path) -> Result<()> {
    crate::paths::atomic_write(
        &plan_file(&plan.source, root)?,
        &serde_json::to_vec_pretty(plan)?,
        false,
    )
}
pub fn split_goal(goal: &str) -> Result<Vec<WorkItem>> {
    let goal = goal.trim();
    ensure!(
        !goal.is_empty() && goal.len() <= 64000,
        "A bounded goal is required"
    );
    let parts: Vec<_> = if goal.contains('\n') {
        goal.lines()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .collect()
    } else {
        goal.splitn(2, " and ")
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .collect()
    };
    // Preserve all requested work in the last item instead of silently dropping lines.
    let groups = if parts.len() > 2 {
        vec![parts[0].to_owned(), parts[1..].join("\n")]
    } else {
        parts.into_iter().map(str::to_owned).collect()
    };
    Ok(groups
        .into_iter()
        .enumerate()
        .map(|(i, text)| WorkItem {
            id: format!("w{}", i + 1),
            title: text.chars().take(80).collect(),
            prompt: format!("Complete this item:\n{text}\n\nShared goal:\n{goal}"),
        })
        .collect())
}
pub fn prepare(source: &Path, goal: &str, root: &Path) -> Result<Value> {
    let _guard = LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("Parallel lock poisoned"))?;
    if !is_git_repo(source) {
        return Ok(
            json!({"ok":false,"enabled":false,"error":"Open a Git repository to prepare worker worktrees.","max_workers":MAX_WORKERS}),
        );
    }
    let source = source.canonicalize()?;
    ensure!(
        PathBuf::from(git(&source, &["rev-parse", "--show-toplevel"])?).canonicalize()? == source,
        "Open the repository root to prepare worktrees"
    );
    ensure!(
        load(&source, root)?.is_none(),
        "This workspace already has a parallel plan; review or clean it up first"
    );
    let items = split_goal(goal)?;
    let head = git(&source, &["rev-parse", "HEAD"])?;
    let mut plan=ParallelPlan {id:crate::id(),goal:goal.into(),source:source.clone(),workers:vec![],verify_status:"pending".into(),lead_note:"Checkouts start from committed HEAD. Source edits stay in the lead checkout. Start tasks in the prepared worktrees explicitly.".into()};
    save(&plan, root)?;
    for item in items {
        let branch = format!("shadowcode/parallel-{}-{}", plan.id, item.id);
        let path = root
            .canonicalize()?
            .join(format!("{}-{}", plan.id, item.id));
        git(
            &source,
            &[
                "worktree",
                "add",
                "-b",
                &branch,
                path.to_str().context("Non-UTF-8 worktree path")?,
                &head,
            ],
        )?;
        plan.workers.push(WorkerSlot {
            item,
            worktree_path: path,
            branch,
            status: "ready".into(),
        });
        save(&plan, root)?;
    }
    Ok(
        json!({"ok":true,"enabled":true,"max_workers":MAX_WORKERS,"plan":plan,"note":"Worktrees prepared. No model jobs have been started."}),
    )
}
pub fn active_plan(source: &Path, root: &Path) -> Result<Option<ParallelPlan>> {
    let _guard = LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("Parallel lock poisoned"))?;
    load(source, root)
}
pub fn mark_worker_status(source: &Path, root: &Path, id: &str, status: &str) -> Result<Value> {
    ensure!(
        ["ready", "running", "finished", "failed"].contains(&status),
        "Invalid worker status"
    );
    let _guard = LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("Parallel lock poisoned"))?;
    let mut plan = load(source, root)?.context("No parallel plan for this workspace")?;
    plan.workers
        .iter_mut()
        .find(|w| w.item.id == id)
        .context("Unknown worker")?
        .status = status.into();
    plan.verify_status = "pending".into();
    save(&plan, root)?;
    Ok(json!({"ok":true,"worker":id,"status":status}))
}
fn clean(worker: &WorkerSlot) -> Result<bool> {
    Ok(git(
        &worker.worktree_path,
        &[
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--ignored=matching",
        ],
    )?
    .is_empty())
}
pub fn verify(source: &Path, root: &Path) -> Result<Value> {
    let _guard = LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("Parallel lock poisoned"))?;
    let mut plan = load(source, root)?.context("No parallel plan for this workspace")?;
    let unfinished: Vec<_> = plan
        .workers
        .iter()
        .filter(|w| w.status != "finished")
        .map(|w| w.item.id.clone())
        .collect();
    if !unfinished.is_empty() {
        return Ok(json!({"ok":false,"verify_status":"waiting","unfinished":unfinished}));
    }
    let mut combined = git(source, &["rev-parse", "HEAD"])?;
    let mut checked = Vec::new();
    let mut conflicts = Vec::new();
    for worker in &plan.workers {
        // A missing worker branch is a Git error, not a merge conflict.
        // merge-tree exits 1 for both on some Git versions, so resolve first.
        ensure!(
            raw_git(
                source,
                &[
                    "rev-parse",
                    "--verify",
                    "--quiet",
                    &format!("refs/heads/{}", worker.branch),
                ],
            )?
            .status
            .success(),
            "Worker branch {} is missing; restore it before verification",
            worker.branch
        );
        ensure!(
            clean(worker)?,
            "Commit or preserve changes in {} before verification",
            worker.item.id
        );
        let out = raw_git(
            source,
            &["merge-tree", "--write-tree", &combined, &worker.branch],
        )?;
        // Exit 1 records a genuine conflict. Any other failure (missing
        // objects, corrupt repository) is an error, not a conflict report.
        ensure!(
            out.status.success() || out.status.code() == Some(1),
            "Git merge check failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        if !out.status.success() {
            conflicts.push(json!({"worker":worker.item.id,"branch":worker.branch,
                "detail":crate::tools::truncate(&format!("{}{}",String::from_utf8_lossy(&out.stdout),String::from_utf8_lossy(&out.stderr)),4000)}));
            break;
        }
        let tree = String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()
            .context("Git returned no merge tree")?
            .to_owned();
        // A temporary commit object lets the next worker be checked against all
        // prior workers together. No branch, index or checkout is changed.
        combined = git(
            source,
            &[
                "-c",
                "user.name=ShadowCode verifier",
                "-c",
                "user.email=verifier@shadowcode.invalid",
                "commit-tree",
                &tree,
                "-p",
                &combined,
                "-p",
                &worker.branch,
                "-m",
                "Temporary worktree verification",
            ],
        )?;
        checked.push(json!({"worker":worker.item.id,"branch":worker.branch,"clean":true}));
    }
    plan.verify_status = if conflicts.is_empty() {
        "clean"
    } else {
        "conflicts"
    }
    .into();
    save(&plan, root)?;
    Ok(
        json!({"ok":conflicts.is_empty(),"verify_status":plan.verify_status,"checked":checked,"conflicts":conflicts,
        "note":"Combined merge check only. Source checkout and branches are unchanged; no worker changes were integrated."}),
    )
}
pub fn cleanup(source: &Path, root: &Path) -> Result<Value> {
    let _guard = LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("Parallel lock poisoned"))?;
    let Some(mut plan) = load(source, root)? else {
        return Ok(json!({"ok":true,"cleaned":0}));
    };
    // Preflight every checkout before removing any. Keep every branch so commits
    // cannot disappear even after a clean working tree is removed.
    for worker in &plan.workers {
        if worker.status == "removed" {
            continue;
        }
        ensure!(
            worker.status != "running",
            "Worker {} is still running",
            worker.item.id
        );
        ensure!(
            clean(worker)?,
            "Worker {} contains changes or ignored files; preserve them before cleanup",
            worker.item.id
        );
    }
    let mut removed = Vec::new();
    for index in 0..plan.workers.len() {
        let worker = plan.workers[index].clone();
        removed.push(worker.branch.clone());
        if worker.status == "removed" {
            continue;
        }
        git(
            source,
            &[
                "worktree",
                "remove",
                worker
                    .worktree_path
                    .to_str()
                    .context("Non-UTF-8 worktree path")?,
            ],
        )?;
        plan.workers[index].status = "removed".into();
        save(&plan, root)?;
    }
    fs::remove_file(plan_file(source, root)?)?;
    Ok(json!({"ok":true,"cleaned":removed.len(),"retained_branches":removed,"plan_id":plan.id}))
}
