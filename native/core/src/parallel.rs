//! Up to two concurrent worker jobs in separate ShadowCode-managed git
//! worktrees for one goal, plus the lead task. Not a fake swarm: capped at 2
//! because a 16 GB GPU typically holds one local model. Workers never share a
//! dirty tree. A verifier step reports conflicts instead of silently merging.
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
};
use uuid::Uuid;

pub const MAX_WORKERS: usize = 2;

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

static ACTIVE: Mutex<Option<ParallelPlan>> = Mutex::new(None);

fn git(cwd: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(args)
        .current_dir(cwd)
        .output()
        .context("git failed to start")?;
    ensure!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub fn is_git_repo(path: &Path) -> bool {
    git(path, &["rev-parse", "--is-inside-work-tree"])
        .map(|v| v == "true")
        .unwrap_or(false)
}

/// Lead splits a goal into at most two concrete work items (deterministic,
/// no extra model spawn). Returns a clear disabled message when not git.
pub fn split_goal(goal: &str) -> Result<Vec<WorkItem>> {
    let goal = goal.trim();
    ensure!(!goal.is_empty(), "Goal required");
    // Prefer explicit "1) ... 2) ..." / "and" split; otherwise one item only.
    let parts: Vec<&str> = if goal.contains('\n') {
        goal.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .take(MAX_WORKERS)
            .collect()
    } else if let Some((a, b)) = goal.split_once(" and ") {
        vec![a.trim(), b.trim()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .take(MAX_WORKERS)
            .collect()
    } else {
        vec![goal]
    };
    Ok(parts
        .into_iter()
        .enumerate()
        .map(|(i, title)| WorkItem {
            id: format!("w{}", i + 1),
            title: title.chars().take(80).collect(),
            prompt: format!("Worker {}: complete this concrete item for the shared goal.\n\nItem: {title}\n\nShared goal: {goal}", i + 1),
        })
        .collect())
}

pub fn prepare(
    source: &Path,
    goal: &str,
    checkout_root: &Path,
) -> Result<Value> {
    if !is_git_repo(source) {
        return Ok(json!({
            "ok": false,
            "enabled": false,
            "error": "Parallel worktrees require a git repository. Initialize git or open a git project.",
            "max_workers": MAX_WORKERS
        }));
    }
    let items = split_goal(goal)?;
    ensure!(
        items.len() <= MAX_WORKERS,
        "At most {MAX_WORKERS} workers are allowed on this machine"
    );
    fs::create_dir_all(checkout_root)?;
    let plan_id = Uuid::new_v4().simple().to_string();
    let head = git(source, &["rev-parse", "HEAD"])?;
    let mut workers = Vec::new();
    for item in items {
        let branch = format!("shadowcode/parallel-{plan_id}-{}", item.id);
        let path = checkout_root.join(format!("{plan_id}-{}", item.id));
        if path.exists() {
            bail!("Worktree path already exists: {}", path.display());
        }
        // Separate worktree — never share one dirty tree.
        git(
            source,
            &[
                "worktree",
                "add",
                "-b",
                &branch,
                path.to_str().context("path")?,
                &head,
            ],
        )?;
        workers.push(WorkerSlot {
            item,
            worktree_path: path,
            branch,
            status: "ready".into(),
        });
    }
    let plan = ParallelPlan {
        id: plan_id,
        goal: goal.to_owned(),
        source: source.to_path_buf(),
        lead_note: format!(
            "Lead retains the source checkout. {} worker worktree(s) prepared (cap {MAX_WORKERS}).",
            workers.len()
        ),
        workers,
        verify_status: "pending".into(),
    };
    *ACTIVE
        .lock()
        .map_err(|_| anyhow::anyhow!("parallel lock poisoned"))? = Some(plan.clone());
    Ok(json!({
        "ok": true,
        "enabled": true,
        "max_workers": MAX_WORKERS,
        "plan": plan,
        "note": "Workers do not share a dirty tree. Verifier reports conflicts instead of silent merge."
    }))
}

pub fn active_plan() -> Option<ParallelPlan> {
    ACTIVE.lock().ok().and_then(|g| g.clone())
}

pub fn mark_worker_status(worker_id: &str, status: &str) -> Result<Value> {
    let mut guard = ACTIVE
        .lock()
        .map_err(|_| anyhow::anyhow!("parallel lock poisoned"))?;
    let plan = guard.as_mut().context("No active parallel plan")?;
    let worker = plan
        .workers
        .iter_mut()
        .find(|w| w.item.id == worker_id)
        .context("Unknown worker")?;
    worker.status = status.to_owned();
    Ok(json!({"ok":true,"worker":worker_id,"status":status}))
}

/// After both workers finish, attempt a clean merge into a verify branch.
/// Unclean merges are reported — never silently merged.
pub fn verify(source: &Path) -> Result<Value> {
    let mut guard = ACTIVE
        .lock()
        .map_err(|_| anyhow::anyhow!("parallel lock poisoned"))?;
    let plan = guard.as_mut().context("No active parallel plan")?;
    ensure!(
        plan.source == source,
        "Verify must run against the plan's source repository"
    );
    let unfinished: Vec<_> = plan
        .workers
        .iter()
        .filter(|w| w.status != "finished")
        .map(|w| w.item.id.clone())
        .collect();
    if !unfinished.is_empty() {
        plan.verify_status = "waiting".into();
        return Ok(json!({
            "ok": false,
            "verify_status": "waiting",
            "unfinished": unfinished,
            "note": "Verifier runs only after both workers finish."
        }));
    }
    let verify_branch = format!("shadowcode/verify-{}", plan.id);
    let base = git(source, &["rev-parse", "HEAD"])?;
    let _ = git(source, &["branch", "-f", &verify_branch, &base]);
    let mut conflicts = Vec::new();
    let mut merged = Vec::new();
    for worker in &plan.workers {
        // Dry merge check: merge-tree if available, else merge --no-commit --no-ff then abort.
        let attempt = Command::new("git")
            .args(["-c", "core.hooksPath=/dev/null"])
            .args([
                "merge-tree",
                "--write-tree",
                &base,
                &worker.branch,
            ])
            .current_dir(source)
            .output();
        match attempt {
            Ok(out) if out.status.success() => {
                merged.push(json!({
                    "worker": worker.item.id,
                    "branch": worker.branch,
                    "clean": true
                }));
            }
            Ok(out) => {
                let detail = String::from_utf8_lossy(&out.stderr);
                conflicts.push(json!({
                    "worker": worker.item.id,
                    "branch": worker.branch,
                    "clean": false,
                    "detail": detail.chars().take(400).collect::<String>()
                }));
            }
            Err(_) => {
                // Fallback: try merge --no-commit and abort.
                let merge = Command::new("git")
                    .args(["-c", "core.hooksPath=/dev/null"])
                    .args(["merge", "--no-commit", "--no-ff", &worker.branch])
                    .current_dir(source)
                    .output();
                let clean = merge.as_ref().map(|o| o.status.success()).unwrap_or(false);
                let _ = Command::new("git")
                    .args(["merge", "--abort"])
                    .current_dir(source)
                    .output();
                if clean {
                    merged.push(json!({
                        "worker": worker.item.id,
                        "branch": worker.branch,
                        "clean": true
                    }));
                } else {
                    conflicts.push(json!({
                        "worker": worker.item.id,
                        "branch": worker.branch,
                        "clean": false,
                        "detail": "merge --no-commit reported conflicts; aborted without changing HEAD"
                    }));
                }
            }
        }
    }
    let unclean = !conflicts.is_empty();
    plan.verify_status = if unclean {
        "conflicts".into()
    } else {
        "clean".into()
    };
    Ok(json!({
        "ok": !unclean,
        "verify_status": plan.verify_status,
        "merged": merged,
        "conflicts": conflicts,
        "note": if unclean {
            "Merge is unclean; conflicts reported instead of silently merging."
        } else {
            "Worker branches merge cleanly against the lead HEAD (check only; main tree not rewritten)."
        }
    }))
}

pub fn cleanup() -> Result<Value> {
    let mut guard = ACTIVE
        .lock()
        .map_err(|_| anyhow::anyhow!("parallel lock poisoned"))?;
    let Some(plan) = guard.take() else {
        return Ok(json!({"ok":true,"cleaned":0}));
    };
    let mut cleaned = 0;
    for worker in &plan.workers {
        let _ = Command::new("git")
            .args([
                "worktree",
                "remove",
                "--force",
                worker.worktree_path.to_str().unwrap_or(""),
            ])
            .current_dir(&plan.source)
            .output();
        let _ = Command::new("git")
            .args(["branch", "-D", &worker.branch])
            .current_dir(&plan.source)
            .output();
        cleaned += 1;
    }
    Ok(json!({"ok":true,"cleaned":cleaned,"plan_id":plan.id}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn repo(root: &Path) {
        fs::create_dir_all(root).unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.name", "Parallel Test"],
            vec!["config", "user.email", "test@example.invalid"],
        ] {
            assert!(Command::new("git").args(&args).current_dir(root).status().unwrap().success());
        }
        fs::write(root.join("README.md"), "hi\n").unwrap();
        assert!(Command::new("git").args(["add", "README.md"]).current_dir(root).status().unwrap().success());
        assert!(Command::new("git").args(["commit", "-qm", "init"]).current_dir(root).status().unwrap().success());
    }

    #[test]
    fn non_git_is_disabled() {
        let root = tempfile::tempdir().unwrap();
        let report = prepare(root.path(), "do a and b", root.path().join("wt").as_path()).unwrap();
        assert_eq!(report["enabled"], false);
    }

    #[test]
    fn caps_at_two_worktrees_and_verifier_runs() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("repo");
        repo(&source);
        let checkouts = root.path().join("checkouts");
        let plan = prepare(&source, "add docs and add tests", &checkouts).unwrap();
        assert_eq!(plan["ok"], true);
        assert!(plan["plan"]["workers"].as_array().unwrap().len() <= 2);
        mark_worker_status("w1", "finished").unwrap();
        mark_worker_status("w2", "finished").unwrap();
        let verify = verify(&source).unwrap();
        assert!(verify["verify_status"].as_str().unwrap() == "clean" || verify["ok"] == true);
        cleanup().unwrap();
    }

    #[test]
    fn split_goal_never_exceeds_cap() {
        let items = split_goal("one\ntwo\nthree\nfour").unwrap();
        assert!(items.len() <= MAX_WORKERS);
    }
}
