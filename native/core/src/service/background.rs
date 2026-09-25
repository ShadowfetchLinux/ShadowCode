//! `/api/background…`: long-running project processes (dev servers,
//! watchers) started from the desktop. The manager lives in
//! `crate::background`.
use super::*;

impl Service {
    pub(super) async fn background_routes(&self, call: &Arc<Call>) -> Result<Value> {
        let parts = call.parts();
        if parts.len() == 4 && call.method == "POST" && parts[3] == "stop" {
            self.check_background_project(parts[2])?;
            return Ok(json!(self.engine.background().stop(parts[2]).await?));
        }
        self.blocking(call, Self::background_routes_sync).await
    }
    fn background_routes_sync(&self, call: &Call) -> Result<Value> {
        match (call.method.as_str(), call.path.as_str()) {
            ("GET", "/api/background") => return self.background_list(),
            ("POST", "/api/background") => {
                let selection = self.snapshot_selection()?;
                let config = Config::load(self.engine.paths(), Some(&selection.workspace))?;
                return Ok(json!(self.engine.background().start(
                    &selection.workspace,
                    &config,
                    selection.session,
                    call.text("name"),
                    call.text("command")
                )?));
            }
            _ => {}
        }
        let parts = call.parts();
        if parts.len() >= 3 {
            let task = self.check_background_project(parts[2])?;
            if call.method == "GET" && parts.len() == 3 {
                return Ok(json!(task));
            }
        }
        Err(call.unavailable())
    }
    /// A process is managed from its own project only.
    fn check_background_project(&self, id: &str) -> Result<crate::background::BackgroundTask> {
        let task = self.engine.background().get(id)?;
        ensure!(
            Path::new(&task.cwd) == self.workspace()?,
            "Switch to this process's project before managing it"
        );
        Ok(task)
    }
    /// GET /api/background: the project's processes with output previews
    /// clipped to their last 4000 bytes.
    fn background_list(&self) -> Result<Value> {
        let tasks = self
            .engine
            .background()
            .list(&self.workspace()?)?
            .into_iter()
            .map(|mut task| {
                if task.command.len() > 4000 {
                    task.command = format!("{}…", truncate(&task.command, 4000));
                }
                let clipped = task.output.len() > 4000;
                if clipped {
                    let mut cut = task.output.len() - 4000;
                    while !task.output.is_char_boundary(cut) {
                        cut += 1;
                    }
                    task.output.drain(..cut);
                }
                let mut value = json!(task);
                value["output_preview_truncated"] = json!(clipped);
                value
            })
            .collect::<Vec<_>>();
        Ok(json!({"tasks":tasks}))
    }
}
