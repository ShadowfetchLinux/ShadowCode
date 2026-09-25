use anyhow::{ensure, Context, Result};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    process::Stdio,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    sync::mpsc,
};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug)]
pub struct ProcessSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub timeout: Duration,
    pub output_limit: usize,
    pub env: BTreeMap<String, String>,
    /// Sandbox work between fork and exec (network namespace, Landlock), and
    /// the allow-list proxy served while the process runs.
    pub child: Option<crate::sandbox::ChildSetup>,
}
impl ProcessSpec {
    pub fn shell(command: &str, cwd: PathBuf, timeout: Duration) -> Self {
        Self {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), command.into()],
            cwd,
            timeout,
            output_limit: 256_000,
            env: BTreeMap::new(),
            child: None,
        }
    }
    pub fn command(program: &str, args: &[&str], cwd: PathBuf) -> Self {
        Self {
            program: program.into(),
            args: args.iter().map(|s| (*s).into()).collect(),
            cwd,
            timeout: Duration::from_secs(30),
            output_limit: 256_000,
            env: BTreeMap::new(),
            child: None,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ProcessChunk {
    pub stream: &'static str,
    pub text: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct ProcessResult {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub timed_out: bool,
    pub cancelled: bool,
    pub truncated: bool,
    pub duration_ms: u128,
    pub pid: u32,
}

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
        #[cfg(unix)]
        if self.0 != 0 {
            #[cfg(target_os = "linux")]
            let descendants = Self::descendants(self.0);
            unsafe {
                libc::kill(-(self.0 as i32), libc::SIGTERM);
                // Some shells alter their process group while spawning a
                // background job. Signal the leader directly as well so an
                // owned task cannot outlive its cancellation scope.
                libc::kill(self.0 as i32, libc::SIGTERM);
                #[cfg(target_os = "linux")]
                for child in descendants {
                    libc::kill(child as i32, libc::SIGTERM);
                }
            }
        }
    }
    fn kill(&mut self) {
        #[cfg(unix)]
        if self.0 != 0 {
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
        }
        self.0 = 0;
    }
}
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        self.kill();
    }
}

struct DrainTask(tokio::task::JoinHandle<Result<()>>);
impl Drop for DrainTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Drain both pipes even after the output cap is reached; otherwise a verbose
/// child can deadlock its parent or exhaust memory. Cancellation kills the
/// entire process group, including grandchildren that inherited output pipes.
pub async fn run(
    spec: ProcessSpec,
    cancel: CancellationToken,
    chunks: Option<mpsc::Sender<ProcessChunk>>,
) -> Result<ProcessResult> {
    run_internal(spec, cancel, chunks, None).await
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct LiveOutput {
    pub pid: u32,
    pub output: String,
    pub truncated: bool,
    pub revision: u64,
}
#[derive(Clone, Default)]
pub struct ProcessMonitor(Arc<Mutex<LiveOutput>>);
impl ProcessMonitor {
    pub fn snapshot(&self) -> Result<LiveOutput> {
        Ok(self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("Process output lock poisoned"))?
            .clone())
    }
    fn append(&self, text: &str) -> Result<()> {
        let mut state = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("Process output lock poisoned"))?;
        state.output.push_str(text);
        if state.output.len() > 64_000 {
            let mut cut = state.output.len() - 64_000;
            while !state.output.is_char_boundary(cut) {
                cut += 1;
            }
            state.output.drain(..cut);
            state.truncated = true;
        }
        state.revision = state.revision.saturating_add(1);
        Ok(())
    }
}

/// A user-started server or watcher runs until it exits or is stopped, rather
/// than inheriting the one-hour foreground-command timeout. The monitor keeps
/// the latest 64 KB while both pipes continue draining. Stop allows two seconds
/// for SIGTERM, then kills the remaining process group.
pub async fn run_background(
    spec: ProcessSpec,
    cancel: CancellationToken,
    monitor: ProcessMonitor,
) -> Result<ProcessResult> {
    run_internal(spec, cancel, None, Some(monitor)).await
}

async fn run_internal(
    spec: ProcessSpec,
    cancel: CancellationToken,
    chunks: Option<mpsc::Sender<ProcessChunk>>,
    monitor: Option<ProcessMonitor>,
) -> Result<ProcessResult> {
    ensure!(spec.cwd.is_dir(), "Command workspace does not exist");
    ensure!(
        spec.output_limit <= 4_000_000 && spec.output_limit > 0,
        "Invalid command output limit"
    );
    ensure!(
        spec.timeout <= Duration::from_secs(3600) && !spec.timeout.is_zero(),
        "Invalid command timeout"
    );
    ensure!(
        !cancel.is_cancelled(),
        "Task was cancelled before the command started"
    );
    let started = std::time::Instant::now();
    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .current_dir(&spec.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .env_clear();
    for name in [
        "PATH",
        "HOME",
        "USER",
        "LANG",
        "LC_ALL",
        "TERM",
        "TMPDIR",
        "VIRTUAL_ENV",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("PAGER", "cat")
        .env("NO_COLOR", "1")
        .envs(&spec.env);
    #[cfg(unix)]
    command.process_group(0);
    #[cfg(target_os = "linux")]
    let setup = spec.child.clone();
    #[cfg(target_os = "linux")]
    unsafe {
        command.pre_exec(move || {
            // Propagate cancellation to shell-created descendants when their
            // owning process exits, preventing orphaned local-model tasks.
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if let Some(setup) = &setup {
                setup.enter()?;
            }
            Ok(())
        });
    }
    let mut child = command
        .spawn()
        .with_context(|| format!("Could not start {}", spec.program))?;
    let pid = child.id().context("Command has no process ID")?;
    let mut group = ProcessGroup(pid);
    // Dropped (stopping the proxy) when this function returns.
    let _proxy = match &spec.child {
        Some(setup) => setup.after_spawn()?,
        None => None,
    };
    if let Some(monitor) = &monitor {
        let mut state = monitor
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("Process output lock poisoned"))?;
        state.pid = pid;
        state.revision += 1;
    }
    let used = Arc::new(AtomicUsize::new(0));
    let output = Arc::new(Mutex::new(Vec::new()));
    let errors = Arc::new(Mutex::new(Vec::new()));
    let mut stdout = DrainTask(tokio::spawn(drain(
        child.stdout.take().context("Missing stdout")?,
        "stdout",
        spec.output_limit,
        used.clone(),
        chunks.clone(),
        output.clone(),
        monitor.clone(),
    )));
    let mut stderr = DrainTask(tokio::spawn(drain(
        child.stderr.take().context("Missing stderr")?,
        "stderr",
        spec.output_limit,
        used.clone(),
        chunks,
        errors.clone(),
        monitor.clone(),
    )));
    let deadline = async {
        if monitor.is_some() {
            std::future::pending::<()>().await;
        } else {
            tokio::time::sleep(spec.timeout).await;
        }
    };
    let (status, timed_out, cancelled) = tokio::select! {
        status=child.wait()=>(Some(status?),false,false),
        _=deadline=>(None,true,false),
        _=cancel.cancelled()=>(None,false,true),
    };
    let status = if cancelled {
        group.terminate();
        tokio::time::timeout(Duration::from_secs(2), child.wait())
            .await
            .ok()
            .transpose()?
            .or(status)
    } else {
        status
    };
    group.kill();
    let status = match status {
        Some(status) => status,
        None => tokio::time::timeout(Duration::from_secs(3), child.wait())
            .await
            .context("Killed process did not exit")??,
    };
    // A deliberately detached process may retain the pipes after its shell
    // exits. Never let that keep the task or application alive indefinitely.
    let drained = tokio::time::timeout(Duration::from_secs(1), async {
        let (out, err) = tokio::join!(&mut stdout.0, &mut stderr.0);
        out??;
        err??;
        Ok::<_, anyhow::Error>(())
    })
    .await;
    let incomplete = drained.is_err();
    if let Ok(result) = drained {
        result?;
    }
    let stdout = output
        .lock()
        .map_err(|_| anyhow::anyhow!("Output lock poisoned"))?
        .clone();
    let stderr = errors
        .lock()
        .map_err(|_| anyhow::anyhow!("Output lock poisoned"))?
        .clone();
    Ok(ProcessResult {
        ok: status.success() && !timed_out && !cancelled,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        exit_code: status.code().unwrap_or(-1),
        timed_out,
        cancelled,
        truncated: incomplete || used.load(Ordering::Relaxed) > spec.output_limit,
        duration_ms: started.elapsed().as_millis(),
        pid,
    })
}

async fn drain(
    mut reader: impl AsyncRead + Unpin,
    stream: &'static str,
    limit: usize,
    used: Arc<AtomicUsize>,
    chunks: Option<mpsc::Sender<ProcessChunk>>,
    result: Arc<Mutex<Vec<u8>>>,
    monitor: Option<ProcessMonitor>,
) -> Result<()> {
    let mut buffer = [0_u8; 8192];
    let mut pending = Vec::new();
    loop {
        let count = reader.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        if let Some(monitor) = &monitor {
            pending.extend_from_slice(&buffer[..count]);
            let mut consumed = 0;
            while consumed < pending.len() {
                match std::str::from_utf8(&pending[consumed..]) {
                    Ok(text) => {
                        monitor.append(text)?;
                        consumed = pending.len();
                    }
                    Err(error) => {
                        let valid = error.valid_up_to();
                        monitor
                            .append(std::str::from_utf8(&pending[consumed..consumed + valid])?)?;
                        consumed += valid;
                        if let Some(invalid) = error.error_len() {
                            monitor.append("\u{fffd}")?;
                            consumed += invalid;
                        } else {
                            break;
                        }
                    }
                }
            }
            pending.drain(..consumed);
        }
        let previous = used.fetch_add(count, Ordering::Relaxed);
        let keep = count.min(limit.saturating_sub(previous));
        if keep > 0 {
            result
                .lock()
                .map_err(|_| anyhow::anyhow!("Output lock poisoned"))?
                .extend_from_slice(&buffer[..keep]);
            if let Some(chunks) = &chunks {
                let _ = chunks.try_send(ProcessChunk {
                    stream,
                    text: String::from_utf8_lossy(&buffer[..keep]).into_owned(),
                });
            }
        }
    }
    if let Some(monitor) = monitor {
        if !pending.is_empty() {
            monitor.append(&String::from_utf8_lossy(&pending))?;
        }
    }
    Ok(())
}
