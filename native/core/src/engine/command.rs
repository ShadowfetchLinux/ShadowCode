use super::*;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommandRequest {
    pub command: String,
    pub timeout_sec: u64,
}
impl Engine {
    pub(crate) async fn start_command(
        &self,
        mut request: StartRequest,
        command: CommandRequest,
        owner: Option<&JobOwner>,
    ) -> Result<Job> {
        ensure!(
            !command.command.trim().is_empty()
                && command.command.len() <= 64_000
                && !command.command.contains('\0'),
            "Command must contain between 1 and 64000 bytes without NUL"
        );
        ensure!(
            (1..=3600).contains(&command.timeout_sec),
            "Command timeout must be between 1 and 3600 seconds"
        );
        request.mode = "command".into();
        request.task = format!("Run test command: {}", command.command);
        self.start_with_context(
            request,
            LaunchContext {
                command: Some(command),
                owner,
                permission_limit: Some(PermissionLevel::Workspace),
                ..Default::default()
            },
        )
        .await
    }
    pub(super) async fn run_command_job(
        &self,
        running: &Running,
        job: &Job,
        events: &TaskEvents,
        tools: &ToolExecutor,
        command: &CommandRequest,
    ) -> Result<(String, Value)> {
        events.emit("user.message", json!({"text":job.task}))?;
        events.emit(
            "agent.started",
            json!({"task":job.task,"model":"native command","mode":"command"}),
        )?;
        let arguments = json!({"command":command.command,"timeout_sec":command.timeout_sec.min(running.config.agent.tool_timeout_sec)});
        let call = crate::models::ToolCall {
            id: crate::id(),
            name: "exec".into(),
            arguments: arguments.clone(),
        };
        let mut result = tools.execute(call).await?;
        if result.success {
            let outcomes = tools
                .fire_hooks(hooks::context(
                    "on_complete",
                    "exec",
                    &arguments,
                    &result.output,
                    "",
                ))
                .await;
            let failure = match outcomes {
                Ok(outcomes) => hooks::failure(&outcomes),
                Err(error) => Some(error.to_string()),
            };
            if let Some(failure) = failure {
                result.success = false;
                result.error = format!("Completion checks failed: {failure}");
            }
        }
        events.emit("command.completed",json!({"command":command.command,"success":result.success,"stdout":result.output["stdout"],"stderr":result.output["stderr"],"exit_code":result.output["exit_code"],"timed_out":result.output["timed_out"],"truncated":result.output["truncated"],"error":result.error}))?;
        events.emit("verification.summary",json!({"status":if result.success{"verified"}else{"failed"},"commands":[{"command":command.command,"exit_code":result.output["exit_code"],"success":result.success}],"source":"native command; no model was called"}))?;
        running
            .record
            .lock()
            .map_err(|_| anyhow!("Job lock poisoned"))?
            .steps = 1;
        ensure!(result.success, "{}", result.error);
        Ok((
            format!(
                "Test command completed with exit status {}.",
                result.output["exit_code"]
            ),
            json!({"steps":[]}),
        ))
    }
}
