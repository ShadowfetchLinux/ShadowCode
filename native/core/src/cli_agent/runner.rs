//! Spawn a vendor CLI in the trusted workspace and translate its stream.
//!
//! The child inherits the user's login environment so the official CLI can
//! read its own credentials. ShadowCode never opens those files, never injects
//! its tools or bubblewrap, and kills the process group on cancel.
use super::{
    adapter_for, clip, redact, resolve_binary, ApprovalPrompt, CliAdapter, CliAgentsConfig,
    LaunchOptions, Update, Vendor, MAX_LINE_BYTES, MAX_MALFORMED_LINES,
};
use crate::{
    approvals::{Approval, ApprovalHub},
    events::TaskEvents,
    models::Usage,
    steering::SteerControl,
};
use anyhow::{anyhow, bail, ensure, Context, Result};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
};
use tokio_util::sync::CancellationToken;

struct ProcessGroup(u32);
impl ProcessGroup {
    #[cfg(target_os = "linux")]
    fn descendants(pid: u32) -> Vec<u32> {
        let path = format!("/proc/{pid}/task/{pid}/children");
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .split_whitespace()
            .filter_map(|value| value.parse().ok())
            .flat_map(|child| {
                let mut all = vec![child];
                all.extend(Self::descendants(child));
                all
            })
            .collect()
    }
    fn terminate(&self) {
        if self.0 == 0 {
            return;
        }
        #[cfg(target_os = "linux")]
        let descendants = Self::descendants(self.0);
        unsafe {
            libc::kill(-(self.0 as i32), libc::SIGTERM);
            libc::kill(self.0 as i32, libc::SIGTERM);
            #[cfg(target_os = "linux")]
            for child in descendants {
                libc::kill(child as i32, libc::SIGTERM);
            }
        }
    }
    fn kill(&mut self) {
        if self.0 == 0 {
            return;
        }
        #[cfg(target_os = "linux")]
        let descendants = Self::descendants(self.0);
        unsafe {
            libc::kill(-(self.0 as i32), libc::SIGKILL);
            libc::kill(self.0 as i32, libc::SIGKILL);
            #[cfg(target_os = "linux")]
            for child in descendants {
                libc::kill(child as i32, libc::SIGKILL);
            }
        }
        self.0 = 0;
    }
}
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Inputs for one vendor-agent task.
pub struct Request<'a> {
    pub vendor: Vendor,
    pub options: LaunchOptions,
    pub config: &'a CliAgentsConfig,
    pub prompt: String,
    pub session_id: String,
    pub task_id: String,
    pub job_id: String,
    pub events: &'a TaskEvents,
    pub approvals: &'a ApprovalHub,
    pub cancel: CancellationToken,
    pub steer: &'a SteerControl,
}

/// Run the vendor CLI until the turn finishes, fails, or is cancelled.
pub async fn run(request: Request<'_>) -> Result<(String, Usage)> {
    let mut fallback = request.vendor == Vendor::Codex
        && !codex_app_server_available(&request.options.binary).await;
    let mut last_error = None;
    for _ in 0..2 {
        match run_once(request.vendor, fallback, &request).await {
            Ok(result) => return Ok(result),
            Err(error)
                if request.vendor == Vendor::Codex
                    && !fallback
                    && looks_like_missing_app_server(&error) =>
            {
                last_error = Some(error);
                fallback = true;
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow!("Codex app-server and exec fallback both failed")))
}

fn looks_like_missing_app_server(error: &anyhow::Error) -> bool {
    let text = format!("{error:#}").to_ascii_lowercase();
    ["unknown", "unrecognized", "not found", "no such", "invalid command"]
        .iter()
        .any(|needle| text.contains(needle))
}

/// `codex app-server` is used when help mentions it; otherwise exec fallback.
pub async fn codex_app_server_available(binary: &str) -> bool {
    let help = short_output(binary, &["--help"]).await.unwrap_or_default();
    if help.contains("app-server") {
        return true;
    }
    match short_output(binary, &["app-server", "--help"]).await {
        Some(text) => {
            let lower = text.to_ascii_lowercase();
            !["unknown", "unrecognized", "no such", "not found"]
                .iter()
                .any(|needle| lower.contains(needle))
        }
        None => false,
    }
}

async fn short_output(binary: &str, args: &[&str]) -> Option<String> {
    let mut command = Command::new(binary);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(5), command.output())
        .await
        .ok()?
        .ok()?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    Some(text)
}

async fn run_once(
    vendor: Vendor,
    codex_exec_fallback: bool,
    request: &Request<'_>,
) -> Result<(String, Usage)> {
    ensure_ready(vendor, request)?;
    let mut adapter = adapter_for(vendor, codex_exec_fallback);
    let (program, args) = adapter.command(&request.options);
    let mut child = spawn_vendor(&program, &args, &request.options.workspace)?;
    let pid = child.id().context("Vendor CLI has no process ID")?;
    let mut group = ProcessGroup(pid);
    let mut stdin = child.stdin.take().context("Vendor CLI stdin missing")?;
    let stdout = child.stdout.take().context("Vendor CLI stdout missing")?;
    let stderr = child.stderr.take().context("Vendor CLI stderr missing")?;
    let mut reader = BufReader::new(stdout);
    tokio::spawn(drain_stderr(stderr, request.events.clone()));
    let mut outgoing = adapter.on_start(&request.options);
    outgoing.extend(adapter.prompt(&request.prompt)?);
    send_lines(&mut stdin, &outgoing).await?;
    let mut collected = String::new();
    let mut usage = Usage::default();
    let mut malformed = 0usize;
    let mut last_line = Instant::now();
    let stall = Duration::from_secs(request.config.stall_timeout_sec.max(30));
    let mut message_id = crate::id();
    let mut pending_text = String::new();
    let mut interrupted_turn = false;
    let mut finished = false;
    let mut final_text = None;
    loop {
        if request.cancel.is_cancelled() {
            send_lines(&mut stdin, &adapter.interrupt()).await.ok();
            group.terminate();
            let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
            group.kill();
            flush_text(request, &message_id, &mut pending_text)?;
            bail!("Task cancelled. The vendor CLI was stopped; its file changes remain on disk.");
        }
        if request.steer.is_paused() && !interrupted_turn && !finished {
            send_lines(&mut stdin, &adapter.interrupt()).await.ok();
            interrupted_turn = true;
        }
        let remaining = stall.saturating_sub(last_line.elapsed());
        let read = tokio::time::timeout(remaining.min(Duration::from_millis(250)), read_line(&mut reader));
        match read.await {
            Ok(Ok(None)) => {
                flush_text(request, &message_id, &mut pending_text)?;
                let status = tokio::time::timeout(Duration::from_secs(3), child.wait())
                    .await
                    .ok()
                    .and_then(Result::ok);
                group.kill();
                if finished {
                    break;
                }
                let code = status.and_then(|s| s.code()).unwrap_or(-1);
                bail!(
                    "{} exited before finishing the turn (status {code})",
                    vendor.label()
                );
            }
            Ok(Ok(Some(line))) => {
                last_line = Instant::now();
                if line.len() > MAX_LINE_BYTES {
                    malformed += 1;
                    request.events.emit(
                        "agent.warning",
                        json!({"text":format!("Ignored an oversized line from {}", vendor.id())}),
                    )?;
                    ensure!(
                        malformed <= MAX_MALFORMED_LINES,
                        "{} sent too many malformed lines",
                        vendor.label()
                    );
                    continue;
                }
                let step = adapter.on_line(&line)?;
                send_lines(&mut stdin, &step.send).await?;
                let mut saw_protocol = false;
                for update in step.updates {
                    match update {
                        Update::Warning(text) => {
                            if text.contains("Ignored a non-JSON")
                                || text.contains("Ignored a non-object")
                                || text.contains("without method or id")
                                || text.contains("without type")
                            {
                                malformed += 1;
                                ensure!(
                                    malformed <= MAX_MALFORMED_LINES,
                                    "{} sent too many malformed lines",
                                    vendor.label()
                                );
                            } else {
                                malformed = 0;
                            }
                            request
                                .events
                                .emit("agent.warning", json!({"text":text}))?;
                        }
                        other => {
                            malformed = 0;
                            saw_protocol = true;
                            apply_update(
                                request,
                                vendor,
                                other,
                                &mut collected,
                                &mut usage,
                                &mut message_id,
                                &mut pending_text,
                                &mut finished,
                                &mut final_text,
                                &mut stdin,
                                &mut adapter,
                            )
                            .await?;
                        }
                    }
                }
                if saw_protocol {
                    malformed = 0;
                }
                if finished {
                    if adapter.one_shot() {
                        let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
                    }
                    group.kill();
                    break;
                }
            }
            Ok(Err(error)) => {
                group.kill();
                return Err(error);
            }
            Err(_) if last_line.elapsed() >= stall => {
                group.terminate();
                let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
                group.kill();
                bail!(
                    "{} produced no output for {} seconds",
                    vendor.label(),
                    request.config.stall_timeout_sec
                );
            }
            Err(_) => {
                if request.steer.is_paused() && finished {
                    break;
                }
            }
        }
        if finished && request.steer.is_paused() {
            flush_text(request, &message_id, &mut pending_text)?;
            if let Some(follow_up) = wait_for_steer(request).await? {
                finished = false;
                interrupted_turn = false;
                message_id = crate::id();
                outgoing = adapter.prompt(&follow_up)?;
                send_lines(&mut stdin, &outgoing).await?;
            } else {
                group.kill();
                break;
            }
        }
    }
    flush_text(request, &message_id, &mut pending_text)?;
    if let Some(text) = final_text {
        if collected.is_empty() {
            collected = text;
        }
    }
    Ok((collected, usage))
}

fn ensure_ready(vendor: Vendor, request: &Request<'_>) -> Result<()> {
    if !request.config.vendor_enabled(vendor) {
        bail!(
            "{} is disabled in Settings → Advanced (cli_agents)",
            vendor.label()
        );
    }
    if resolve_binary(&request.options.binary).is_none()
        && !Path::new(&request.options.binary).is_file()
    {
        bail!(
            "{} was not found on PATH. {}",
            vendor.binary(),
            vendor.install_hint()
        );
    }
    Ok(())
}

fn spawn_vendor(program: &str, args: &[String], workspace: &Path) -> Result<Child> {
    ensure_workspace(workspace)?;
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(workspace)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .env("NO_COLOR", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("PAGER", "cat");
    // Inherit the user environment so the official CLI can use its own login.
    // Do not wrap in bubblewrap and do not inject ShadowCode tools or secrets.
    #[cfg(unix)]
    command.process_group(0);
    #[cfg(target_os = "linux")]
    unsafe {
        command.pre_exec(|| {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command
        .spawn()
        .with_context(|| format!("Could not start {program}"))
}

fn ensure_workspace(workspace: &Path) -> Result<()> {
    anyhow::ensure!(
        workspace.is_dir(),
        "Vendor CLI workspace does not exist"
    );
    Ok(())
}

async fn send_lines(stdin: &mut ChildStdin, lines: &[String]) -> Result<()> {
    for line in lines {
        stdin.write_all(line.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
    }
    if !lines.is_empty() {
        stdin.flush().await?;
    }
    Ok(())
}

async fn read_line<R: tokio::io::AsyncBufRead + Unpin>(reader: &mut R) -> Result<Option<String>> {
    let mut buf = Vec::new();
    let count = reader.read_until(b'\n', &mut buf).await?;
    if count == 0 {
        return Ok(None);
    }
    if buf.ends_with(b"\n") {
        buf.pop();
        if buf.ends_with(b"\r") {
            buf.pop();
        }
    }
    Ok(Some(String::from_utf8_lossy(&buf).into_owned()))
}

async fn drain_stderr<R: tokio::io::AsyncRead + Unpin + Send + 'static>(
    stderr: R,
    events: TaskEvents,
) {
    let mut reader = BufReader::new(stderr);
    let mut buf = String::new();
    while reader.read_line(&mut buf).await.unwrap_or(0) > 0 {
        let line = redact(buf.trim());
        buf.clear();
        if line.is_empty() {
            continue;
        }
        let _ = events.emit(
            "agent.warning",
            json!({"text":format!("vendor stderr: {}", clip(&line, 400))}),
        );
    }
}

fn flush_text(request: &Request<'_>, message_id: &str, pending: &mut String) -> Result<()> {
    if pending.is_empty() {
        return Ok(());
    }
    request.events.emit(
        "model.stream",
        json!({"text":pending.clone(),"message_id":message_id}),
    )?;
    pending.clear();
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn apply_update(
    request: &Request<'_>,
    vendor: Vendor,
    update: Update,
    collected: &mut String,
    usage: &mut Usage,
    message_id: &mut String,
    pending_text: &mut String,
    finished: &mut bool,
    final_text: &mut Option<String>,
    stdin: &mut ChildStdin,
    adapter: &mut Box<dyn CliAdapter>,
) -> Result<()> {
    match update {
        Update::Text(text) => {
            collected.push_str(&text);
            pending_text.push_str(&text);
            if pending_text.len() >= 4000 {
                flush_text(request, message_id, pending_text)?;
            }
        }
        Update::ToolStarted { id, name, detail } => {
            flush_text(request, message_id, pending_text)?;
            request.events.emit(
                "tool.started",
                json!({"tool":name,"call_id":id,"arguments":detail}),
            )?;
        }
        Update::ToolCompleted {
            id,
            name,
            success,
            output,
        } => {
            request.events.emit(
                "tool.completed",
                json!({
                    "tool":name,
                    "call_id":id,
                    "success":success,
                    "output":output,
                    "output_preview":crate::tools::truncate(&output.to_string(), 2000),
                    "error":""
                }),
            )?;
        }
        Update::FilesChanged { paths, detail } => {
            request.events.emit(
                "files.changed",
                json!({"paths":paths,"detail":detail,"vendor":vendor.id()}),
            )?;
        }
        Update::Approval(prompt) => {
            flush_text(request, message_id, pending_text)?;
            let approved = request_approval(request, prompt.clone()).await?;
            send_lines(stdin, &adapter.approve(&prompt.request_id, approved)?).await?;
        }
        Update::Warning(text) => {
            request
                .events
                .emit("agent.warning", json!({"text":text}))?;
        }
        Update::Usage {
            input,
            output: completion,
        } => {
            usage.prompt_tokens = usage.prompt_tokens.saturating_add(input);
            usage.completion_tokens = usage.completion_tokens.saturating_add(completion);
            usage.total_tokens = usage.prompt_tokens + usage.completion_tokens;
        }
        Update::TurnCompleted { text, interrupted } => {
            flush_text(request, message_id, pending_text)?;
            if let Some(text) = text {
                if !text.is_empty() {
                    collected.push_str(&text);
                    *final_text = Some(text);
                }
            }
            if interrupted && request.steer.is_paused() {
                if let Some(follow_up) = wait_for_steer(request).await? {
                    *message_id = crate::id();
                    send_lines(stdin, &adapter.prompt(&follow_up)?).await?;
                    return Ok(());
                }
            }
            *finished = true;
        }
        Update::TurnFailed(error) => {
            flush_text(request, message_id, pending_text)?;
            bail!("{error}");
        }
    }
    Ok(())
}

async fn request_approval(request: &Request<'_>, prompt: ApprovalPrompt) -> Result<bool> {
    let record = Approval {
        id: String::new(),
        session_id: request.session_id.clone(),
        task_id: request.task_id.clone(),
        tool: prompt.tool,
        arguments: prompt.arguments,
        command: prompt.command,
        reason: prompt.reason,
        pending: true,
        created_at: 0.0,
        expires_at: 0.0,
    };
    let mut pending_error = None;
    let allowed = request
        .approvals
        .request(
            record,
            Duration::from_secs(request.config.approval_timeout_sec),
            request.cancel.clone(),
            |record| {
                if let Err(error) = request.events.emit("approval.requested", json!(record)) {
                    pending_error = Some(error);
                    request.cancel.cancel();
                }
            },
        )
        .await?;
    if let Some(error) = pending_error {
        return Err(error);
    }
    request.events.emit(
        "approval.resolved",
        json!({"tool":"vendor","approved":allowed,"job_id":request.job_id}),
    )?;
    Ok(allowed)
}

async fn wait_for_steer(request: &Request<'_>) -> Result<Option<String>> {
    if !request.steer.is_paused() {
        return Ok(None);
    }
    let _parked = request.steer.park()?;
    request.events.emit(
        "agent.paused",
        json!({"job_id":request.job_id,"status":"paused","vendor_agent":true}),
    )?;
    while request.steer.is_paused() {
        tokio::select! {
            _ = request.steer.notify().notified() => {}
            _ = request.cancel.cancelled() => {
                bail!("Task cancelled while paused");
            }
        }
    }
    let note = request
        .steer
        .consume_resume(&std::collections::BTreeMap::new())?;
    if let Some(note) = &note {
        request.events.emit(
            "agent.steered",
            json!({"job_id":request.job_id,"note":crate::tools::truncate(note, 2000)}),
        )?;
    }
    Ok(note)
}

/// Public probe used by tests and doctor-style UI.
pub fn resolve_launch_binary(configured: &str) -> Option<PathBuf> {
    resolve_binary(configured).or_else(|| {
        let path = PathBuf::from(configured);
        path.is_file().then_some(path)
    })
}
