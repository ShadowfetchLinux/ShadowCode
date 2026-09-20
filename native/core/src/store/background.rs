use super::*;
use crate::background::BackgroundTask;

impl Store {
    pub fn save_background_event(
        &self,
        task: &BackgroundTask,
        event: &str,
        payload: &Value,
    ) -> Result<()> {
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        tx.execute("INSERT INTO background_processes(id,workspace,started_at,status,payload) VALUES(?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET status=excluded.status,payload=excluded.payload", params![task.id,task.cwd,task.started_at,task.status,serde_json::to_string(task)?])?;
        tx.execute(
            "INSERT INTO events(ts,type,session_id,task_id,payload) VALUES(?,?,?,NULL,?)",
            params![now(), event, task.session_id, payload.to_string()],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn save_background(&self, task: &BackgroundTask) -> Result<()> {
        self.execute("INSERT INTO background_processes(id,workspace,started_at,status,payload) VALUES(?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET status=excluded.status,payload=excluded.payload", params![task.id,task.cwd,task.started_at,task.status,serde_json::to_string(task)?])?;
        Ok(())
    }
    pub fn background_task(&self, id: &str) -> Result<Option<BackgroundTask>> {
        self.lock()?
            .query_row(
                "SELECT payload FROM background_processes WHERE id=?",
                [id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|text| Ok(serde_json::from_str(&text)?))
            .transpose()
    }
    pub fn background_tasks(&self, workspace: &Path) -> Result<Vec<BackgroundTask>> {
        let db = self.lock()?;
        let mut stmt = db.prepare("SELECT payload FROM background_processes WHERE workspace=? ORDER BY started_at DESC,id DESC LIMIT 100")?;
        let rows = stmt.query_map([workspace.to_string_lossy()], |row| row.get::<_, String>(0))?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }
    pub fn recover_background(&self) -> Result<()> {
        self.execute("UPDATE background_processes SET status='INTERRUPTED',payload=json_set(payload,'$.status','INTERRUPTED','$.ended_at',?,'$.error','ShadowCode stopped before this process finished. It was not restarted; an old PID is never used to signal a process.') WHERE status IN ('STARTING','RUNNING','STOPPING')",[now()])?;
        Ok(())
    }
    pub(super) fn import_legacy_background(&self) -> Result<()> {
        let path = self.path.with_file_name("background.db");
        if !path.is_file() {
            return Ok(());
        }
        let mut db = self.lock()?;
        if db
            .query_row(
                "SELECT value FROM native_meta WHERE key='legacy_background_imported'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .is_some()
        {
            return Ok(());
        }
        let legacy = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        legacy.busy_timeout(Duration::from_secs(2))?;
        let mut stmt = legacy.prepare("SELECT id,name,command,cwd,status,pid,started_at,ended_at,exit_code,substr(output,-64000),error,length(CAST(output AS BLOB))>64000 FROM tasks")?;
        let rows = stmt.query_map([], |row| {
            Ok(BackgroundTask {
                id: format!("legacy:{}", row.get::<_, String>(0)?),
                name: row.get(1)?,
                command: row.get(2)?,
                cwd: row.get(3)?,
                status: row.get(4)?,
                pid: row.get::<_, Option<u32>>(5)?.unwrap_or(0),
                started_at: row.get::<_, Option<f64>>(6)?.unwrap_or(0.0),
                ended_at: row.get(7)?,
                exit_code: row.get(8)?,
                output: row.get::<_, Option<String>>(9)?.unwrap_or_default(),
                error: row.get::<_, Option<String>>(10)?.unwrap_or_default(),
                truncated: row.get::<_, Option<bool>>(11)?.unwrap_or(false),
                ..Default::default()
            })
        })?;
        let tx = db.transaction()?;
        for row in rows {
            let mut task = row?;
            // Keep imports bounded without rewriting the original log database.
            if task.output.len() > 64000 {
                let mut cut = task.output.len() - 64000;
                while !task.output.is_char_boundary(cut) {
                    cut += 1;
                }
                task.output.drain(..cut);
            }
            if !matches!(task.status.as_str(), "COMPLETED" | "FAILED" | "CANCELLED") {
                task.status = "INTERRUPTED".into();
                task.error="Imported history; no live native process handle exists. The old PID is never signalled.".into();
                task.ended_at = Some(now());
            }
            tx.execute("INSERT OR IGNORE INTO background_processes(id,workspace,started_at,status,payload) VALUES(?,?,?,?,?)",params![task.id,task.cwd,task.started_at,task.status,serde_json::to_string(&task)?])?;
        }
        tx.execute(
            "INSERT INTO native_meta(key,value) VALUES('legacy_background_imported','true')",
            [],
        )?;
        tx.commit()?;
        Ok(())
    }
}
