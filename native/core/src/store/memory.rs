use super::*;

impl Store {
    pub fn task_notes(&self, task_id: &str) -> Result<Option<String>> {
        Ok(self
            .lock()?
            .query_row(
                "SELECT content FROM task_notes WHERE task_id=?",
                [task_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// The caller resolves legacy notes before this transaction. Concurrent
    /// appenders always merge the latest stored value, never an earlier read.
    pub fn write_task_note(
        &self,
        workspace: &Path,
        task_id: &str,
        legacy: &str,
        note: &str,
        replacement_hash: Option<&str>,
    ) -> Result<String> {
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        let sid: String = tx.query_row("SELECT t.session_id FROM tasks t JOIN sessions s ON s.id=t.session_id WHERE t.id=? AND s.workspace=?", params![task_id,workspace.to_string_lossy()], |r| r.get(0)).optional()?.context("Task does not belong to this project")?;
        let old: Option<String> = tx
            .query_row(
                "SELECT content FROM task_notes WHERE task_id=?",
                [task_id],
                |r| r.get(0),
            )
            .optional()?;
        let old = old.as_deref().unwrap_or(legacy);
        let next = if let Some(hash) = replacement_hash {
            crate::memory::replace(old, note, Some(hash))?
        } else {
            crate::memory::append(old, note)?
        };
        tx.execute("INSERT INTO task_notes(task_id,content,updated_at) VALUES(?,?,?) ON CONFLICT(task_id) DO UPDATE SET content=excluded.content,updated_at=excluded.updated_at", params![task_id,next,now()])?;
        tx.execute("INSERT INTO events(ts,type,session_id,task_id,payload) VALUES(?,'memory.updated',?,?,?)",params![now(),sid,task_id,json!({"scope":"task","bytes":next.len()}).to_string()])?;
        tx.commit()?;
        Ok(next)
    }
}
