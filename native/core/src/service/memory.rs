use super::*;
use crate::memory;

impl Service {
    pub(super) fn memory(&self, body: &Value) -> Result<Value> {
        let action = body["action"].as_str().unwrap_or("read");
        let scope = body["scope"].as_str().unwrap_or("project");
        ensure!(
            matches!(action, "read" | "append" | "replace") && matches!(scope, "project" | "task"),
            "Choose read/append/replace and project/task memory"
        );
        let selection = self.snapshot_selection()?;
        let workspace = selection.workspace;
        let store = self.engine.store();
        let sid = body["session_id"]
            .as_str()
            .map(str::to_owned)
            .or(selection.session);
        if let Some(sid) = &sid {
            ensure!(
                store.session(sid)?.context("Conversation does not exist")?["workspace"].as_str()
                    == workspace.to_str(),
                "Conversation belongs to another project"
            );
        }
        let task_id = if let Some(id) = body["task_id"].as_str().filter(|s| !s.is_empty()) {
            Some(id.to_owned())
        } else if let Some(sid) = sid.as_ref().filter(|_| scope == "task" || action == "read") {
            store
                .tasks(sid, 1)?
                .first()
                .and_then(|t| t["id"].as_str())
                .map(str::to_owned)
        } else {
            None
        };
        if scope == "task" {
            ensure!(
                task_id.is_some(),
                "Choose an existing task ID or a conversation with a task"
            );
        }
        let reservation = if action != "read" {
            Some(self.mutable_workspace_at(&workspace)?)
        } else {
            None
        };
        let ws = Workspace::open(&workspace)?;
        let path = ".shadow/memory/project.md";
        let before = ws.snapshot(path)?;
        let mut project = before
            .bytes
            .map(String::from_utf8)
            .transpose()?
            .unwrap_or_default();
        let mut task = task_id
            .as_deref()
            .map(|id| memory::task_notes(self.engine.paths(), &store, &workspace, id))
            .transpose()?
            .unwrap_or_default();
        if action != "read" {
            let note = body["note"].as_str().context("A note is required")?;
            let replacement_hash =
                if action == "replace" {
                    Some(body["expected_hash"].as_str().context(
                        "Read the notes and provide their expected_hash before replacing",
                    )?)
                } else {
                    None
                };
            if scope == "project" {
                project = if replacement_hash.is_some() {
                    memory::replace(&project, note, replacement_hash)?
                } else {
                    memory::append(&project, note)?
                };
                reservation.as_ref().unwrap().write(
                    path,
                    project.as_bytes(),
                    before.hash.as_deref().or(Some("missing")),
                )?;
            } else {
                task = store.write_task_note(
                    &workspace,
                    task_id.as_deref().unwrap(),
                    &task,
                    note,
                    replacement_hash,
                )?;
            }
        }
        let mut report = memory::report(project, task, task_id.as_deref());
        if let Some(sid) = sid {
            if let Some(seed) = store
                .query(
                    "SELECT value FROM session_meta WHERE session_id=? AND key='memory_seed'",
                    [sid],
                )?
                .pop()
            {
                let text = seed["value"].as_str().unwrap_or("");
                let excerpt = truncate(text, 4000);
                report["inherited_notes"] = json!(excerpt);
                report["inherited_truncated"] = json!(text.len() > 4000);
                report["text"] = json!(format!(
                    "{}\n\n## Inherited task notes\n{excerpt}{}",
                    report["text"].as_str().unwrap_or(""),
                    if text.len() > 4000 {
                        "\n[Excerpt; export this conversation for all inherited notes.]"
                    } else {
                        ""
                    }
                ));
            }
        }
        Ok(report)
    }
}
