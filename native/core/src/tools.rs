//! Audited native tools. Tool arguments never grant permissions or approvals.
use crate::{
    approvals::{Approval, ApprovalHub},
    checkpoint,
    config::Config,
    events::TaskEvents,
    hooks,
    models::ToolCall,
    patch::{self, Change},
    permissions::{self, Decision},
    process::{self, ProcessSpec},
    workspace::{hash, Snapshot, Workspace},
};
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

mod background;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolResult {
    pub id: String,
    pub success: bool,
    pub output: Value,
    pub error: String,
}
impl ToolResult {
    pub fn message(&self, name: &str, limit: usize) -> Value {
        let mut payload = json!({
            "success": self.success,
            "output": self.output,
            "error": self.error,
        });
        let redacted = crate::redaction::redact_value(&mut payload);
        let content = serde_json::to_string(&payload).unwrap_or_default();
        let content = if content.len() > limit {
            json!({
                "success":self.success,
                "truncated":true,
                "preview":truncate(&content,limit),
                "error":self.error,
                "redacted": redacted > 0,
                "note":format!("Tool output was truncated to {limit} bytes. Use offset/limit, max_hits, or a narrower query; do not assume the omitted bytes.")
            })
            .to_string()
        } else {
            content
        };
        let mut message =
            json!({"role":"tool","tool_call_id":self.id,"name":name,"content":content});
        if redacted > 0 {
            message["redacted"] = json!(true);
            message["redaction_count"] = json!(redacted);
        }
        message
    }
}
#[derive(Clone)]
pub struct ToolExecutor {
    pub workspace: Arc<Workspace>,
    pub config: Config,
    pub approvals: ApprovalHub,
    pub events: TaskEvents,
    pub cancel: CancellationToken,
    observed: Arc<Mutex<HashMap<String, String>>>,
    plan: Arc<Mutex<Value>>,
    hooks: hooks::Runner,
    background: Option<Arc<crate::background::BackgroundManager>>,
    #[cfg(unix)]
    mcp: crate::mcp::runner::Runner,
}
impl ToolExecutor {
    pub fn new(
        workspace: Arc<Workspace>,
        config: Config,
        approvals: ApprovalHub,
        events: TaskEvents,
        cancel: CancellationToken,
    ) -> Result<Self> {
        let task = events
            .store
            .task(&events.task_id)?
            .context("Task does not exist")?;
        ensure!(
            task["session_id"] == events.session_id,
            "Task belongs to another session"
        );
        let session = events
            .store
            .session(&events.session_id)?
            .context("Session does not exist")?;
        ensure!(
            session["workspace"].as_str() == workspace.path.to_str(),
            "Task workspace does not match its session"
        );
        let hooks = hooks::Runner::load(&workspace, &config)?;
        #[cfg(unix)]
        let mcp = crate::mcp::runner::Runner::load(workspace.clone(), config.clone())?;
        Ok(Self {
            workspace,
            config,
            approvals,
            events,
            cancel,
            observed: Arc::new(Mutex::new(HashMap::new())),
            plan: Arc::new(Mutex::new(json!({"goal":"","steps":[]}))),
            hooks,
            background: None,
            #[cfg(unix)]
            mcp,
        })
    }
    pub fn with_profile(mut self, paths: crate::paths::AppPaths) -> Self {
        #[cfg(unix)]
        self.mcp.set_profile(paths);
        self
    }
    pub fn with_background(mut self, manager: Arc<crate::background::BackgroundManager>) -> Self {
        self.background = Some(manager);
        self
    }
    pub fn schemas(&self) -> Vec<Value> {
        let mut schemas = schemas();
        if self.background.is_none() {
            schemas.retain(|schema| {
                !schema["function"]["name"]
                    .as_str()
                    .unwrap_or("")
                    .starts_with("background_")
            });
        }
        #[cfg(unix)]
        if !self.mcp.is_empty() {
            schemas.extend(crate::mcp::runner::schemas());
        }
        schemas
    }
    pub fn has_external_processes(&self) -> bool {
        #[cfg(unix)]
        if !self.mcp.is_empty() {
            return true;
        }
        self.has_hooks()
    }
    pub async fn close_integrations(&self) -> Result<()> {
        #[cfg(unix)]
        self.mcp.close(&self.events).await?;
        Ok(())
    }
    pub fn plan(&self) -> Value {
        self.plan
            .lock()
            .map(|v| v.clone())
            .unwrap_or_else(|_| json!({"steps":[]}))
    }
    pub fn observed_hashes(&self) -> Result<std::collections::BTreeMap<String, String>> {
        let observed = self
            .observed
            .lock()
            .map_err(|_| anyhow::anyhow!("File observations lock poisoned"))?;
        Ok(observed
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect())
    }
    pub fn has_hooks(&self) -> bool {
        !self.hooks.is_empty()
    }
    pub async fn fire_hooks(&self, context: Value) -> Result<Vec<hooks::Outcome>> {
        let outcomes = self
            .hooks
            .fire(
                context,
                &self.workspace,
                &self.config,
                &self.events,
                self.cancel.clone(),
            )
            .await?;
        if outcomes.iter().any(|o| o.process.is_some()) {
            self.observed
                .lock()
                .map_err(|_| anyhow::anyhow!("File observations lock poisoned"))?
                .clear();
        }
        Ok(outcomes)
    }
    pub async fn execute(&self, call: ToolCall) -> Result<ToolResult> {
        ensure!(!self.cancel.is_cancelled(), "Task cancelled");
        self.events.emit(
            "tool.started",
            json!({"tool":call.name,"arguments":call.arguments,"call_id":call.id}),
        )?;
        let mut result = match self.execute_inner(&call).await {
            Ok(output) => {
                let success = output.get("ok").and_then(Value::as_bool).unwrap_or(true);
                ToolResult {
                    id: call.id.clone(),
                    success,
                    error: if success {
                        String::new()
                    } else if let Some(error) = output["error"].as_str() {
                        error.into()
                    } else {
                        format!(
                            "Command exited with status {}{}",
                            output["exit_code"],
                            if output["timed_out"] == true {
                                " (timed out)"
                            } else if output["cancelled"] == true {
                                " (cancelled)"
                            } else {
                                ""
                            }
                        )
                    },
                    output,
                }
            }
            Err(error) => ToolResult {
                id: call.id.clone(),
                success: false,
                output: Value::Null,
                error: format!("{error:#}"),
            },
        };
        let mut outcomes = Vec::new();
        if result.success
            && matches!(
                call.name.as_str(),
                "write_file" | "edit_file" | "apply_patch"
            )
        {
            // A multi-file patch fires once per changed path, so a formatter
            // receives one literal path and suffix filters cannot select the
            // wrong file. All patch changes have already been applied.
            for path in result.output["paths"].as_array().into_iter().flatten() {
                outcomes.extend(
                    self.fire_hooks(hooks::context(
                        "after_edit",
                        &call.name,
                        &call.arguments,
                        &json!({"paths":[path]}),
                        "",
                    ))
                    .await?,
                );
            }
        }
        if call.name == "exec"
            && result.output.get("exit_code").is_some()
            && hooks::is_test(call.arguments["command"].as_str().unwrap_or(""))
        {
            outcomes.extend(
                self.fire_hooks(hooks::context(
                    "after_test",
                    &call.name,
                    &call.arguments,
                    &result.output,
                    &result.error,
                ))
                .await?,
            );
        }
        if !result.success {
            outcomes.extend(
                self.fire_hooks(hooks::context(
                    "on_error",
                    &call.name,
                    &call.arguments,
                    &result.output,
                    &result.error,
                ))
                .await?,
            );
        }
        if let Some(failure) = hooks::failure(&outcomes) {
            result.success = false;
            result.error=format!("{}\nLifecycle command failed after the tool action; completed changes remain applied:\n{failure}",result.error).trim().into();
        }
        if !outcomes.is_empty() {
            if !result.output.is_object() {
                result.output = json!({"tool_output":result.output});
            }
            result.output["hooks"] = json!(outcomes);
        }
        self.events.emit("tool.completed",json!({"tool":call.name,"call_id":call.id,"success":result.success,"output":result.output,"output_preview":truncate(&result.output.to_string(),2000),"error":result.error}))?;
        Ok(result)
    }
    async fn execute_inner(&self, call: &ToolCall) -> Result<Value> {
        ensure!(
            call.arguments.is_object(),
            "Tool arguments must be an object"
        );
        ensure!(
            call.arguments.to_string().len() <= 8_000_000,
            "Tool arguments exceed the limit"
        );
        let background_prompt = self.background_prompt(call)?;
        let decision = permissions::check(&self.config.permissions, &call.name, &call.arguments);
        #[cfg(unix)]
        let decision = if matches!(call.name.as_str(), "mcp_tools" | "mcp_call") {
            self.mcp
                .decision(&call.name, &call.arguments, &self.events)
                .await?
        } else {
            decision
        };
        match decision {
            Decision::Deny(reason) => bail!(reason),
            Decision::Ask(reason) => {
                let record = Approval {
                    id: String::new(),
                    session_id: self.events.session_id.clone(),
                    task_id: self.events.task_id.clone(),
                    tool: call.name.clone(),
                    arguments: call.arguments.clone(),
                    command: if let Some(prompt) = background_prompt {
                        prompt
                    } else if call.name == "mcp_call" {
                        format!(
                            "MCP {} / {}\n{}",
                            call.arguments["server"].as_str().unwrap_or(""),
                            call.arguments["tool"].as_str().unwrap_or(""),
                            serde_json::to_string_pretty(&call.arguments["arguments"])?
                        )
                    } else {
                        call.arguments["command"]
                            .as_str()
                            .unwrap_or(&call.name)
                            .into()
                    },
                    reason,
                    pending: true,
                    created_at: 0.0,
                    expires_at: 0.0,
                };
                let mut pending_error = None;
                let allowed = self
                    .approvals
                    .request(
                        record,
                        Duration::from_secs(600),
                        self.cancel.clone(),
                        |record| {
                            if let Err(error) =
                                self.events.emit("approval.requested", json!(record))
                            {
                                pending_error = Some(error);
                                self.cancel.cancel();
                            }
                        },
                    )
                    .await?;
                if let Some(error) = pending_error {
                    return Err(error);
                }
                self.events.emit(
                    "approval.resolved",
                    json!({"tool":call.name,"call_id":call.id,"approved":allowed}),
                )?;
                ensure!(allowed, "Permission was denied, cancelled, or expired");
            }
            Decision::Allow => {}
        }
        ensure!(
            !self.cancel.is_cancelled(),
            "Task cancelled before executing tool"
        );
        #[cfg(unix)]
        if matches!(call.name.as_str(), "mcp_tools" | "mcp_call") {
            // An external process can change the workspace even during catalog
            // discovery. Blind edits must not reuse observations from before it.
            self.observed
                .lock()
                .map_err(|_| anyhow::anyhow!("File observations lock poisoned"))?
                .clear();
            return self
                .mcp
                .execute(
                    &call.name,
                    &call.arguments,
                    &self.events,
                    self.cancel.clone(),
                )
                .await;
        }
        let before = match call.name.as_str() {
            "exec" | "background_start" => Some("before_command"),
            "git_commit" => Some("before_commit"),
            _ => None,
        };
        if let Some(event) = before {
            let outcomes = self
                .fire_hooks(hooks::context(
                    event,
                    &call.name,
                    &call.arguments,
                    &Value::Null,
                    "",
                ))
                .await?;
            if let Some(failure) = hooks::failure(&outcomes) {
                bail!("Action blocked by lifecycle command:\n{failure}");
            }
            ensure!(
                !self.cancel.is_cancelled(),
                "Task cancelled before executing tool"
            );
        }
        if call.name == "exec" {
            return self.shell(&call.arguments).await;
        }
        if matches!(
            call.name.as_str(),
            "background_start" | "background_list" | "background_output" | "background_stop"
        ) {
            return self.background_call(call).await;
        }
        if call.name.starts_with("git_") {
            return self.git(&call.name, &call.arguments).await;
        }
        #[cfg(unix)]
        if matches!(call.name.as_str(), "mcp_sqlite_tables" | "mcp_sqlite_query") {
            let mut request: crate::sqlite::Request =
                serde_json::from_value(call.arguments.clone())?;
            ensure!(
                request.sql.is_some() == (call.name == "mcp_sqlite_query"),
                "Use mcp_sqlite_query with sql, or mcp_sqlite_tables without sql"
            );
            request.timeout_ms = request
                .timeout_ms
                .min(self.config.agent.tool_timeout_sec.saturating_mul(1000));
            return crate::sqlite::inspect(self.workspace.clone(), request, self.cancel.clone())
                .await;
        }
        if matches!(
            call.name.as_str(),
            "workspace_symbols" | "goto_definition" | "find_references" | "get_type_signature"
        ) {
            let root = self.workspace.path.clone();
            let name = call.name.clone();
            let args = call.arguments.clone();
            return tokio::task::spawn_blocking(move || {
                let query = args["query"]
                    .as_str()
                    .or_else(|| args["symbol"].as_str())
                    .unwrap_or("");
                let max_hits = args["max_hits"].as_u64().unwrap_or(40) as usize;
                match name.as_str() {
                    "workspace_symbols" => {
                        crate::intelligence::workspace_symbols(&root, query, max_hits)
                    }
                    "goto_definition" => {
                        crate::intelligence::goto_definition(&root, query, max_hits)
                    }
                    "find_references" => {
                        crate::intelligence::find_references(&root, query, max_hits)
                    }
                    "get_type_signature" => crate::intelligence::get_type_signature(&root, query),
                    _ => unreachable!(),
                }
            })
            .await
            .context("Tool worker stopped unexpectedly")?;
        }
        if call.name == "get_diagnostics" {
            let path = call.arguments["path"].as_str().unwrap_or("");
            return crate::intelligence::get_diagnostics(&self.workspace.path, path).await;
        }
        let worker = self.clone();
        let call = call.clone();
        tokio::task::spawn_blocking(move || worker.files(&call.name, &call.arguments))
            .await
            .context("Tool worker stopped unexpectedly")?
    }
    async fn shell(&self, args: &Value) -> Result<Value> {
        let command = string(args, "command")?;
        ensure!(
            !command.is_empty() && command.len() <= 64_000 && !command.contains('\0'),
            "Invalid shell command"
        );
        let seconds = integer(
            args,
            "timeout_sec",
            self.config.agent.tool_timeout_sec as usize,
            1,
            3600,
        )?
        .min(self.config.agent.tool_timeout_sec as usize);
        let cwd = if let Some(path) = args["cwd"].as_str() {
            let relative = self.workspace.relative(path)?;
            let cwd = self.workspace.path.join(relative).canonicalize()?;
            ensure!(
                cwd.starts_with(&self.workspace.path),
                "Command directory escapes the workspace"
            );
            cwd
        } else {
            self.workspace.path.clone()
        };
        let allow_network = self.config.permissions.network;
        let sandbox_note;
        let mut scratch_path = None;
        let probe_cwd = self.workspace.path.clone();
        let probe_command = command.to_owned();
        let profile = tokio::task::spawn_blocking(move || {
            crate::sandbox::build_shell_profile(&probe_cwd, &probe_command, allow_network)
        })
        .await?;
        let primary = match profile {
            Ok(mut profile) => {
                if let Some(index) = profile.args.iter().position(|arg| arg == "--chdir") {
                    profile.args[index + 1] = cwd.to_string_lossy().into_owned();
                }
                scratch_path = profile.scratch_dir.clone();
                sandbox_note = json!({
                    "mode": "bubblewrap",
                    "network": profile.network,
                    "scratch": profile.scratch_dir.as_ref().map(|p| p.to_string_lossy()),
                    "workspace_cow": profile.workspace_cow,
                    "note": "Optional bubblewrap profile with ephemeral scratch at SHADOWCODE_SCRATCH; not a full OS sandbox. Real home is read-only."
                });
                ProcessSpec {
                    program: profile.program.to_string_lossy().into_owned(),
                    args: profile.args,
                    cwd: cwd.clone(),
                    timeout: Duration::from_secs(seconds as u64),
                    output_limit: self.config.agent.max_output_bytes,
                    env: Default::default(),
                }
            }
            Err(error) => {
                sandbox_note = json!({
                    "mode": "none",
                    "reason": format!("{error:#}"),
                    "note": "Shell continues without bubblewrap; policy remains heuristic, not an OS sandbox."
                });
                let mut spec =
                    ProcessSpec::shell(command, cwd.clone(), Duration::from_secs(seconds as u64));
                spec.output_limit = self.config.agent.max_output_bytes;
                spec
            }
        };
        // Never replay a command after a process failure: it may have changed files.
        let result = process::run(primary, self.cancel.clone(), None).await;
        if let Some(path) = scratch_path {
            crate::sandbox::discard_scratch(&path)?;
        }
        let result = result?;
        let mut value = serde_json::to_value(result)?;
        value["sandbox"] = sandbox_note;
        Ok(value)
    }
    async fn git(&self, name: &str, args: &Value) -> Result<Value> {
        let mut command = vec![
            "--no-pager".to_owned(),
            "--no-optional-locks".into(),
            "--literal-pathspecs".into(),
            "-c".into(),
            "core.fsmonitor=false".into(),
            "-c".into(),
            "core.hooksPath=/dev/null".into(),
        ];
        let mut git: Vec<String> = match name {
            "git_status" => vec!["status".into(), "--porcelain=v1".into(), "-b".into()],
            "git_diff" => {
                let mut v = vec![
                    "diff".into(),
                    "--no-ext-diff".into(),
                    "--no-textconv".into(),
                ];
                if args["staged"] == true {
                    v.push("--cached".into());
                }
                v
            }
            "git_log" => vec![
                "log".into(),
                format!("-{}", integer(args, "limit", 12, 1, 200)?),
                "--oneline".into(),
                "--decorate".into(),
                "--no-show-signature".into(),
            ],
            "git_branch" => {
                if args["create"] == true {
                    vec![
                        "switch".into(),
                        "-c".into(),
                        git_ref(string(args, "name")?)?,
                    ]
                } else {
                    vec!["branch".into(), "--list".into(), "-v".into()]
                }
            }
            "git_checkout" => vec!["switch".into(), git_ref(string(args, "ref")?)?],
            "git_add" => {
                let paths = args["paths"].as_array().context("paths must be an array")?;
                ensure!(
                    !paths.is_empty() && paths.len() <= 200,
                    "Stage between 1 and 200 paths"
                );
                let mut v = vec!["add".into(), "--".into()];
                for path in paths {
                    let path = path.as_str().context("Each path must be a string")?;
                    let rel = self.workspace.relative(path)?;
                    ensure!(
                        !rel.components().any(|c| c.as_os_str() == ".git"),
                        "Cannot stage Git metadata"
                    );
                    v.push(rel.to_string_lossy().into_owned());
                }
                v
            }
            "git_commit" => {
                let message = string(args, "message")?;
                ensure!(
                    !message.trim().is_empty()
                        && message.len() <= 32_000
                        && !message.contains('\0'),
                    "Invalid commit message"
                );
                vec![
                    "-c".into(),
                    "commit.gpgSign=false".into(),
                    "commit".into(),
                    "-m".into(),
                    message.into(),
                ]
            }
            "git_reset" => vec![
                "reset".into(),
                "--hard".into(),
                git_ref(args["ref"].as_str().unwrap_or("HEAD"))?,
            ],
            "git_clean" => vec!["clean".into(), "-fd".into()],
            _ => bail!("Unknown Git tool"),
        };
        command.append(&mut git);
        let refs: Vec<_> = command.iter().map(String::as_str).collect();
        let mut spec = ProcessSpec::command("git", &refs, self.workspace.path.clone());
        spec.output_limit = self.config.agent.max_output_bytes;
        let result = process::run(spec, self.cancel.clone(), None).await?;
        Ok(serde_json::to_value(result)?)
    }
    fn files(&self, name: &str, args: &Value) -> Result<Value> {
        ensure!(!self.cancel.is_cancelled(), "Task cancelled");
        match name {
            "system_info" => {
                ensure!(
                    args.as_object().is_some_and(|args| args.is_empty()),
                    "system_info takes no arguments"
                );
                Ok(crate::system_info::inspect())
            }
            "list_files" => {
                let entries = self.workspace.list(args["path"].as_str().unwrap_or("."))?;
                let count = integer(args, "max_entries", 400, 1, 10_000)?;
                let truncated = entries.len() > count;
                Ok(json!({
                    "truncated":truncated,
                    "entries":entries.into_iter().take(count).collect::<Vec<_>>(),
                    "note": if truncated {
                        "Listing was truncated. Raise max_entries or narrow the path; omitted names are not in this result."
                    } else {
                        ""
                    }
                }))
            }
            "read_file" => {
                let path = string(args, "path")?;
                if crate::redaction::is_secret_path(path) {
                    return Ok(crate::redaction::secret_file_refusal(path));
                }
                let file = self.workspace.read(path)?;
                self.observed
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Read history lock poisoned"))?
                    .insert(file.path.clone(), file.hash.clone());
                let offset = integer(args, "offset", 1, 1, 4_000_001)?;
                let limit = integer(args, "limit", 400, 1, 20_000)?;
                let total = file.content.lines().count();
                let content = file
                    .content
                    .split_inclusive('\n')
                    .skip(offset - 1)
                    .take(limit)
                    .collect::<String>();
                let redacted = crate::redaction::redact_text(&content);
                let preview = truncate(&redacted.text, self.config.agent.max_output_bytes);
                let range_truncated = offset - 1 + limit < total;
                let byte_truncated = preview.len() < redacted.text.len();
                let truncated = range_truncated || byte_truncated;
                let next_offset = if range_truncated {
                    Some(offset + limit)
                } else {
                    None
                };
                Ok(json!({
                    "path":file.path,
                    "content":preview,
                    "hash":file.hash,
                    "bytes":file.bytes,
                    "offset":offset,
                    "total_lines":total,
                    "truncated":truncated,
                    "next_offset":next_offset,
                    "redacted": redacted.redacted,
                    "redaction_count": redacted.count,
                    "note": if crate::redaction::is_secret_path(&file.path) {
                        "Secret file blocked.".into()
                    } else if truncated {
                        "Read was truncated. Continue from next_offset or raise limit; omitted lines are not in this result.".into()
                    } else if redacted.redacted {
                        "One or more secret-looking tokens were replaced with [redacted secret] before model context.".into()
                    } else {
                        String::new()
                    }
                }))
            }
            "search_files" => self.workspace.find_files(
                string(args, "query")?,
                args["path"].as_str().unwrap_or("."),
                &self.cancel,
            ),
            "search_text" | "search_symbol" => {
                let query = string(args, "query")?;
                ensure!(!query.is_empty(), "Search query must not be empty");
                let pattern = if name == "search_symbol" {
                    format!(
                        r"^\s*(?:(?:pub|async|export|static)\s+)*(?:def|class|function|fn|const|let|var|struct|enum|trait|interface|type)\s+{}\b",
                        regex::escape(query)
                    )
                } else if args["regex"] == true {
                    query.to_owned()
                } else {
                    regex::escape(query)
                };
                self.workspace.search_with_control(
                    &pattern,
                    args["glob"].as_str(),
                    args["path"].as_str().unwrap_or("."),
                    integer(args, "max_hits", 80, 1, 1000)?,
                    &self.cancel,
                )
            }
            "write_file" => {
                let path = string(args, "path")?;
                let before = self.workspace.snapshot(path)?;
                self.check_observed(path, &before, args["expected_hash"].as_str(), true)?;
                self.commit_changes(vec![Change {
                    path: path.into(),
                    mode: before.mode,
                    before,
                    after: Some(string(args, "content")?.as_bytes().to_vec()),
                }])
            }
            "edit_file" => {
                let path = string(args, "path")?;
                let before = self.workspace.snapshot(path)?;
                self.check_observed(path, &before, args["expected_hash"].as_str(), false)?;
                let text = std::str::from_utf8(before.bytes.as_deref().context("File not found")?)?;
                let updated = if let Some(old) = args["old_string"].as_str() {
                    ensure!(!old.is_empty(), "old_string must not be empty");
                    let new = string(args, "new_string")?;
                    let count = text.matches(old).count();
                    ensure!(count > 0, "old_string not found");
                    ensure!(
                        count == 1 || args["replace_all"] == true,
                        "old_string is ambiguous; include more context"
                    );
                    if args["replace_all"] == true {
                        text.replace(old, new)
                    } else {
                        text.replacen(old, new, 1)
                    }
                } else {
                    self.check_observed(path, &before, args["expected_hash"].as_str(), true)?;
                    edit_line_hunks(
                        text,
                        args["hunks"]
                            .as_array()
                            .context("Provide old_string/new_string or hunks")?,
                    )?
                };
                self.commit_changes(vec![Change {
                    path: path.into(),
                    mode: before.mode,
                    before,
                    after: Some(updated.into_bytes()),
                }])
            }
            "apply_patch" => {
                let patch = args["patch"]
                    .as_str()
                    .or_else(|| args["diff"].as_str())
                    .context("patch is required")?;
                let patch = if patch.starts_with("@@") {
                    let path = string(args, "path")?;
                    format!("--- a/{path}\n+++ b/{path}\n{patch}")
                } else {
                    patch.to_owned()
                };
                self.commit_changes(patch::prepare(&self.workspace, &patch)?)
            }
            "create_directory" => {
                let path = string(args, "path")?;
                self.workspace.mkdir(path)?;
                Ok(json!({"path":path,"created":true,"checkpointed":false}))
            }
            "delete_file" => {
                let path = string(args, "path")?;
                let before = self.workspace.snapshot(path)?;
                ensure!(before.bytes.is_some(), "File not found");
                self.check_observed(path, &before, args["expected_hash"].as_str(), false)?;
                self.commit_changes(vec![Change {
                    path: path.into(),
                    mode: before.mode,
                    before,
                    after: None,
                }])
            }
            "move_file" => {
                let src = string(args, "src")?;
                let dest = string(args, "dest")?;
                let before = self.workspace.snapshot(src)?;
                let bytes = before.bytes.clone().context("Source file does not exist")?;
                let target = self.workspace.snapshot(dest)?;
                ensure!(target.bytes.is_none(), "Destination already exists");
                self.check_observed(src, &before, None, false)?;
                self.commit_changes(vec![
                    Change {
                        path: dest.into(),
                        mode: before.mode,
                        before: target,
                        after: Some(bytes),
                    },
                    Change {
                        path: src.into(),
                        mode: before.mode,
                        before,
                        after: None,
                    },
                ])
            }
            "update_plan" | "update_todos" => {
                let steps = args["steps"]
                    .as_array()
                    .or_else(|| args["todos"].as_array())
                    .context("steps must be an array")?;
                ensure!(steps.len() <= 50, "Plan exceeds 50 steps");
                let mut normalized = Vec::new();
                let mut active = 0;
                for (i, step) in steps.iter().enumerate() {
                    let title = string(step, "title")?;
                    ensure!(
                        !title.is_empty() && title.len() <= 1000,
                        "Invalid step title"
                    );
                    let status = step["status"].as_str().unwrap_or("pending");
                    let status = match status {
                        "completed" | "done" => "done",
                        "in_progress" | "running" => {
                            active += 1;
                            "running"
                        }
                        "pending" | "blocked" | "failed" => status,
                        _ => bail!("Invalid plan status"),
                    };
                    normalized.push(json!({"id":step["id"].as_str().map(str::to_owned).unwrap_or_else(||format!("step-{}",i+1)),"title":title,"status":status,"detail":truncate(step["detail"].as_str().unwrap_or(""),2000)}));
                }
                ensure!(active <= 1, "Only one plan step can be in progress");
                let plan = json!({"goal":truncate(args["goal"].as_str().unwrap_or(""),2000),"steps":normalized});
                self.events.emit("plan.updated", json!({"plan":plan}))?;
                *self
                    .plan
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Plan lock poisoned"))? = plan.clone();
                Ok(plan)
            }
            _ => bail!("Unknown tool: {name}"),
        }
    }
    fn check_observed(
        &self,
        path: &str,
        before: &Snapshot,
        explicit: Option<&str>,
        required: bool,
    ) -> Result<()> {
        if let Some(hash) = explicit {
            ensure!(
                hash == "missing" || (hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())),
                "expected_hash must be 'missing' for a new file or the current SHA-256 hash returned by read_file. Omit it to use this task's recorded read; do not pass an empty string"
            );
        }
        let path = self
            .workspace
            .relative(path)?
            .to_string_lossy()
            .into_owned();
        let reads = self
            .observed
            .lock()
            .map_err(|_| anyhow::anyhow!("Read history lock poisoned"))?;
        let expected = explicit.or_else(|| reads.get(&path).map(String::as_str));
        if let Some(expected) = expected {
            ensure!(
                before.hash.as_deref().unwrap_or("missing") == expected,
                "File changed after it was read; read it again before editing"
            );
        } else {
            ensure!(
                !required || before.bytes.is_none(),
                "Read this file before replacing it, or supply its expected_hash"
            );
        }
        Ok(())
    }
    fn commit_changes(&self, changes: Vec<Change>) -> Result<Value> {
        let mut paths = std::collections::HashSet::new();
        for change in &changes {
            let normalized = self.workspace.writable(&change.path)?;
            ensure!(paths.insert(normalized), "Duplicate file in change set");
            ensure!(
                change
                    .after
                    .as_ref()
                    .is_none_or(|b| b.len() <= crate::workspace::MAX_FILE_BYTES),
                "File exceeds 4 MB"
            );
            ensure!(
                self.workspace.snapshot(&change.path)?.hash == change.before.hash,
                "File changed during patch preparation: {}",
                change.path
            );
        }
        // Journal every intended mutation before touching any file.
        for change in &changes {
            checkpoint::record(
                &self.events.store,
                &self.workspace,
                &self.events.task_id,
                &change.path,
                &change.before,
                change.after.as_deref(),
            )?;
        }
        let mut changed = Vec::new();
        for change in changes {
            ensure!(
                !self.cancel.is_cancelled(),
                "Task cancelled; applied files remain checkpointed: {changed:?}"
            );
            let expected = change.before.hash.as_deref().unwrap_or("missing");
            let result = match &change.after {
                Some(bytes) => self
                    .workspace
                    .write(&change.path, bytes, Some(expected))
                    .and_then(|_| {
                        if let Some(mode) = change.mode {
                            self.workspace.set_mode(&change.path, mode)
                        } else {
                            Ok(())
                        }
                    }),
                None => self.workspace.delete(&change.path, Some(expected)),
            };
            result.with_context(|| {
                format!(
                    "Failed to update {}; already applied files are checkpointed: {changed:?}",
                    change.path
                )
            })?;
            let path = self
                .workspace
                .relative(&change.path)?
                .to_string_lossy()
                .into_owned();
            self.observed
                .lock()
                .map_err(|_| anyhow::anyhow!("Read history lock poisoned"))?
                .insert(
                    path.clone(),
                    change
                        .after
                        .as_deref()
                        .map(hash)
                        .unwrap_or_else(|| "missing".into()),
                );
            let _ = crate::symbol_index::touch(&self.workspace.path, &path);
            changed.push(path);
        }
        self.events.emit(
            "checkpoint.updated",
            checkpoint::summary(&self.events.store, &self.workspace, &self.events.task_id)?,
        )?;
        let mut payload = json!({"paths":changed.clone(),"checkpointed":true});
        if let Some(first) = changed.first() {
            if let Some(callers) = callers_note(&self.workspace.path, first, first) {
                payload["callers"] = callers;
            }
        }
        Ok(payload)
    }
}
fn edit_line_hunks(text: &str, hunks: &[Value]) -> Result<String> {
    ensure!(
        !hunks.is_empty() && hunks.len() <= 128,
        "Provide 1 to 128 line hunks"
    );
    let mut lines: Vec<String> = text.split_inclusive('\n').map(str::to_owned).collect();
    let total = lines.len();
    let mut edits = Vec::new();
    for hunk in hunks {
        let start = integer(hunk, "start_line", 0, 1, total + 1)?;
        let end = integer(hunk, "end_line", 0, 0, total)?;
        ensure!(end >= start - 1, "Invalid line range");
        edits.push((start - 1, end, string(hunk, "replacement")?.to_owned()));
    }
    edits.sort_by_key(|e| e.0);
    for pair in edits.windows(2) {
        ensure!(
            pair[0].1 <= pair[1].0 && pair[0].0 != pair[1].0,
            "Line hunks overlap"
        );
    }
    for (start, end, replacement) in edits.into_iter().rev() {
        lines.splice(start..end, [replacement]);
    }
    Ok(lines.concat())
}

fn callers_note(root: &std::path::Path, path: &str, content_hint: &str) -> Option<Value> {
    // Best-effort: if the edited region looks like a Rust/TS function name, list callers.
    let name = content_hint
        .lines()
        .find_map(|line| {
            let line = line.trim();
            if let Some(rest) = line
                .strip_prefix("fn ")
                .or_else(|| line.strip_prefix("pub fn "))
            {
                Some(
                    rest.split(|c: char| c == '(' || c.is_whitespace())
                        .next()?
                        .to_owned(),
                )
            } else if let Some(rest) = line.strip_prefix("function ") {
                Some(
                    rest.split(|c: char| c == '(' || c.is_whitespace())
                        .next()?
                        .to_owned(),
                )
            } else {
                None
            }
        })
        .or_else(|| {
            std::path::Path::new(path)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
        })?;
    if name.is_empty() || name.len() > 64 {
        return None;
    }
    crate::intelligence::callers_for_patch(root, &name).ok()
}

fn git_ref(value: &str) -> Result<String> {
    ensure!(
        !value.is_empty()
            && !value.starts_with('-')
            && !value.contains(['\0', '\n', '\r'])
            && value.len() <= 1024,
        "Invalid Git reference"
    );
    Ok(value.into())
}
fn string<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args[key]
        .as_str()
        .with_context(|| format!("{key} must be a string"))
}
fn integer(args: &Value, key: &str, default: usize, min: usize, max: usize) -> Result<usize> {
    let value = match args.get(key) {
        None => default,
        Some(v) => v
            .as_u64()
            .and_then(|v| usize::try_from(v).ok())
            .with_context(|| format!("{key} must be a nonnegative integer"))?,
    };
    ensure!(
        (min..=max).contains(&value),
        "{key} must be between {min} and {max}"
    );
    Ok(value)
}
pub fn truncate(value: &str, max_bytes: usize) -> &str {
    let mut end = value.len().min(max_bytes);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

pub fn schemas() -> Vec<Value> {
    let s = json!({"type":"string"});
    let n = json!({"type":"integer"});
    let b = json!({"type":"boolean"});
    let specs=vec![
        ("system_info","Inspect OS and connected displays. Read-only.",json!({}),vec![]),
        ("list_files","List direct children of a workspace directory.",json!({"path":s,"max_entries":n}),vec![]),
        ("read_file","Read UTF-8 text with 1-based offset/limit. Returns truncated=true and next_offset when the range or byte budget is exceeded; do not assume omitted lines.",json!({"path":s,"offset":n,"limit":n}),vec!["path"]),
        ("search_files","Find filenames by substring; respects ignore rules.",json!({"query":s,"path":s}),vec!["query"]),
        ("search_text","Search text; literal by default, optional regex and file glob.",json!({"query":s,"path":s,"regex":b,"glob":s,"max_hits":n}),vec!["query"]),
        ("search_symbol","Find likely symbol definitions by name (regex heuristic).",json!({"query":s,"path":s}),vec!["query"]),
        ("workspace_symbols","Tree-sitter workspace symbol search for Rust/TypeScript. Bounded; not a vector index.",json!({"query":s,"max_hits":n}),vec!["query"]),
        ("goto_definition","Best-effort tree-sitter definition lookup for a symbol name.",json!({"query":s,"symbol":s,"max_hits":n}),vec![]),
        ("find_references","Tree-sitter identifier references for a symbol name. Not type-aware.",json!({"query":s,"symbol":s,"max_hits":n}),vec![]),
        ("get_type_signature","Parser signature for a symbol when LSP is absent (tree-sitter AST).",json!({"symbol":s,"query":s}),vec![]),
        ("get_diagnostics","One-shot rust-analyzer diagnostics when rust-analyzer is already installed. Does not start a persistent LSP.",json!({"path":s}),vec!["path"]),
        ("mcp_sqlite_tables","List tables and CREATE TABLE definitions in a project SQLite file. Native read-only tool; no registration. SQLite may maintain WAL sidecars.",json!({"path":s}),vec!["path"]),
        ("mcp_sqlite_query","Read a project SQLite file with SELECT/WITH or schema PRAGMA (table_info etc). Bind ? placeholders with params; check truncated. Unique column aliases required. SQLite may maintain WAL sidecars.",json!({"path":s,"sql":s,"params":{"type":"array","items":{"type":["string","number","boolean","null"]}},"limit":n}),vec!["path","sql"]),
        ("background_start","Start a named project server/watcher under shell permissions. Continues independently after the task, including cancellation; stop it when no longer wanted. Inspect status/output before claiming readiness.",json!({"name":s,"command":s}),vec!["name","command"]),
        ("background_list","List this project's active processes and recent results with bounded log previews.",json!({}),vec![]),
        ("background_output","Read a project process's status and retained log tail; max_bytes defaults to 8000, maximum 64000.",json!({"id":s,"max_bytes":n}),vec!["id"]),
        ("background_stop","Stop a managed project process after approval; waits for process-group cleanup. Use its exact listed ID, never a PID.",json!({"id":s}),vec!["id"]),
        ("write_file","Create/replace text. Read existing files first. Optional expected_hash: 'missing' for new files or read_file's SHA-256; omit when unused, never empty.",json!({"path":s,"content":s,"expected_hash":s}),vec!["path","content"]),
        ("edit_file","Replace exact unique text. Set replace_all explicitly for repeated matches.",json!({"path":s,"old_string":s,"new_string":s,"replace_all":b,"expected_hash":s}),vec!["path","old_string","new_string"]),
        ("apply_patch","Apply a unified diff or complete *** Begin Patch block. All file contexts are preflighted; changes are checkpointed.",json!({"patch":s,"path":s}),vec!["patch"]),
        ("create_directory","Create a workspace directory and parents.",json!({"path":s}),vec!["path"]),
        ("move_file","Move a regular file without overwriting its destination; checkpoint both paths.",json!({"src":s,"dest":s}),vec!["src","dest"]),
        ("delete_file","Delete one regular workspace file after approval; retain a checkpoint.",json!({"path":s,"expected_hash":s}),vec!["path"]),
        ("exec","Run a shell command after approval. Uses optional bubblewrap when available; still not a full OS sandbox. Output and runtime are bounded.",json!({"command":s,"cwd":s,"timeout_sec":n}),vec!["command"]),
        ("git_status","Show repository status.",json!({}),vec![]),
        ("git_diff","Show staged or unstaged changes. External diff helpers and textconv are disabled.",json!({"staged":b}),vec![]),
        ("git_log","Show recent commits.",json!({"limit":n}),vec![]),
        ("git_branch","List branches, or create and switch after approval.",json!({"create":b,"name":s}),vec![]),
        ("git_checkout","Switch to a branch after approval; refuses force flags.",json!({"ref":s}),vec!["ref"]),
        ("git_add","Stage exact workspace paths. Does not commit or push.",json!({"paths":{"type":"array","items":s}}),vec!["paths"]),
        ("git_commit","Commit staged changes after approval. Hooks and signing are disabled; no push.",json!({"message":s}),vec!["message"]),
        ("update_plan","Update the visible plan. Only one step may be in progress; mark completed only with evidence.",json!({"goal":s,"steps":{"type":"array","items":{"type":"object","properties":{"id":s,"title":s,"status":{"type":"string","enum":["pending","in_progress","completed","blocked","failed"]},"detail":s},"required":["title","status"]}}}),vec!["steps"]),
    ];
    // Keep the catalog usable for 4K local models; argument names and focused
    // descriptions carry the important contract while full docs remain in source.
    specs.into_iter().map(|(name,description,properties,required)|json!({"type":"function","function":{"name":name,"description":truncate(description, 64),"parameters":{"type":"object","properties":properties,"required":required,"additionalProperties":false}}})).collect()
}
