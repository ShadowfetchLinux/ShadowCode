//! Automations and their run history (tables `automations` and
//! `automation_runs`, schema version 26). The record itself is JSON in
//! `payload`; `paused` and `next_run_at` are columns the scheduler reads and
//! are authoritative over the copy in the payload.
use super::*;
use crate::automations::{Automation, Draft};
use serde::{Deserialize, Serialize};

/// One row of an automation's history: a run, or a missed / skipped time.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutomationRun {
    pub id: String,
    pub automation_id: String,
    /// `running`, `completed`, `failed`, `cancelled`, `timed_out`,
    /// `needs_approval`, `interrupted`, `missed` or `skipped`.
    pub status: String,
    /// `schedule`, `catch_up` or `manual`.
    pub trigger: String,
    pub scheduled_for: Option<f64>,
    pub started_at: f64,
    pub finished_at: Option<f64>,
    pub session_id: Option<String>,
    pub job_id: Option<String>,
    pub task_id: Option<String>,
    pub summary: String,
    pub detail: String,
    /// The job's `usage` (tokens and cost) when it finished.
    pub usage: Option<Value>,
    /// `{id, path, branch}` of the managed worktree the run used.
    pub worktree: Option<Value>,
    /// For `missed`: how many scheduled times were missed.
    pub missed: u32,
}

impl AutomationRun {
    pub fn duration(&self) -> Option<f64> {
        self.finished_at.map(|end| (end - self.started_at).max(0.0))
    }
}

const RUN_HISTORY_KEEP: usize = 200;

fn automation_from(row: &Value) -> Result<Automation> {
    let mut automation: Automation = serde_json::from_value(row["payload"].clone())
        .context("Automation record is unreadable")?;
    automation.paused = row["paused"].as_i64().unwrap_or(0) != 0;
    automation.next_run_at = row["next_run_at"].as_f64();
    Ok(automation)
}

fn run_from(row: &Value) -> Result<AutomationRun> {
    let mut run: AutomationRun = serde_json::from_value(row["payload"].clone())
        .context("Automation run record is unreadable")?;
    run.status = row["status"].as_str().unwrap_or("").to_owned();
    run.finished_at = row["finished_at"].as_f64();
    Ok(run)
}

impl Store {
    pub fn create_automation(
        &self,
        workspace: &Path,
        draft: &Draft,
        next_run_at: Option<f64>,
    ) -> Result<Automation> {
        draft.validate()?;
        let time = now();
        let automation = Automation {
            id: id(),
            workspace: workspace.to_owned(),
            name: draft.name.trim().to_owned(),
            prompt: draft.prompt.clone(),
            model: draft.model.trim().to_owned(),
            mode: draft.mode.clone(),
            schedule: draft.schedule.clone().context("Choose a schedule")?,
            timezone: draft.timezone,
            options: draft.options.clone(),
            paused: false,
            next_run_at,
            created_at: time,
            updated_at: time,
        };
        let count: i64 = self.lock()?.query_row(
            "SELECT COUNT(*) FROM automations WHERE workspace=?",
            [workspace.to_string_lossy()],
            |r| r.get(0),
        )?;
        ensure!(count < 50, "A project can have at most 50 automations");
        self.execute(
            "INSERT INTO automations(id,workspace,name,payload,paused,next_run_at,created_at,updated_at) VALUES(?,?,?,?,0,?,?,?)",
            params![automation.id, workspace.to_string_lossy(), automation.name, serde_json::to_string(&automation)?, next_run_at, time, time],
        )?;
        Ok(automation)
    }
    pub fn automation(&self, id: &str) -> Result<Automation> {
        let row = self
            .query("SELECT * FROM automations WHERE id=?", [id])?
            .into_iter()
            .next()
            .context("Automation not found")?;
        automation_from(&row)
    }
    /// A project's automations (or every project's), oldest first.
    pub fn automations(&self, workspace: Option<&Path>) -> Result<Vec<Automation>> {
        let rows = match workspace {
            Some(path) => self.query(
                "SELECT * FROM automations WHERE workspace=? ORDER BY created_at,id",
                [path.to_string_lossy()],
            )?,
            None => self.query("SELECT * FROM automations ORDER BY created_at,id", [])?,
        };
        rows.iter().map(automation_from).collect()
    }
    /// Enabled automations whose next time is at or before `now`.
    pub fn due_automations(&self, now: f64) -> Result<Vec<Automation>> {
        self.query(
            "SELECT * FROM automations WHERE paused=0 AND next_run_at IS NOT NULL AND next_run_at<=? ORDER BY next_run_at,id",
            [now],
        )?
        .iter()
        .map(automation_from)
        .collect()
    }
    /// Replace the editable fields; the schedule's next time is recomputed
    /// by the caller.
    pub fn update_automation(
        &self,
        id: &str,
        draft: &Draft,
        next_run_at: Option<f64>,
    ) -> Result<Automation> {
        draft.validate()?;
        let mut automation = self.automation(id)?;
        automation.name = draft.name.trim().to_owned();
        automation.prompt = draft.prompt.clone();
        automation.model = draft.model.trim().to_owned();
        automation.mode = draft.mode.clone();
        automation.schedule = draft.schedule.clone().context("Choose a schedule")?;
        automation.timezone = draft.timezone;
        automation.options = draft.options.clone();
        automation.next_run_at = next_run_at;
        automation.updated_at = now();
        self.execute(
            "UPDATE automations SET name=?,payload=?,next_run_at=?,updated_at=? WHERE id=?",
            params![
                automation.name,
                serde_json::to_string(&automation)?,
                next_run_at,
                automation.updated_at,
                id
            ],
        )?;
        Ok(automation)
    }
    pub fn set_automation_paused(
        &self,
        id: &str,
        paused: bool,
        next_run_at: Option<f64>,
    ) -> Result<Automation> {
        let changed = self.execute(
            "UPDATE automations SET paused=?,next_run_at=?,updated_at=? WHERE id=?",
            params![paused as i64, next_run_at, now(), id],
        )?;
        ensure!(changed == 1, "Automation not found");
        self.automation(id)
    }
    pub fn set_automation_next_run(&self, id: &str, next_run_at: Option<f64>) -> Result<()> {
        self.execute(
            "UPDATE automations SET next_run_at=? WHERE id=?",
            params![next_run_at, id],
        )?;
        Ok(())
    }
    /// Delete an automation and its history. Conversations its runs created
    /// stay in the sidebar.
    pub fn delete_automation(&self, id: &str) -> Result<()> {
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        tx.execute(
            "DELETE FROM automation_runs WHERE automation_id=?",
            params![id],
        )?;
        let removed = tx.execute("DELETE FROM automations WHERE id=?", params![id])?;
        ensure!(removed == 1, "Automation not found");
        tx.commit()?;
        Ok(())
    }

    pub fn save_automation_run(&self, run: &AutomationRun) -> Result<()> {
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        tx.execute(
            "INSERT INTO automation_runs(id,automation_id,status,origin,scheduled_for,started_at,finished_at,session_id,job_id,payload) VALUES(?,?,?,?,?,?,?,?,?,?)
             ON CONFLICT(id) DO UPDATE SET status=excluded.status,finished_at=excluded.finished_at,session_id=excluded.session_id,job_id=excluded.job_id,payload=excluded.payload",
            params![run.id, run.automation_id, run.status, run.trigger, run.scheduled_for, run.started_at, run.finished_at, run.session_id, run.job_id, serde_json::to_string(run)?],
        )?;
        // Keep the newest rows only.
        tx.execute(
            "DELETE FROM automation_runs WHERE automation_id=? AND id NOT IN (SELECT id FROM automation_runs WHERE automation_id=? ORDER BY started_at DESC,rowid DESC LIMIT ?)",
            params![run.automation_id, run.automation_id, RUN_HISTORY_KEEP as i64],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn automation_run(&self, id: &str) -> Result<AutomationRun> {
        let row = self
            .query("SELECT * FROM automation_runs WHERE id=?", [id])?
            .into_iter()
            .next()
            .context("Automation run not found")?;
        run_from(&row)
    }
    /// Newest first.
    pub fn automation_runs(&self, automation_id: &str, limit: usize) -> Result<Vec<AutomationRun>> {
        self.query(
            "SELECT * FROM automation_runs WHERE automation_id=? ORDER BY started_at DESC,rowid DESC LIMIT ?",
            params![automation_id, limit.clamp(1, RUN_HISTORY_KEEP) as i64],
        )?
        .iter()
        .map(run_from)
        .collect()
    }
    /// Point a finished automation conversation at the project itself once
    /// its temporary worktree (which had no edits) is removed, so it still
    /// opens and a follow-up runs in the project.
    pub fn move_session_workspace(&self, session_id: &str, workspace: &Path) -> Result<()> {
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        tx.execute(
            "UPDATE sessions SET workspace=? WHERE id=?",
            params![workspace.to_string_lossy(), session_id],
        )?;
        tx.execute(
            "DELETE FROM session_meta WHERE session_id=? AND key=?",
            params![session_id, keys::AUTOMATION_WORKTREE],
        )?;
        tx.commit()?;
        Ok(())
    }
    /// Runs that were still going when ShadowCode stopped.
    pub fn recover_automation_runs(&self) -> Result<usize> {
        let time = now();
        let rows = self.query("SELECT * FROM automation_runs WHERE status='running'", [])?;
        let mut count = 0;
        for row in rows {
            let mut run = run_from(&row)?;
            run.status = "interrupted".into();
            run.finished_at = Some(time);
            if run.detail.is_empty() {
                run.detail = "ShadowCode stopped before this run finished. Open its conversation to review what it did.".into();
            }
            self.save_automation_run(&run)?;
            count += 1;
        }
        Ok(count)
    }
}
