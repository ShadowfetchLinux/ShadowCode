//! `/api/automations…`: scheduled automations, their history, pause /
//! resume / run now / stop, and a schedule preview for the editor. The rules
//! live in `crate::automations`, the scheduler in `engine::automations`.
use super::*;
use crate::{
    automations::{self as rules, Automation, Draft, Schedule, Timezone},
    store::AutomationRun,
};

#[derive(Default, Deserialize)]
#[serde(default)]
struct PreviewBody {
    schedule: Option<Schedule>,
    timezone: Timezone,
}

fn run_view(run: &AutomationRun) -> Value {
    let mut value = json!(run);
    value["duration"] = json!(run.duration());
    value
}

impl Service {
    pub(super) async fn automation_routes(&self, call: &Arc<Call>) -> Result<Value> {
        let parts = call.parts();
        if parts.len() == 4 && call.method == "POST" {
            match parts[3] {
                "run" => {
                    let run = self.engine.run_automation_now(parts[2]).await?;
                    return Ok(run_view(&run));
                }
                "stop" => {
                    let run = self.engine.stop_automation(parts[2]).await?;
                    return Ok(run_view(&run));
                }
                _ => {}
            }
        }
        self.blocking(call, Self::automation_routes_sync).await
    }

    fn automation_view(&self, automation: &Automation) -> Result<Value> {
        let last = self
            .engine
            .store()
            .automation_runs(&automation.id, 1)?
            .into_iter()
            .next();
        let mut value = json!(automation);
        value["description"] = json!(automation.schedule.describe(automation.timezone));
        value["running_run"] = json!(self.engine.automation_active_run(&automation.id));
        value["last_run"] = last.as_ref().map(run_view).unwrap_or(Value::Null);
        Ok(value)
    }

    fn automation_routes_sync(&self, call: &Call) -> Result<Value> {
        let store = self.engine.store();
        let now = crate::now();
        match (call.method.as_str(), call.path.as_str()) {
            ("GET", "/api/automations") => {
                let workspace = self.workspace()?;
                let all = matches!(call.q("all"), "true" | "1");
                let rows = store.automations(if all { None } else { Some(&workspace) })?;
                let views = rows
                    .iter()
                    .map(|a| self.automation_view(a))
                    .collect::<Result<Vec<_>>>()?;
                return Ok(json!({
                    "workspace": workspace,
                    "automations": views,
                    "scheduler": self.engine.automations_scheduled(),
                    "now": now,
                }));
            }
            ("POST", "/api/automations/preview") => {
                let body: PreviewBody = call.body()?;
                let Some(schedule) = body.schedule else {
                    return Ok(json!({"ok": false, "error": "Choose a schedule"}));
                };
                return Ok(match rules::upcoming(&schedule, body.timezone, now, 3) {
                    Ok(next) if next.is_empty() => {
                        json!({"ok": false, "error": "This schedule never runs"})
                    }
                    Ok(next) => json!({
                        "ok": true,
                        "description": schedule.describe(body.timezone),
                        "next": next,
                        "now": now,
                    }),
                    Err(error) => json!({"ok": false, "error": format!("{error:#}")}),
                });
            }
            ("POST", "/api/automations") => {
                let workspace = match call.text("workspace") {
                    "" => self.workspace()?,
                    path => Workspace::open(&expand_path(path)?)?.path,
                };
                ensure!(
                    Config::load(self.engine.paths(), Some(&workspace))?.is_trusted(&workspace),
                    "Trust this project before adding automations"
                );
                let draft: Draft = serde_json::from_value(call.body.clone())
                    .context("Automation settings are not valid")?;
                draft.validate()?;
                let schedule = draft.schedule.as_ref().context("Choose a schedule")?;
                let next = rules::next_run(schedule, draft.timezone, now)?;
                ensure!(next.is_some(), "This schedule never runs");
                let automation = store.create_automation(&workspace, &draft, next)?;
                return self.automation_view(&automation);
            }
            _ => {}
        }
        let parts = call.parts();
        if parts.len() < 3 {
            return Err(call.unavailable());
        }
        let id = parts[2];
        match (call.method.as_str(), parts.get(3).copied(), parts.len()) {
            ("GET", None, 3) => {
                let automation = store.automation(id)?;
                let mut value = self.automation_view(&automation)?;
                value["runs"] = json!(store
                    .automation_runs(id, call.limit(50, 200))?
                    .iter()
                    .map(run_view)
                    .collect::<Vec<_>>());
                Ok(value)
            }
            ("GET", Some("runs"), 4) => Ok(json!({
                "runs": store
                    .automation_runs(id, call.limit(50, 200))?
                    .iter()
                    .map(run_view)
                    .collect::<Vec<_>>()
            })),
            ("POST", None, 3) => {
                let draft: Draft = serde_json::from_value(call.body.clone())
                    .context("Automation settings are not valid")?;
                draft.validate()?;
                let current = store.automation(id)?;
                let schedule = draft.schedule.as_ref().context("Choose a schedule")?;
                let next = if current.paused {
                    None
                } else {
                    let next = rules::next_run(schedule, draft.timezone, now)?;
                    ensure!(next.is_some(), "This schedule never runs");
                    next
                };
                let automation = store.update_automation(id, &draft, next)?;
                self.automation_view(&automation)
            }
            ("DELETE", None, 3) => {
                self.engine.delete_automation(id)?;
                Ok(json!({"ok": true}))
            }
            ("POST", Some("pause"), 4) => {
                let automation = store.set_automation_paused(id, true, None)?;
                self.automation_view(&automation)
            }
            ("POST", Some("resume"), 4) => {
                // Paused time is never caught up: the next time is from now.
                let current = store.automation(id)?;
                let next = rules::next_run(&current.schedule, current.timezone, now)?;
                let automation = store.set_automation_paused(id, false, next)?;
                self.automation_view(&automation)
            }
            _ => Err(call.unavailable()),
        }
    }
}
