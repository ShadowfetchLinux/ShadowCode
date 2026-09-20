use super::*;
use crate::background::{BackgroundManager, BackgroundTask};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Start {
    name: String,
    command: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Id {
    id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Output {
    id: String,
    #[serde(default = "output_limit")]
    max_bytes: usize,
}
fn output_limit() -> usize {
    8000
}

fn active(task: &BackgroundTask) -> bool {
    matches!(task.status.as_str(), "STARTING" | "RUNNING" | "STOPPING")
}
fn view(mut task: BackgroundTask, max_bytes: usize) -> Value {
    let clipped = task.output.len() > max_bytes;
    if clipped {
        let mut cut = task.output.len() - max_bytes;
        while !task.output.is_char_boundary(cut) {
            cut += 1;
        }
        task.output.drain(..cut);
    }
    let command_clipped = task.command.len() > 1000;
    task.command = truncate(&task.command, 1000).into();
    let mut value = json!(task);
    value["output_preview_truncated"] = json!(clipped);
    value["command_truncated"] = json!(command_clipped);
    value["lifetime"] = json!("project");
    value
}
impl ToolExecutor {
    fn background_manager(&self) -> Result<&BackgroundManager> {
        self.background
            .as_deref()
            .context("Managed background tools require the native engine")
    }
    fn project_process(&self, id: &str) -> Result<BackgroundTask> {
        ensure!(
            !id.is_empty() && id.len() <= 128 && !id.contains('\0'),
            "Use an exact background process ID from background_list"
        );
        let task = self.background_manager()?.get(id)?;
        ensure!(
            std::path::Path::new(&task.cwd) == self.workspace.path,
            "Background process belongs to another project"
        );
        Ok(task)
    }
    /// Validate the exact operation before showing an approval. Stopping a
    /// process presents its recorded command, not a model-provided description.
    pub(super) fn background_prompt(&self, call: &ToolCall) -> Result<Option<String>> {
        match call.name.as_str() {
            "background_start" => {
                self.background_manager()?;
                let args: Start = serde_json::from_value(call.arguments.clone())?;
                ensure!(
                    !args.name.trim().is_empty() && args.name.len() <= 80,
                    "Process name must contain 1–80 bytes"
                );
                ensure!(
                    !args.command.trim().is_empty()
                        && args.command.len() <= 64000
                        && !args.command.contains('\0'),
                    "Command must contain 1–64000 bytes without NUL"
                );
                ensure!(
                    self.config.is_trusted(&self.workspace.path),
                    "Trust this project before running background processes"
                );
                Ok(Some(format!(
                    "Start background process: {}\nProject: {}\n{}",
                    args.name.trim(),
                    self.workspace.path.display(),
                    args.command
                )))
            }
            "background_stop" => {
                let args: Id = serde_json::from_value(call.arguments.clone())?;
                let task = self.project_process(&args.id)?;
                Ok(Some(format!(
                    "Stop background process: {} ({})\nProject: {}\n{}",
                    task.name, task.id, task.cwd, task.command
                )))
            }
            "background_output" => {
                let args: Output = serde_json::from_value(call.arguments.clone())?;
                ensure!(
                    (1..=64000).contains(&args.max_bytes),
                    "Log limit must be between 1 and 64000 bytes"
                );
                self.project_process(&args.id)?;
                Ok(None)
            }
            "background_list" => {
                self.background_manager()?;
                ensure!(
                    call.arguments
                        .as_object()
                        .is_some_and(|args| args.is_empty()),
                    "background_list takes no arguments"
                );
                Ok(None)
            }
            _ => Ok(None),
        }
    }
    pub(super) async fn background_call(&self, call: &ToolCall) -> Result<Value> {
        let manager = self.background_manager()?;
        match call.name.as_str() {
            "background_start" => {
                let args: Start = serde_json::from_value(call.arguments.clone())?;
                self.observed
                    .lock()
                    .map_err(|_| anyhow::anyhow!("File observations lock poisoned"))?
                    .clear();
                let task = manager.start_for_task(
                    &self.workspace.path,
                    &self.config,
                    &self.events,
                    &args.name,
                    &args.command,
                )?;
                let mut value = view(task, 8000);
                value["notice"] = json!("Registered a project process. Check status and output before claiming readiness. It continues independently after this coding task, including cancellation; use background_stop when finished.");
                Ok(value)
            }
            "background_list" => {
                let tasks = manager.list(&self.workspace.path)?;
                let available = tasks.len();
                let mut history = 0;
                let tasks: Vec<_> = tasks
                    .into_iter()
                    .filter(|task| {
                        if active(task) {
                            true
                        } else {
                            history += 1;
                            history <= 12
                        }
                    })
                    .map(|task| view(task, 512))
                    .collect();
                Ok(json!({"history_truncated":tasks.len()<available,"tasks":tasks}))
            }
            "background_output" => {
                let args: Output = serde_json::from_value(call.arguments.clone())?;
                Ok(view(self.project_process(&args.id)?, args.max_bytes))
            }
            "background_stop" => {
                let args: Id = serde_json::from_value(call.arguments.clone())?;
                self.project_process(&args.id)?;
                self.observed
                    .lock()
                    .map_err(|_| anyhow::anyhow!("File observations lock poisoned"))?
                    .clear();
                // Once approved, cleanup finishes even if the coding task is
                // cancelled concurrently. The manager owns the live handle.
                Ok(view(manager.stop(&args.id).await?, 8000))
            }
            _ => bail!("Unknown background tool"),
        }
    }
}
