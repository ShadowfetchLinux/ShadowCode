//! Native MCP transport. Callers must obtain explicit server activation and
//! tool approval before using this low-level client; discovery never starts it.
//! The application-facing registration/approval layer is a separate component.
use anyhow::{anyhow, bail, ensure, Context, Result};
use rmcp::{
    model::{
        CallToolRequestParams, CallToolResponse, ClientConfig, Implementation,
        PaginatedRequestParams, Tool,
    },
    service::{ClientCacheConfig, RunningService, RxJsonRpcMessage, TxJsonRpcMessage},
    transport::Transport,
    RoleClient, ServiceExt,
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashSet},
    future::Future,
    io,
    path::Path,
    process::Stdio,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{ChildStdin, ChildStdout, Command},
    sync::Semaphore,
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

pub const FRAME_LIMIT: usize = 1_048_576;
const TOTAL_LIMIT: usize = 32 * FRAME_LIMIT;
const STDERR_LIMIT: usize = 16_384;
const CATALOG_LIMIT: usize = 2 * FRAME_LIMIT;
static CONNECTIONS: OnceLock<Arc<Semaphore>> = OnceLock::new();
pub mod registry;
pub mod runner;

#[derive(Clone, Debug)]
pub struct StdioSpec {
    /// Executable followed by literal arguments; no shell interpolation.
    pub command: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub timeout: Duration,
}
impl StdioSpec {
    fn validate(&self) -> Result<()> {
        ensure!(
            !self.command.is_empty() && self.command.len() <= 128,
            "MCP command needs 1–128 arguments"
        );
        ensure!(!self.command[0].is_empty(), "MCP executable is empty");
        ensure!(
            self.command.iter().all(|s| !s.contains('\0'))
                && self.command.iter().map(String::len).sum::<usize>() <= 32_000,
            "Invalid or oversized MCP command"
        );
        ensure!(
            self.env.len() <= 64
                && self
                    .env
                    .iter()
                    .all(|(k, v)| crate::config::valid_secret_name(k)
                        && !v.contains('\0')
                        && v.len() <= 16_000),
            "Invalid MCP environment"
        );
        ensure!(
            self.timeout >= Duration::from_millis(100) && self.timeout <= Duration::from_secs(120),
            "MCP timeout must be 0.1–120 seconds"
        );
        Ok(())
    }
}

#[derive(Clone, Default)]
struct Diagnostics {
    error: Arc<Mutex<Option<String>>>,
    stderr: Arc<Mutex<Vec<u8>>>,
}
impl Diagnostics {
    fn fail(&self, message: &str) {
        if let Ok(mut error) = self.error.lock() {
            if error.is_none() {
                *error = Some(message.into());
            }
        }
    }
    fn error(&self) -> Option<String> {
        self.error.lock().ok().and_then(|e| e.clone())
    }
}

struct BoundedStdio {
    reader: BufReader<ChildStdout>,
    writer: Arc<tokio::sync::Mutex<Option<ChildStdin>>>,
    line: Vec<u8>,
    total: usize,
    window: Instant,
    frames: usize,
    diagnostics: Diagnostics,
    cancel: CancellationToken,
}
impl BoundedStdio {
    fn stop(&self, message: &str) {
        self.diagnostics.fail(message);
        self.cancel.cancel();
    }
}
impl Transport<RoleClient> for BoundedStdio {
    type Error = io::Error;
    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleClient>,
    ) -> impl Future<Output = io::Result<()>> + Send + 'static {
        let writer = self.writer.clone();
        async move {
            let mut bytes = serde_json::to_vec(&item)?;
            if bytes.len() > FRAME_LIMIT {
                return Err(io::Error::other("MCP outgoing frame exceeds 1 MiB"));
            }
            bytes.push(b'\n');
            let mut guard = writer.lock().await;
            let writer = guard
                .as_mut()
                .ok_or_else(|| io::Error::other("MCP transport closed"))?;
            writer.write_all(&bytes).await?;
            writer.flush().await
        }
    }
    async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleClient>> {
        loop {
            // fill_buf/consume and the persistent line are cancellation-safe:
            // a simultaneous outgoing message cannot discard a partial reply.
            let buffer = match self.reader.fill_buf().await {
                Ok(buffer) => buffer,
                Err(_) => {
                    self.stop("MCP stdout read failed");
                    return None;
                }
            };
            if buffer.is_empty() {
                self.stop(if self.line.is_empty() {
                    "MCP server closed stdout"
                } else {
                    "MCP server ended with an incomplete frame"
                });
                return None;
            }
            let end = buffer.iter().position(|b| *b == b'\n').map(|i| i + 1);
            let count = end.unwrap_or(buffer.len());
            if self.line.len() + count > FRAME_LIMIT || self.total + count > TOTAL_LIMIT {
                self.stop("MCP incoming frame or connection byte limit exceeded");
                return None;
            }
            self.line.extend_from_slice(&buffer[..count]);
            self.total += count;
            self.reader.consume(count);
            if end.is_none() {
                continue;
            }
            if self.window.elapsed() >= Duration::from_secs(1) {
                self.window = Instant::now();
                self.frames = 0;
            }
            self.frames += 1;
            if self.frames > 128 {
                self.stop("MCP incoming frame rate exceeded");
                return None;
            }
            if self.line.iter().all(u8::is_ascii_whitespace) {
                self.line.clear();
                continue;
            }
            let parsed = serde_json::from_slice(&self.line);
            self.line.clear();
            return match parsed {
                Ok(message) => Some(message),
                Err(_) => {
                    self.stop("MCP server sent malformed JSON-RPC");
                    None
                }
            };
        }
    }
    async fn close(&mut self) -> io::Result<()> {
        self.cancel.cancel();
        self.writer.lock().await.take();
        Ok(())
    }
}

struct Group(u32);
impl Group {
    fn signal(&self, signal: i32) {
        if self.0 != 0 {
            unsafe {
                libc::kill(-(self.0 as i32), signal);
            }
        }
    }
    fn kill(&mut self) {
        self.signal(libc::SIGKILL);
        self.0 = 0;
    }
}
impl Drop for Group {
    fn drop(&mut self) {
        self.kill();
    }
}
struct ReaderTask(JoinHandle<()>);
impl Drop for ReaderTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}
struct Owner {
    cancel: CancellationToken,
    task: Option<JoinHandle<Result<()>>>,
    group: Arc<Mutex<Group>>,
}
impl Owner {
    async fn close(&mut self) -> Result<()> {
        self.cancel.cancel();
        if let Some(task) = self.task.take() {
            task.await.context("MCP cleanup worker failed")??;
        }
        Ok(())
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.group.lock().unwrap_or_else(|e| e.into_inner()).kill();
    }
}

pub struct Client {
    service: Option<RunningService<RoleClient, ClientConfig>>,
    owner: Owner,
    diagnostics: Diagnostics,
    timeout: Duration,
    tools: Vec<Tool>,
    pid: u32,
}
impl Client {
    /// Starts an explicitly authorized command in a canonical project. It does
    /// not read configuration, auto-discover servers, or infer authorization.
    pub async fn connect(
        spec: &StdioSpec,
        workspace: &Path,
        cancel: CancellationToken,
    ) -> Result<Self> {
        spec.validate()?;
        ensure!(!cancel.is_cancelled(), "MCP connection cancelled");
        let permit = CONNECTIONS
            .get_or_init(|| Arc::new(Semaphore::new(16)))
            .clone()
            .try_acquire_owned()
            .context("Native MCP connection limit reached")?;
        let workspace = workspace.canonicalize()?;
        ensure!(workspace.is_dir(), "MCP workspace is not a directory");
        let mut command = Command::new(&spec.command[0]);
        // Do not pass the application's provider credentials, control-socket
        // variables, or dynamic-library overrides to an external integration.
        command.env_clear();
        for key in [
            "PATH",
            "HOME",
            "USER",
            "LOGNAME",
            "LANG",
            "LC_ALL",
            "LC_CTYPE",
            "TMPDIR",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_CACHE_HOME",
            "XDG_STATE_HOME",
            "XDG_RUNTIME_DIR",
        ] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        command
            .args(&spec.command[1..])
            .envs(&spec.env)
            .current_dir(workspace)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .process_group(0);
        let mut child = command.spawn().context("Cannot start MCP server")?;
        let pid = child.id().context("MCP process has no PID")?;
        let group = Arc::new(Mutex::new(Group(pid)));
        let stdin = child.stdin.take().context("MCP stdin is unavailable")?;
        let stdout = child.stdout.take().context("MCP stdout is unavailable")?;
        let mut stderr = child.stderr.take().context("MCP stderr is unavailable")?;
        let diagnostics = Diagnostics::default();
        let stderr_output = diagnostics.stderr.clone();
        let drain = ReaderTask(tokio::spawn(async move {
            let mut buffer = [0; 4096];
            while let Ok(n) = stderr.read(&mut buffer).await {
                if n == 0 {
                    break;
                }
                if let Ok(mut output) = stderr_output.lock() {
                    let count = n.min(STDERR_LIMIT.saturating_sub(output.len()));
                    output.extend_from_slice(&buffer[..count]);
                }
            }
        }));
        let cancel = cancel.child_token();
        let worker_cancel = cancel.clone();
        let worker_group = group.clone();
        let task = tokio::spawn(async move {
            let _permit = permit;
            let mut drain = drain;
            // Keep the leader unreaped until its group has been killed: this
            // reserves the PID and prevents a late signal hitting a reused ID.
            worker_cancel.cancelled().await;
            worker_group
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .signal(libc::SIGTERM);
            tokio::time::sleep(Duration::from_millis(150)).await;
            worker_group
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .kill();
            child.start_kill().ok();
            tokio::time::timeout(Duration::from_secs(2), child.wait())
                .await
                .context("MCP process did not exit after kill")??;
            // Descendants can escape a user-level group; never wait forever on
            // a pipe they retained. The guard aborts any remaining drain task.
            let _ = tokio::time::timeout(Duration::from_millis(200), &mut drain.0).await;
            Ok(())
        });
        let transport = BoundedStdio {
            reader: BufReader::new(stdout),
            writer: Arc::new(tokio::sync::Mutex::new(Some(stdin))),
            line: Vec::new(),
            total: 0,
            window: Instant::now(),
            frames: 0,
            diagnostics: diagnostics.clone(),
            cancel: cancel.clone(),
        };
        let mut client = Self {
            service: None,
            owner: Owner {
                cancel: cancel.clone(),
                task: Some(task),
                group,
            },
            diagnostics,
            timeout: spec.timeout,
            tools: Vec::new(),
            pid,
        };
        let info = ClientConfig::new(
            Default::default(),
            Implementation::new("ShadowCode", crate::VERSION),
        );
        let result = tokio::select! {
            _ = cancel.cancelled() => Err(anyhow!("MCP connection cancelled")),
            result = tokio::time::timeout(spec.timeout, info.serve_with_ct(transport, cancel.clone())) => match result {
                Ok(Ok(service)) => { client.service = Some(service); Ok(()) },
                Ok(Err(_)) => Err(anyhow!("MCP initialization failed")),
                Err(_) => Err(anyhow!("MCP initialization timed out")),
            }
        };
        if let Err(error) = result {
            let reason = client.diagnostics.error();
            client.close().await?;
            return Err(
                error.context(reason.unwrap_or_else(|| "Cannot initialize MCP connection".into()))
            );
        }
        client
            .service
            .as_ref()
            .unwrap()
            .set_response_cache_config(ClientCacheConfig::disabled())
            .await;
        if let Err(error) = client.refresh_tools().await {
            client.close().await?;
            return Err(error);
        }
        Ok(client)
    }
    pub fn tools(&self) -> &[Tool] {
        &self.tools
    }
    pub fn pid(&self) -> u32 {
        self.pid
    }
    pub fn is_closed(&self) -> bool {
        self.service.is_none() || self.owner.cancel.is_cancelled()
    }
    /// Captured stderr is untrusted diagnostic data; callers must redact secrets
    /// before persisting or displaying it. It is not appended to protocol errors.
    pub fn stderr(&self) -> String {
        self.diagnostics
            .stderr
            .lock()
            .map(|s| String::from_utf8_lossy(&s).into_owned())
            .unwrap_or_default()
    }
    pub async fn close(&mut self) -> Result<()> {
        self.owner.cancel.cancel();
        let service_result = if let Some(mut service) = self.service.take() {
            service
                .close_with_timeout(Duration::from_secs(2))
                .await
                .map_err(anyhow::Error::from)
                .and_then(|v| v.ok_or_else(|| anyhow!("MCP protocol shutdown timed out")))
                .map(|_| ())
        } else {
            Ok(())
        };
        self.owner.close().await?;
        service_result
    }
    async fn refresh_tools(&mut self) -> Result<()> {
        let service = self.service.as_ref().context("MCP connection closed")?;
        let deadline = tokio::time::Instant::now() + self.timeout;
        let mut cursor = None;
        let mut cursors = HashSet::new();
        let mut names = HashSet::new();
        let mut tools = Vec::new();
        let mut bytes = 0;
        for _ in 0..8 {
            let mut params = PaginatedRequestParams::default();
            params.cursor = cursor;
            let page = tokio::select! {
                _ = self.owner.cancel.cancelled() => bail!(self.diagnostics.error().unwrap_or_else(|| "MCP catalog cancelled".into())),
                result = tokio::time::timeout_at(deadline, service.list_tools(Some(params))) => result.context("MCP tool discovery timed out")?.context("MCP tool discovery failed")?
            };
            for tool in page.tools {
                ensure!(
                    !tool.name.is_empty()
                        && tool.name.len() <= 256
                        && !tool.name.chars().any(char::is_control),
                    "MCP tool has an invalid name"
                );
                ensure!(
                    names.insert(tool.name.to_string()),
                    "MCP catalog contains duplicate tool names"
                );
                bytes += serde_json::to_vec(&tool)?.len();
                ensure!(
                    tools.len() < 128 && bytes <= CATALOG_LIMIT,
                    "MCP catalog exceeds 128 tools or 2 MiB"
                );
                tools.push(tool);
            }
            cursor = page.next_cursor;
            if let Some(ref next) = cursor {
                ensure!(
                    next.len() <= 4096 && cursors.insert(next.clone()),
                    "MCP pagination cursor is oversized or repeated"
                );
            } else {
                self.tools = tools;
                return Ok(());
            }
        }
        bail!("MCP catalog exceeds eight pages")
    }
    /// The caller must approve the exact server/tool/arguments before calling.
    /// The SDK receives no roots, sampling, elicitation, or task capabilities.
    /// Tool annotations and descriptions never grant execution permission.
    pub async fn call(&mut self, name: &str, arguments: Value) -> Result<Value> {
        ensure!(
            self.tools.iter().any(|tool| tool.name == name),
            "Unknown MCP tool"
        );
        let arguments = arguments
            .as_object()
            .context("MCP tool arguments must be an object")?
            .clone();
        ensure!(
            serde_json::to_vec(&arguments)?.len() <= FRAME_LIMIT / 2,
            "MCP arguments exceed 512 KiB"
        );
        let service = self.service.as_ref().context("MCP connection closed")?;
        let result = tokio::select! {
            _ = self.owner.cancel.cancelled() => Err(anyhow!(self.diagnostics.error().unwrap_or_else(|| "MCP call cancelled".into()))),
            result = tokio::time::timeout(self.timeout, service.call_tool_once(CallToolRequestParams::new(name.to_owned()).with_arguments(arguments))) => match result {
                Ok(Ok(CallToolResponse::Complete(result))) => serde_json::to_value(result).map_err(Into::into),
                Ok(Ok(_)) => Err(anyhow!("MCP server requested unsupported interactive or background-task continuation")),
                Ok(Err(_)) => Err(anyhow!("MCP tool request failed")),
                Err(_) => Err(anyhow!("MCP tool request timed out")),
            }
        };
        let result = result.map_err(|error| match self.diagnostics.error() {
            Some(reason) => error.context(reason),
            None => error,
        });
        if result.is_err() {
            self.close().await?;
        }
        result
    }
}
