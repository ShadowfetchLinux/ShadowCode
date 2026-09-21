//! Human notes, distinct from model reasoning and recorded task results.
use crate::{paths::AppPaths, store::Store, tools::truncate, workspace::Workspace};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::path::Path;

pub const LIMIT: usize = 16_000;
pub fn replace(old: &str, note: &str, expected_hash: Option<&str>) -> Result<String> {
    ensure!(
        expected_hash == Some(crate::workspace::hash(old.as_bytes()).as_str()),
        "Notes changed or their expected hash is missing; read them again before replacing"
    );
    ensure!(
        note.len() <= LIMIT && !note.contains('\0'),
        "Notes must fit within 16 KB without NUL"
    );
    Ok(note.to_owned())
}
pub fn append(old: &str, note: &str) -> Result<String> {
    ensure!(
        !note.trim().is_empty() && !note.contains('\0'),
        "A nonempty note without NUL is required"
    );
    let next = format!(
        "{old}{}- {}\n",
        if old.is_empty() || old.ends_with('\n') {
            ""
        } else {
            "\n"
        },
        note.trim()
    );
    ensure!(
        next.len() <= LIMIT,
        "Notes exceed 16 KB; shorten the existing notes before appending"
    );
    Ok(next)
}
pub fn task_record(store: &Store, workspace: &Path, id: &str) -> Result<Value> {
    ensure!(
        uuid::Uuid::parse_str(id).is_ok(),
        "Choose an exact task UUID"
    );
    let task = store.task(id)?.context("Task does not exist")?;
    let session = store
        .session(
            task["session_id"]
                .as_str()
                .context("Task has no conversation")?,
        )?
        .context("Conversation does not exist")?;
    ensure!(
        session["workspace"].as_str() == workspace.to_str(),
        "Task does not belong to this project"
    );
    Ok(task)
}
pub fn task_notes(paths: &AppPaths, store: &Store, workspace: &Path, id: &str) -> Result<String> {
    task_record(store, workspace, id)?;
    if let Some(notes) = store.task_notes(id)? {
        return Ok(notes);
    }
    // Legacy files are read through the profile directory capability. Neither a
    // caller-supplied path nor a symlink can redirect this read outside it.
    let state = Workspace::open(&paths.state)?;
    let file = format!("tasks/{id}/memory.md");
    match state.inspect(&file, LIMIT + 1) {
        Ok((bytes, size)) => {
            ensure!(
                size <= LIMIT as u64 && bytes.len() <= LIMIT,
                "Legacy task notes exceed 16 KB; shorten the original file before importing"
            );
            Ok(String::from_utf8(bytes)?)
        }
        Err(e)
            if e.downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            Ok(String::new())
        }
        Err(e) => Err(e.context("Could not read legacy task notes")),
    }
}
/// Recent notes are bounded, labelled historical data. Branch seeds freeze the
/// inherited text so deleting or editing the source never alters a branch.
pub fn archive(paths: &AppPaths, store: &Store, workspace: &Path, sid: &str) -> Result<String> {
    let count = store
        .query(
            "SELECT count(*) AS count FROM tasks WHERE session_id=?",
            [sid],
        )?
        .pop()
        .context("Task count missing")?;
    ensure!(
        count["count"].as_u64().unwrap_or(0) <= 10000,
        "This conversation exceeds the 10,000-task memory archive limit"
    );
    let mut text = String::new();
    for task in store.tasks(sid, 10000)? {
        let id = task["id"].as_str().context("Task ID missing")?;
        let notes = task_notes(paths, store, workspace, id)?;
        if !notes.is_empty() {
            text.push_str(&format!("Task {id} notes:\n{notes}\n\n"));
        }
        ensure!(
            text.len() <= 32_000_000,
            "Task notes exceed the 32 MB branch memory limit"
        );
    }
    if let Some(seed) = store
        .query(
            "SELECT value FROM session_meta WHERE session_id=? AND key='memory_seed'",
            [sid],
        )?
        .pop()
    {
        text.push_str(seed["value"].as_str().unwrap_or(""));
    }
    ensure!(
        text.len() <= 32_000_000,
        "Task notes exceed the 32 MB branch memory limit"
    );
    Ok(text)
}
pub fn context(paths: &AppPaths, store: &Store, workspace: &Path, sid: &str) -> Result<String> {
    let mut pieces = Vec::new();
    let seed = store
        .query(
            "SELECT value FROM session_meta WHERE session_id=? AND key='memory_seed'",
            [sid],
        )?
        .pop();
    let tasks = store.tasks(sid, 8)?;
    for task in &tasks {
        let id = task["id"].as_str().context("Missing task ID")?;
        let notes = task_notes(paths, store, workspace, id)?;
        if !notes.is_empty() {
            pieces.push(format!("Task {id} notes:\n{}", truncate(&notes, 4000)));
            if notes.len() > 4000 {
                pieces.push("[Task note excerpt; read task memory for the complete note.]".into());
            }
        }
    }
    if let Some(seed) = seed.and_then(|v| v["value"].as_str().map(str::to_owned)) {
        pieces.push(format!(
            "Inherited notes at branching:\n{}",
            truncate(&seed, 4000)
        ));
        if seed.len() > 4000 {
            pieces.push(
                "[Inherited notes excerpt; export the conversation for the saved branch notes.]"
                    .into(),
            );
        }
    }
    let text = pieces.join("\n\n");
    Ok(if text.len() > LIMIT {
        format!(
            "{}\n[Task notes excerpt; inspect task memory for the complete notes.]",
            truncate(&text, LIMIT - 100)
        )
    } else {
        text
    })
}
pub fn report(project: String, task: String, task_id: Option<&str>) -> Value {
    let mut parts = Vec::new();
    if !project.is_empty() {
        parts.push(format!("## Project notes\n{project}"));
    }
    if !task.is_empty() {
        parts.push(format!("## Task notes\n{task}"));
    }
    json!({"ok":true,"project":project,"task":task,"project_hash":crate::workspace::hash(project.as_bytes()),"task_hash":task_id.map(|_|crate::workspace::hash(task.as_bytes())),"task_id":task_id,"text":if parts.is_empty(){"No notes saved.".into()}else{parts.join("\n\n")}})
}
