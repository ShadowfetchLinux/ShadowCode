use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MilestoneSpec {
    pub title: String,
    #[serde(default = "code_mode")]
    pub mode: String,
    #[serde(default)]
    pub require_verification: bool,
}
fn code_mode() -> String {
    "code".into()
}
impl MilestoneSpec {
    pub fn default_plan() -> Vec<Self> {
        [
            (
                "Inspect the project and define acceptance checks",
                "plan",
                false,
            ),
            ("Implement the requested change", "code", false),
            (
                "Run the acceptance checks and report evidence",
                "code",
                true,
            ),
        ]
        .into_iter()
        .map(|(title, mode, require_verification)| Self {
            title: title.into(),
            mode: mode.into(),
            require_verification,
        })
        .collect()
    }
}

impl Store {
    pub fn create_goal(
        &self,
        workspace: &Path,
        instruction: &str,
        milestones: &[MilestoneSpec],
    ) -> Result<Value> {
        let instruction = instruction.trim();
        ensure!(
            !instruction.is_empty() && instruction.len() <= 100_000,
            "Goal must contain between 1 and 100000 bytes"
        );
        ensure!(
            !milestones.is_empty() && milestones.len() <= 32,
            "A goal needs between 1 and 32 milestones"
        );
        for milestone in milestones {
            ensure!(
                !milestone.title.trim().is_empty() && milestone.title.len() <= 2000,
                "Milestone title must contain between 1 and 2000 bytes"
            );
            ensure!(
                matches!(milestone.mode.as_str(), "code" | "plan" | "review"),
                "Unknown milestone mode"
            );
            ensure!(
                !milestone.require_verification || milestone.mode == "code",
                "Command verification requires a Build milestone"
            );
        }
        let goal_id = id();
        let time = now();
        let title: String = instruction
            .lines()
            .next()
            .unwrap_or(instruction)
            .chars()
            .take(80)
            .collect();
        {
            let mut db = self.lock()?;
            let tx = db.transaction()?;
            tx.execute(
                "INSERT INTO goals VALUES(?,?,?,'active',0,?,?,?)",
                params![
                    goal_id,
                    workspace.to_string_lossy(),
                    instruction,
                    title,
                    time,
                    time
                ],
            )?;
            for (index, milestone) in milestones.iter().enumerate() {
                tx.execute("INSERT INTO milestones(id,goal_id,title,status,order_index,detail,task_id,created_at,updated_at,mode,require_verification) VALUES(?,?,?,'pending',?,'','',?,?,?,?)",
                    params![id(),goal_id,milestone.title,index,time,time,milestone.mode,milestone.require_verification])?;
            }
            tx.commit()?;
        }
        self.goal(&goal_id)
    }
    pub fn goal(&self, goal_id: &str) -> Result<Value> {
        goal_on(&*self.lock()?, goal_id)
    }
    pub fn goals(&self, workspace: Option<&Path>) -> Result<Vec<Value>> {
        let db = self.lock()?;
        let rows = query_rows(&db, "SELECT id FROM goals WHERE ? IS NULL OR workspace=? ORDER BY updated_at DESC LIMIT 500",
            params![workspace.map(|p|p.to_string_lossy()), workspace.map(|p|p.to_string_lossy())])?;
        rows.iter()
            .map(|row| goal_on(&db, row["id"].as_str().context("Goal ID missing")?))
            .collect()
    }
    pub(crate) fn begin_goal(&self, goal_id: &str, sid: &str) -> Result<Value> {
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        let goal = goal_on(&tx, goal_id)?;
        ensure!(
            !goal["running"].as_bool().unwrap_or(false),
            "This goal is already running"
        );
        ensure!(
            goal["milestones"]
                .as_array()
                .is_some_and(|ms| ms.iter().any(|m| m["status"] != "done")),
            "All milestones are already complete"
        );
        let time = now();
        tx.execute("UPDATE milestones SET status='pending',updated_at=? WHERE goal_id=? AND status IN ('failed','in_progress')",params![time,goal_id])?;
        tx.execute(
            "UPDATE goals SET status='active',updated_at=? WHERE id=?",
            params![time, goal_id],
        )?;
        tx.execute("INSERT INTO goal_runs VALUES(?,?,NULL,'running','',?) ON CONFLICT(goal_id) DO UPDATE SET session_id=excluded.session_id,job_id=NULL,status='running',detail='',updated_at=excluded.updated_at",params![goal_id,sid,time])?;
        let goal = goal_on(&tx, goal_id)?;
        tx.commit()?;
        Ok(goal)
    }
    pub(crate) fn goal_milestone_started(
        &self,
        goal_id: &str,
        mid: &str,
        job: &Value,
    ) -> Result<()> {
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        ensure!(tx.execute("UPDATE milestones SET status='in_progress',task_id=?,detail='',updated_at=? WHERE goal_id=? AND id=?",params![job["task_id"].as_str(),now(),goal_id,mid])?==1,"Milestone not found in this goal");
        tx.execute(
            "UPDATE goal_runs SET job_id=?,updated_at=? WHERE goal_id=?",
            params![job["id"].as_str(), now(), goal_id],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub(crate) fn set_milestone(
        &self,
        goal_id: &str,
        mid: &str,
        status: &str,
        detail: &str,
    ) -> Result<Value> {
        ensure!(
            matches!(status, "pending" | "in_progress" | "done" | "failed"),
            "Unknown milestone status"
        );
        ensure!(
            detail.len() <= 16000,
            "Milestone detail exceeds 16000 bytes"
        );
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        ensure!(
            tx.execute(
                "UPDATE milestones SET status=?,detail=?,updated_at=? WHERE goal_id=? AND id=?",
                params![status, detail, now(), goal_id, mid]
            )? == 1,
            "Milestone not found in this goal"
        );
        tx.execute("UPDATE goals SET progress=(SELECT AVG(CASE WHEN status='done' THEN 1.0 ELSE 0.0 END) FROM milestones WHERE goal_id=?),updated_at=? WHERE id=?",params![goal_id,now(),goal_id])?;
        tx.execute("UPDATE goals SET status=CASE WHEN progress=1 THEN 'completed' WHEN status='completed' THEN 'active' ELSE status END WHERE id=?",[goal_id])?;
        let goal = goal_on(&tx, goal_id)?;
        tx.commit()?;
        Ok(goal)
    }
    pub(crate) fn finish_goal(&self, goal_id: &str, status: &str, detail: &str) -> Result<()> {
        ensure!(
            matches!(
                status,
                "active" | "completed" | "blocked" | "paused" | "abandoned"
            ),
            "Unknown goal status"
        );
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        // Completion is derived from the persisted checklist, never from a
        // worker's optimistic final response.
        if status == "completed" {
            let goal = goal_on(&tx, goal_id)?;
            ensure!(
                goal["progress"].as_f64() == Some(1.0),
                "Unfinished milestones remain"
            );
        }
        ensure!(
            tx.execute(
                "UPDATE goals SET status=?,updated_at=? WHERE id=?",
                params![status, now(), goal_id]
            )? == 1,
            "Goal not found"
        );
        tx.execute(
            "UPDATE goal_runs SET status=?,detail=?,updated_at=? WHERE goal_id=?",
            params![status, detail, now(), goal_id],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub(crate) fn delete_goal(&self, goal_id: &str) -> Result<()> {
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        goal_on(&tx, goal_id)?;
        tx.execute("DELETE FROM goal_runs WHERE goal_id=?", [goal_id])?;
        tx.execute("DELETE FROM milestones WHERE goal_id=?", [goal_id])?;
        tx.execute("DELETE FROM goals WHERE id=?", [goal_id])?;
        tx.commit()?;
        Ok(())
    }
    /// A crashed run is never replayed automatically: its workspace may have
    /// changed, and a recorded shell command may already have had side effects.
    pub(crate) fn recover_goals(&self) -> Result<()> {
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        let detail="Application interrupted this goal. Review saved work, then resume the unfinished milestones.";
        tx.execute("UPDATE goals SET status='paused',updated_at=? WHERE id IN (SELECT goal_id FROM goal_runs WHERE status='running') OR id IN (SELECT goal_id FROM milestones WHERE status='in_progress')",[now()])?;
        tx.execute("UPDATE milestones SET status='pending',detail=?,updated_at=? WHERE status='in_progress'",params![detail,now()])?;
        tx.execute(
            "UPDATE goal_runs SET status='paused',detail=?,updated_at=? WHERE status='running'",
            params![detail, now()],
        )?;
        tx.commit()?;
        Ok(())
    }
}
fn goal_on(db: &Connection, goal_id: &str) -> Result<Value> {
    let mut goal=query_rows(db,"SELECT g.*,r.session_id,r.job_id,r.detail AS run_detail,COALESCE(r.status='running',0) AS running FROM goals g LEFT JOIN goal_runs r ON r.goal_id=g.id WHERE g.id=?",[goal_id])?.pop().context("Goal not found")?;
    goal["running"] = json!(goal["running"].as_i64() == Some(1));
    goal["progress_pct"] = json!((goal["progress"].as_f64().unwrap_or(0.0) * 100.0).round() as i64);
    let mut milestones = query_rows(
        db,
        "SELECT * FROM milestones WHERE goal_id=? ORDER BY order_index",
        [goal_id],
    )?;
    for m in &mut milestones {
        m["require_verification"] = json!(m["require_verification"].as_i64() == Some(1));
    }
    goal["milestones"] = json!(milestones);
    Ok(goal)
}
