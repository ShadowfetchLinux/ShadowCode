//! `/api/goals…`: long-running goals and their milestones. The engine side
//! lives in `crate::engine::goals`; pausing awaits the running job, the rest
//! runs on the blocking pool.
use super::*;

#[derive(Default, Deserialize)]
#[serde(default)]
struct GoalBody {
    workspace: Text,
    instruction: Text,
    session_id: Text,
    run: Flag,
    milestones: Option<Value>,
    status: Text,
    detail: Text,
}

impl Service {
    pub(super) async fn goal_routes(&self, call: &Arc<Call>) -> Result<Value> {
        let parts = call.parts();
        if parts.len() == 4 && call.method == "POST" {
            if let action @ ("pause" | "abandon") = parts[3] {
                return self.engine.stop_goal(parts[2], action == "abandon").await;
            }
        }
        self.blocking(call, Self::goal_routes_sync).await
    }
    fn goal_routes_sync(&self, call: &Call) -> Result<Value> {
        let store = self.engine.store();
        let body: GoalBody = call.body()?;
        match (call.method.as_str(), call.path.as_str()) {
            ("GET", "/api/goals") => {
                let workspace = self.workspace()?;
                return Ok(
                    json!({"goals":store.goals(if matches!(call.q("all"),"true"|"1"){None}else{Some(&workspace)})?}),
                );
            }
            ("POST", "/api/goals") => {
                let workspace = if body.workspace.is_empty() {
                    self.workspace()?
                } else {
                    Workspace::open(&expand_path(body.workspace.as_str())?)?.path
                };
                if let Some(sid) = body.session_id.non_empty() {
                    ensure!(
                        store.session(sid)?.context("Session not found")?["workspace"].as_str()
                            == workspace.to_str(),
                        "Session belongs to a different workspace"
                    );
                }
                let milestones = if call.body.get("milestones").is_some() {
                    serde_json::from_value::<Vec<MilestoneSpec>>(
                        body.milestones.clone().unwrap_or(Value::Null),
                    )?
                } else {
                    MilestoneSpec::default_plan()
                };
                let goal = store.create_goal(&workspace, body.instruction.as_str(), &milestones)?;
                if body.run.is_true() {
                    let goal = self.engine.start_goal(
                        goal["id"].as_str().context("Goal ID missing")?,
                        body.session_id.non_empty(),
                    )?;
                    self.select(&workspace, goal["session_id"].as_str().map(str::to_owned))?;
                    return Ok(goal);
                }
                return Ok(goal);
            }
            _ => {}
        }
        let parts = call.parts();
        if parts.len() < 3 {
            return Err(call.unavailable());
        }
        let gid = parts[2];
        match (call.method.as_str(), parts.get(3).copied(), parts.len()) {
            ("GET", None, 3) => store.goal(gid),
            ("DELETE", None, 3) => {
                self.engine.delete_goal(gid)?;
                Ok(json!({"ok":true}))
            }
            ("POST", Some("run"), 4) => {
                let goal = self.engine.start_goal(gid, body.session_id.non_empty())?;
                self.select(
                    Path::new(
                        goal["workspace"]
                            .as_str()
                            .context("Goal workspace missing")?,
                    ),
                    goal["session_id"].as_str().map(str::to_owned),
                )?;
                Ok(goal)
            }
            ("POST", Some("milestones"), 5) => self.engine.update_goal_milestone(
                gid,
                parts[4],
                body.status.as_str(),
                body.detail.as_str(),
            ),
            _ => Err(call.unavailable()),
        }
    }
}
