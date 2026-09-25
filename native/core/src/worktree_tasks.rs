//! Worktree tasks ("Run in new worktree"): a new conversation whose turns
//! run in its own managed Git worktree, so it can work while another task
//! runs in the project's main checkout. The engine allows one active task per
//! checkout (folder); a worktree is a different folder, so both run at once.
//!
//! The worktree starts from the same state Compare uses: HEAD plus the
//! project's uncommitted, non-ignored work, captured without touching the
//! project's index or working tree. When the task is done the user either
//! applies its result to the project (`git apply --check` first, working
//! tree only, never the index or a commit), keeps it on its branch, or
//! discards it. The worktree is removed in every case and the conversation
//! moves back to the project, so later turns run in the main checkout.
//!
//! A worktree task's folder is never listed as a project or remembered as
//! the relaunch folder (see `Service::select_if`); its conversation is
//! listed under its project (`worktree_source`).
use crate::{
    compare::{self, Base, FileStat},
    engine::{Engine, Job},
    store::{keys, Store},
    workspace::Workspace,
    worktrees,
};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// Serialises worktree task operations in this process (load → change →
/// save), the way Compare does.
static LOCK: Mutex<()> = Mutex::const_new(());
const INDEXED: usize = 100;
const LISTED: usize = 30;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Record {
    pub id: String,
    /// The project the task belongs to (the main checkout).
    pub workspace: PathBuf,
    pub session_id: String,
    pub worktree: PathBuf,
    pub worktree_id: String,
    pub branch: String,
    pub base: Base,
    /// The first message, for lists.
    pub task: String,
    pub created_at: f64,
    pub finished_at: Option<f64>,
    /// starting | running | done | applied | branch | discarded
    pub state: String,
    /// The conversation's latest job and its status.
    pub job_id: String,
    pub status: String,
    pub changed_files: Vec<FileStat>,
    pub changed_files_truncated: bool,
    /// Files written to the project by "Apply to project".
    pub applied_files: Vec<String>,
    /// Files `git apply --check` refused on the last apply; nothing was
    /// written. Empty once an apply succeeds.
    pub conflicts: Vec<String>,
    pub conflict_detail: String,
    /// The branch holding the result, after "Keep as branch".
    pub kept_branch: Option<String>,
    /// Cleanup problems and a worktree deleted outside ShadowCode.
    pub notes: Vec<String>,
    /// The worktree was removed.
    pub removed: bool,
    /// Job id and finish time the stored diffstat belongs to (internal).
    stats_for: String,
}
impl Record {
    pub fn to_json(&self) -> Value {
        let mut value = json!(self);
        if let Some(map) = value.as_object_mut() {
            map.remove("stats_for");
        }
        value
    }
    /// Its worktree still exists and its conversation runs there.
    pub fn open(&self) -> bool {
        matches!(self.state.as_str(), "starting" | "running" | "done")
    }
}

fn active(status: &str) -> bool {
    matches!(status, "queued" | "running" | "paused" | "cancelling")
}
fn valid_id(id: &str) -> Result<()> {
    ensure!(
        id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()),
        "Unknown worktree task"
    );
    Ok(())
}
pub fn load(store: &Store, id: &str) -> Result<Record> {
    valid_id(id)?;
    let text = store
        .native_meta(&keys::worktree_task_record(id))?
        .context("Worktree task not found")?;
    Ok(serde_json::from_str(&text)?)
}
/// Store the record; a new one joins its project's index in the same
/// transaction.
fn save(store: &Store, record: &Record) -> Result<()> {
    let key = keys::worktree_task_record(&record.id);
    store.meta_transaction(|meta| {
        if meta.get(&key)?.is_none() {
            let index = keys::worktree_task_index(&record.workspace);
            let mut ids: Vec<String> = meta
                .get(&index)?
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or_default();
            ids.retain(|id| id != &record.id);
            ids.insert(0, record.id.clone());
            ids.truncate(INDEXED);
            meta.set_json(&index, &ids)?;
        }
        meta.set_json(&key, record)
    })
}
fn index(store: &Store, workspace: &Path) -> Result<Vec<String>> {
    Ok(store
        .native_meta(&keys::worktree_task_index(workspace))?
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default())
}
/// Remove a record that never started (and its index entry).
fn forget(store: &Store, record: &Record) -> Result<()> {
    store.meta_transaction(|meta| {
        let index = keys::worktree_task_index(&record.workspace);
        let mut ids: Vec<String> = meta
            .get(&index)?
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        ids.retain(|id| id != &record.id);
        meta.set_json(&index, &ids)?;
        meta.set(&keys::worktree_task_record(&record.id), "null")
    })
}

/// The open worktree task a conversation runs in, if any.
pub fn session_task(store: &Store, session_id: &str) -> Result<Option<String>> {
    store.session_meta(session_id, keys::WORKTREE_TASK)
}

/// A conversation whose worktree still exists is not deleted with it: the
/// worktree would be left behind without a way to reach it.
pub fn ensure_deletable(store: &Store, session_id: &str) -> Result<()> {
    if let Some(id) = session_task(store, session_id)? {
        if load(store, &id).is_ok_and(|record| record.open() && record.state != "starting") {
            anyhow::bail!(
                "This conversation still has its own worktree. Apply it to the project, keep it as a branch or discard it first."
            );
        }
    }
    Ok(())
}

/// Only one local model fits in GPU memory at a time: a worktree task on a
/// local model may not start while a task elsewhere uses a different one.
pub fn ensure_one_local_model(engine: &Engine, target: Option<&str>) -> Result<()> {
    let Some(target) = target.filter(|id| id.starts_with("local:gguf:")) else {
        return Ok(());
    };
    let store = engine.store();
    for job in store.job_summaries(100)? {
        if !active(job["status"].as_str().unwrap_or("")) {
            continue;
        }
        let Some(session) = job["session_id"].as_str() else {
            continue;
        };
        if let Some(other) = store.session_meta(session, keys::EXECUTION_TARGET)? {
            ensure!(
                !other.starts_with("local:gguf:") || other == target,
                "Another task is using a different local model, and only one local model fits in GPU memory at a time. Choose the same local model, a cloud or subscription model, or wait for the other task."
            );
        }
    }
    Ok(())
}

/// Copy the composer's attachments (`.shadow/attachments/…`, named in the
/// task or passed as images) into the worktree; they are usually ignored
/// files, so the snapshot does not carry them.
fn copy_attachments(source: &Path, worktree: &Path, task: &str, images: &[String]) -> Result<()> {
    const PREFIX: &str = ".shadow/attachments/";
    let mut wanted: Vec<String> = images.to_vec();
    let mut rest = task;
    while let Some(at) = rest.find(PREFIX) {
        let tail = &rest[at..];
        let end = tail
            .find(|c: char| c.is_whitespace() || c == ',')
            .unwrap_or(tail.len());
        wanted.push(tail[..end].to_owned());
        rest = &tail[end..];
    }
    for relative in wanted {
        let path = Path::new(&relative);
        if !relative.starts_with(PREFIX)
            || path
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            continue;
        }
        let from = source.join(path);
        let Ok(meta) = fs::symlink_metadata(&from) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let to = worktree.join(path);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&from, &to)
            .with_context(|| format!("Could not copy the attachment {relative}"))?;
    }
    Ok(())
}

/// Create the worktree and its conversation. The caller starts the first
/// turn there and then calls `started`, or `abandon` when it could not.
pub(crate) async fn prepare(
    engine: &Engine,
    source: &Path,
    task: &str,
    images: &[String],
    model: &str,
) -> Result<Record> {
    let cancel = CancellationToken::new();
    let source = Workspace::open(source)?.path;
    compare::repository_root_for(
        &source,
        "Running in a new worktree needs",
        "running a task in a new worktree",
        &cancel,
    )
    .await?;
    let base = compare::snapshot_for(
        &engine.paths().data,
        &source,
        "running a task in a new worktree",
        "ShadowCode worktree task base",
        &cancel,
    )
    .await?;
    let _guard = LOCK.lock().await;
    let checkout = worktrees::create(engine.paths(), &source, &base.commit, cancel.clone()).await?;
    let mut record = Record {
        id: crate::id(),
        workspace: source.clone(),
        worktree: checkout.path.clone(),
        worktree_id: checkout.id.clone(),
        branch: checkout.branch.clone(),
        base,
        task: task.chars().take(512).collect(),
        created_at: crate::now(),
        state: "starting".into(),
        status: "queued".into(),
        ..Default::default()
    };
    let prepared: Result<()> = async {
        record.worktree = checkout.path.canonicalize()?;
        compare::set_trust(engine, std::slice::from_ref(&record.worktree), true)?;
        copy_attachments(&source, &record.worktree, task, images)?;
        let store = engine.store();
        let session = store.create_session(&record.worktree, model, "")?;
        record.session_id = session["id"]
            .as_str()
            .context("Missing session ID")?
            .to_owned();
        store.set_session_meta(&record.session_id, keys::WORKTREE_TASK, &record.id)?;
        store.set_session_meta(
            &record.session_id,
            keys::WORKTREE_SOURCE,
            &source.to_string_lossy(),
        )?;
        save(&store, &record)
    }
    .await;
    if let Err(error) = prepared {
        drop(_guard);
        abandon(engine, record).await;
        return Err(error);
    }
    Ok(record)
}

/// The first turn started in the worktree.
pub(crate) async fn started(engine: &Engine, mut record: Record, job: &Job) -> Result<Record> {
    let _guard = LOCK.lock().await;
    record.job_id = job.id.clone();
    record.status = job.status.clone();
    record.state = "running".into();
    let store = engine.store();
    let saved = record.clone();
    store.run(move |store| save(store, &saved)).await?;
    Ok(record)
}

/// The first turn could not start: remove the worktree, its conversation
/// and the record, as if nothing happened.
pub(crate) async fn abandon(engine: &Engine, record: Record) {
    let _ = worktrees::dispose(
        engine.paths(),
        &record.workspace,
        &record.worktree_id,
        CancellationToken::new(),
    )
    .await;
    let _ = compare::set_trust(engine, std::slice::from_ref(&record.worktree), false);
    let store = engine.store();
    let _ = forget(&store, &record);
    if !record.session_id.is_empty() {
        let _ = engine.delete_session(&record.session_id);
    }
}

/// Bring status and changed files up to date; `running` ⇄ `done` follows the
/// conversation's latest job (a follow-up turn runs it again).
async fn refresh(engine: &Engine, record: &mut Record, cancel: &CancellationToken) -> Result<()> {
    if !record.open() || record.state == "starting" {
        return Ok(());
    }
    let store = engine.store();
    let latest = store
        .session_jobs(&record.session_id, 1)?
        .pop()
        .and_then(|job| job["id"].as_str().map(str::to_owned))
        .unwrap_or_else(|| record.job_id.clone());
    let job = match engine.job(&latest)? {
        Some(job) => job,
        None => {
            record.status = "failed".into();
            return Ok(());
        }
    };
    record.job_id = job.id.clone();
    record.status = job.status.clone();
    if active(&job.status) {
        record.state = "running".into();
        record.finished_at = None;
    } else if record.state == "running" {
        record.state = "done".into();
        record.finished_at = job.finished_at.or_else(|| Some(crate::now()));
    }
    if !record.worktree.is_dir() {
        let note = format!(
            "Its worktree {} was deleted outside ShadowCode",
            record.worktree.display()
        );
        if !record.notes.contains(&note) {
            record.notes.push(note);
        }
        return Ok(());
    }
    let key = format!("{}:{:?}", job.id, job.finished_at);
    if active(&job.status) || record.stats_for != key {
        if let Ok((files, truncated)) =
            compare::diffstat(&record.worktree, &record.base.commit, cancel).await
        {
            record.changed_files = files;
            record.changed_files_truncated = truncated;
            if !active(&job.status) {
                record.stats_for = key;
            }
        }
    }
    Ok(())
}

pub async fn get(engine: &Engine, id: &str) -> Result<Record> {
    let _guard = LOCK.lock().await;
    let store = engine.store();
    let mut record = load(&store, id)?;
    refresh(engine, &mut record, &CancellationToken::new()).await?;
    save(&store, &record)?;
    Ok(record)
}

/// The project's worktree tasks, newest first; open ones are refreshed.
pub async fn list(engine: &Engine, workspace: &Path) -> Result<Vec<Record>> {
    let workspace = Workspace::open(workspace)?.path;
    let _guard = LOCK.lock().await;
    let store = engine.store();
    let mut records = Vec::new();
    for id in index(&store, &workspace)?.iter().take(LISTED) {
        let Ok(mut record) = load(&store, id) else {
            continue;
        };
        if record.open() {
            refresh(engine, &mut record, &CancellationToken::new()).await?;
            save(&store, &record)?;
        }
        records.push(record);
    }
    Ok(records)
}

/// A worktree task that can be closed now: open, with no turn running.
async fn closable(engine: &Engine, id: &str, cancel: &CancellationToken) -> Result<Record> {
    let store = engine.store();
    let mut record = load(&store, id)?;
    ensure!(
        record.open() && record.state != "starting",
        "This worktree task was already {}",
        closed_label(&record.state)
    );
    refresh(engine, &mut record, cancel).await?;
    Ok(record)
}
fn closed_label(state: &str) -> &'static str {
    match state {
        "applied" => "applied to the project",
        "branch" => "kept as a branch",
        "discarded" => "discarded",
        _ => "closed",
    }
}

/// "Apply to project": commit the result on the worktree's own branch, then
/// apply it to the project's working tree. When `git apply --check` refuses,
/// nothing is written and the record lists the conflicting files (state
/// stays `done`); otherwise the worktree is removed and the conversation
/// returns to the project.
pub async fn apply(engine: &Engine, id: &str) -> Result<Record> {
    let cancel = CancellationToken::new();
    let _guard = LOCK.lock().await;
    let store = engine.store();
    let mut record = closable(engine, id, &cancel).await?;
    ensure!(
        !active(&record.status),
        "The task is still working. Wait for it to finish or stop it first."
    );
    ensure!(
        record.worktree.is_dir(),
        "Its worktree was deleted outside ShadowCode, so its result cannot be applied. Discard it instead."
    );
    // No task may run in the project, or in the worktree, while this runs.
    let _project = engine.reserve_workspace(&record.workspace).map_err(|error| {
        anyhow::anyhow!("{error:#}. Stop or wait for the task running in the project, then apply again.")
    })?;
    let _checkout = engine.reserve_workspace(&record.worktree)?;
    let (head, files) = compare::commit_checkout(
        &record.worktree,
        &record.base.commit,
        "ShadowCode worktree task result",
        &cancel,
    )
    .await?;
    if !files.is_empty() {
        if let Some(refused) = compare::apply_checkout(
            &engine.paths().data,
            &record.worktree,
            &record.workspace,
            &record.base.commit,
            &head,
            &cancel,
        )
        .await?
        {
            record.conflicts = refused.conflicts;
            record.conflict_detail = refused.detail;
            save(&store, &record)?;
            return Ok(record);
        }
    }
    record.conflicts.clear();
    record.conflict_detail.clear();
    record.applied_files = files;
    close(engine, &mut record, "applied").await;
    save(&store, &record)?;
    Ok(record)
}

/// "Keep as branch": commit the result on the worktree's branch
/// (`shadowcode/<id>`), remove the worktree and keep the branch.
pub async fn keep_branch(engine: &Engine, id: &str) -> Result<Record> {
    let cancel = CancellationToken::new();
    let _guard = LOCK.lock().await;
    let store = engine.store();
    let mut record = closable(engine, id, &cancel).await?;
    ensure!(
        !active(&record.status),
        "The task is still working. Wait for it to finish or stop it first."
    );
    ensure!(
        record.worktree.is_dir(),
        "Its worktree was deleted outside ShadowCode, so there is nothing to keep. Discard it instead."
    );
    let _checkout = engine.reserve_workspace(&record.worktree)?;
    let first_line = record.task.lines().next().unwrap_or("").trim();
    let message = if first_line.is_empty() {
        "ShadowCode worktree task result".to_owned()
    } else {
        format!(
            "ShadowCode: {}",
            first_line.chars().take(72).collect::<String>()
        )
    };
    compare::commit_checkout(&record.worktree, &record.base.commit, &message, &cancel).await?;
    record.kept_branch = Some(record.branch.clone());
    close(engine, &mut record, "branch").await;
    save(&store, &record)?;
    Ok(record)
}

/// "Discard": stop a running turn, remove the worktree and its branch.
pub async fn discard(engine: &Engine, id: &str) -> Result<Record> {
    let cancel = CancellationToken::new();
    let _guard = LOCK.lock().await;
    let store = engine.store();
    let mut record = load(&store, id)?;
    if !record.open() {
        if !record.removed && !record.worktree_id.is_empty() {
            // Retry the cleanup a previous attempt left behind.
            let state = record.state.clone();
            close(engine, &mut record, &state).await;
            save(&store, &record)?;
        }
        return Ok(record);
    }
    refresh(engine, &mut record, &cancel).await?;
    if active(&record.status) {
        engine.request_cancel(&record.job_id)?;
        tokio::time::timeout(Duration::from_secs(60), engine.wait(&record.job_id))
            .await
            .context("The task is still stopping; try again shortly")??;
        refresh(engine, &mut record, &cancel).await?;
    }
    close(engine, &mut record, "discarded").await;
    save(&store, &record)?;
    Ok(record)
}

/// Remove the worktree (keeping its branch for `branch`), stop trusting its
/// folder and move the conversation back to the project.
async fn close(engine: &Engine, record: &mut Record, state: &str) {
    record.state = state.into();
    record.finished_at.get_or_insert_with(crate::now);
    if !record.removed && !record.worktree_id.is_empty() {
        let removed = if state == "branch" {
            worktrees::release(
                engine.paths(),
                &record.workspace,
                &record.worktree_id,
                CancellationToken::new(),
            )
            .await
        } else {
            worktrees::dispose(
                engine.paths(),
                &record.workspace,
                &record.worktree_id,
                CancellationToken::new(),
            )
            .await
        };
        match removed {
            Ok(note) => {
                record.removed = true;
                record.notes.extend(note);
            }
            Err(error) => record.notes.push(format!(
                "The worktree {} was kept: {error:#}",
                record.worktree.display()
            )),
        }
        if let Err(error) =
            compare::set_trust(engine, std::slice::from_ref(&record.worktree), false)
        {
            record
                .notes
                .push(format!("Could not update trusted projects: {error:#}"));
        }
    }
    let store = engine.store();
    let moved = store
        .move_session(&record.session_id, &record.workspace)
        .and_then(|()| {
            store.delete_session_meta(&record.session_id, keys::WORKTREE_TASK)?;
            store.delete_session_meta(&record.session_id, keys::WORKTREE_SOURCE)
        });
    if let Err(error) = moved {
        record.notes.push(format!(
            "The conversation could not move back to the project: {error:#}"
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachments_named_in_the_task_are_copied() {
        let root = tempfile::tempdir().unwrap();
        let (source, worktree) = (root.path().join("s"), root.path().join("w"));
        fs::create_dir_all(source.join(".shadow/attachments")).unwrap();
        fs::create_dir_all(&worktree).unwrap();
        fs::write(source.join(".shadow/attachments/a-notes.txt"), "notes").unwrap();
        fs::write(source.join(".shadow/attachments/b-shot.png"), "png").unwrap();
        fs::write(source.join("secret.txt"), "no").unwrap();
        copy_attachments(
            &source,
            &worktree,
            "Fix it\n\nAttached paths: .shadow/attachments/a-notes.txt, .shadow/attachments/../../secret.txt",
            &[".shadow/attachments/b-shot.png".into()],
        )
        .unwrap();
        assert!(worktree.join(".shadow/attachments/a-notes.txt").is_file());
        assert!(worktree.join(".shadow/attachments/b-shot.png").is_file());
        assert!(!worktree.join("secret.txt").exists());
    }

    #[test]
    fn records_open_until_closed() {
        let mut record = Record {
            state: "done".into(),
            ..Default::default()
        };
        assert!(record.open());
        for state in ["applied", "branch", "discarded"] {
            record.state = state.into();
            assert!(!record.open());
        }
        assert!(record.to_json().get("stats_for").is_none());
        assert!(valid_id("../x").is_err());
    }
}
