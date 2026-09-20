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
    fn kill(&mut self) {
        #[cfg(unix)]
        if self.0 != 0 {
            unsafe {
                libc::kill(-(self.0 as i32), libc::SIGKILL);
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
    let mut child = command
        .spawn()
        .with_context(|| format!("Could not start {}", spec.program))?;
    let pid = child.id().context("Command has no process ID")?;
    let mut group = ProcessGroup(pid);
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
    )));
    let mut stderr = DrainTask(tokio::spawn(drain(
        child.stderr.take().context("Missing stderr")?,
        "stderr",
        spec.output_limit,
        used.clone(),
        chunks,
        errors.clone(),
    )));
    let (status, timed_out, cancelled) = tokio::select! {
        status=child.wait()=>(Some(status?),false,false),
        _=tokio::time::sleep(spec.timeout)=>(None,true,false),
        _=cancel.cancelled()=>(None,false,true),
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
) -> Result<()> {
    let mut buffer = [0_u8; 8192];
    loop {
        let count = reader.read(&mut buffer).await?;
        if count == 0 {
            break;
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
    Ok(())
}
