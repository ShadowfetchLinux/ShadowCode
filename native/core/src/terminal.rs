//! The user's own interactive terminals (drawer › Terminal).
//!
//! Each terminal is the user's login shell on a pseudo-terminal, started in a
//! project folder with the user's environment. It runs outside the agent
//! sandbox and needs no approval: only the person at the window types into
//! it. Nothing here is stored in the database or offered to a model. Output
//! lives in a bounded in-memory scrollback that the window reads by byte
//! cursor after a transient `terminal.output` wake-up (never stored, and it
//! carries no output), so a closed and reopened drawer replays what is kept.
//!
//! Terminals belong to the [`Terminals`] hub of one desktop view. Dropping the
//! hub (the window quits or detaches) hangs up every shell it started.
use anyhow::{bail, ensure, Context, Result};
use base64::Engine as _;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::sync::broadcast;

/// Scrollback kept per terminal. Older output is dropped from the front.
pub const SCROLLBACK_BYTES: usize = 512 * 1024;
/// Open terminals per desktop view, across projects.
pub const MAX_TERMINALS: usize = 12;
/// One read returns at most this much output; `more` asks for another read.
pub const READ_LIMIT: usize = 256 * 1024;
/// One write (a keystroke batch or a paste) is at most this large.
pub const WRITE_LIMIT: usize = 64 * 1024;
/// Output wake-ups per terminal are coalesced to at most one per interval;
/// the last chunk of a burst always gets its own.
const WAKE_INTERVAL: Duration = Duration::from_millis(16);

/// How to start a terminal. `shell` overrides the login shell (tests).
#[derive(Clone, Debug, Default)]
pub struct OpenOptions {
    pub cols: u16,
    pub rows: u16,
    pub shell: Option<PathBuf>,
    /// Start the shell as a login shell (reads the user's profile).
    pub login: bool,
}

/// The terminals of one desktop view.
pub struct Terminals {
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    sender: broadcast::Sender<Value>,
}

struct Output {
    bytes: Vec<u8>,
    /// Absolute offset of `bytes[0]` in everything the terminal printed.
    start: u64,
    exit_code: Option<i32>,
    /// The shell exited (or the terminal was closed).
    exited: bool,
}

struct Session {
    id: String,
    workspace: PathBuf,
    number: u32,
    shell: String,
    created: f64,
    pid: i32,
    master: std::fs::File,
    writer: Mutex<()>,
    output: Mutex<Output>,
    size: Mutex<(u16, u16)>,
    closed: AtomicBool,
    sender: broadcast::Sender<Value>,
}

impl Session {
    fn wake(&self, kind: &str) {
        let _ = self
            .sender
            .send(json!({"type": kind, "terminal_id": self.id, "session_id": null}));
    }
    fn output(&self) -> Result<std::sync::MutexGuard<'_, Output>> {
        self.output
            .lock()
            .map_err(|_| anyhow::anyhow!("Terminal output lock poisoned"))
    }
    fn append(&self, chunk: &[u8]) {
        let Ok(mut output) = self.output() else {
            return;
        };
        output.bytes.extend_from_slice(chunk);
        if output.bytes.len() > SCROLLBACK_BYTES {
            // Keep three quarters, cut after a line break when one is near so
            // a replay rarely starts inside an escape sequence.
            let mut cut = output.bytes.len() - SCROLLBACK_BYTES * 3 / 4;
            if let Some(newline) = output.bytes[cut..]
                .iter()
                .take(4096)
                .position(|b| *b == b'\n')
            {
                cut += newline + 1;
            }
            output.bytes.drain(..cut);
            output.start += cut as u64;
        }
    }
    fn describe(&self) -> Value {
        let (cols, rows) = self.size.lock().map(|s| *s).unwrap_or((80, 24));
        let (exited, exit_code, end) = self
            .output()
            .map(|o| (o.exited, o.exit_code, o.start + o.bytes.len() as u64))
            .unwrap_or((true, None, 0));
        json!({
            "id": self.id,
            "title": format!("Terminal {}", self.number),
            "number": self.number,
            "workspace": self.workspace,
            "shell": self.shell,
            "cols": cols,
            "rows": rows,
            "created": self.created,
            "exited": exited,
            "exit_code": exit_code,
            "cursor": end,
        })
    }
    fn exited(&self) -> bool {
        self.output().map(|o| o.exited).unwrap_or(true)
    }
    /// Signal the shell's session. The shell itself is signalled only while
    /// it has not been reaped, so a reused process id is never hit.
    fn signal(&self, signal: libc::c_int) {
        signal_session(self.pid, signal, !self.exited());
    }
    /// Hang up the shell and its jobs; force them after `grace`.
    fn hang_up(self: &Arc<Self>, grace: Duration) {
        self.closed.store(true, Ordering::Release);
        self.signal(libc::SIGHUP);
        let session = self.clone();
        std::thread::spawn(move || {
            let deadline = Instant::now() + grace;
            while Instant::now() < deadline && !session.exited() {
                std::thread::sleep(Duration::from_millis(25));
            }
            // Jobs the shell left behind in its session go too.
            session.signal(libc::SIGKILL);
        });
    }
}

impl Terminals {
    pub fn new(sender: broadcast::Sender<Value>) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            sender,
        }
    }
    fn sessions(&self) -> Result<std::sync::MutexGuard<'_, HashMap<String, Arc<Session>>>> {
        self.sessions
            .lock()
            .map_err(|_| anyhow::anyhow!("Terminal registry lock poisoned"))
    }
    fn get(&self, id: &str) -> Result<Arc<Session>> {
        self.sessions()?
            .get(id)
            .cloned()
            .context("This terminal was closed")
    }
    /// Start a shell in `workspace` and return its description.
    pub fn open(&self, workspace: &Path, options: OpenOptions) -> Result<Value> {
        ensure!(workspace.is_dir(), "The project folder does not exist");
        let mut sessions = self.sessions()?;
        ensure!(
            sessions.len() < MAX_TERMINALS,
            "At most {MAX_TERMINALS} terminals can be open. Close one first."
        );
        let used: Vec<u32> = sessions
            .values()
            .filter(|s| s.workspace == workspace)
            .map(|s| s.number)
            .collect();
        let number = (1..).find(|n| !used.contains(n)).unwrap_or(1);
        let (cols, rows) = clamp_size(options.cols, options.rows);
        let shell = options.shell.unwrap_or_else(login_shell);
        let pty = spawn(&shell, options.login, workspace, cols, rows)?;
        let session = Arc::new(Session {
            id: crate::id(),
            workspace: workspace.to_owned(),
            number,
            shell: shell.to_string_lossy().into_owned(),
            created: crate::now(),
            pid: pty.pid,
            master: pty.master,
            writer: Mutex::new(()),
            output: Mutex::new(Output {
                bytes: Vec::new(),
                start: 0,
                exit_code: None,
                exited: false,
            }),
            size: Mutex::new((cols, rows)),
            closed: AtomicBool::new(false),
            sender: self.sender.clone(),
        });
        let reader = session.master.try_clone()?;
        let pump = session.clone();
        std::thread::Builder::new()
            .name("terminal-output".into())
            .spawn(move || pump_output(pump, reader))?;
        let waiter = session.clone();
        let mut child = pty.child;
        std::thread::Builder::new()
            .name("terminal-wait".into())
            .spawn(move || {
                let code = child.wait().ok().map(|status| {
                    use std::os::unix::process::ExitStatusExt;
                    status
                        .code()
                        .unwrap_or_else(|| 128 + status.signal().unwrap_or(0))
                });
                if let Ok(mut output) = waiter.output() {
                    output.exit_code = code;
                    output.exited = true;
                }
                waiter.wake("terminal.exited");
            })?;
        let description = session.describe();
        sessions.insert(session.id.clone(), session);
        Ok(description)
    }
    /// Terminals, oldest first; only those in `workspace` when given.
    pub fn list(&self, workspace: Option<&Path>) -> Result<Vec<Value>> {
        let mut rows: Vec<_> = self
            .sessions()?
            .values()
            .filter(|s| workspace.is_none_or(|w| s.workspace == w))
            .cloned()
            .collect();
        rows.sort_by(|a, b| a.created.total_cmp(&b.created));
        Ok(rows.iter().map(|s| s.describe()).collect())
    }
    /// The terminal's project folder (routes check it against the view).
    pub fn workspace(&self, id: &str) -> Result<PathBuf> {
        Ok(self.get(id)?.workspace.clone())
    }
    /// Type into the terminal. Blocks while the terminal's input buffer is
    /// full, so callers run it off the async workers.
    pub fn write(&self, id: &str, data: &[u8]) -> Result<()> {
        ensure!(data.len() <= WRITE_LIMIT, "Paste at most 64 KB at a time");
        let session = self.get(id)?;
        ensure!(
            !session.output()?.exited,
            "This terminal's shell has exited. Open a new terminal."
        );
        let _order = session
            .writer
            .lock()
            .map_err(|_| anyhow::anyhow!("Terminal input lock poisoned"))?;
        use std::io::Write;
        (&session.master)
            .write_all(data)
            .context("Could not type into the terminal")?;
        Ok(())
    }
    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<Value> {
        let session = self.get(id)?;
        let (cols, rows) = clamp_size(cols, rows);
        set_size(&session.master, cols, rows)?;
        *session
            .size
            .lock()
            .map_err(|_| anyhow::anyhow!("Terminal size lock poisoned"))? = (cols, rows);
        Ok(json!({"cols": cols, "rows": rows}))
    }
    /// Output after byte offset `after`, base64-encoded. When `after` fell
    /// out of the scrollback the answer starts at the oldest kept byte and
    /// says `truncated`.
    pub fn read(&self, id: &str, after: u64) -> Result<Value> {
        let session = self.get(id)?;
        let output = session.output()?;
        let end = output.start + output.bytes.len() as u64;
        let from = after.clamp(output.start, end);
        let offset = (from - output.start) as usize;
        let take = (output.bytes.len() - offset).min(READ_LIMIT);
        let data = &output.bytes[offset..offset + take];
        Ok(json!({
            "id": session.id,
            "data": base64::engine::general_purpose::STANDARD.encode(data),
            "from": from,
            "cursor": from + take as u64,
            "more": from + (take as u64) < end,
            "truncated": after < output.start,
            "exited": output.exited,
            "exit_code": output.exit_code,
        }))
    }
    /// Hang up one terminal and forget it.
    pub fn close(&self, id: &str) -> Result<()> {
        let session = self
            .sessions()?
            .remove(id)
            .context("This terminal was already closed")?;
        session.hang_up(Duration::from_millis(1500));
        Ok(())
    }
    /// Hang up every terminal (the window is quitting). Waits up to `wait`
    /// for the shells to exit, then kills what is left.
    pub fn close_all(&self, wait: Duration) {
        let sessions: Vec<_> = match self.sessions.lock() {
            Ok(mut sessions) => sessions.drain().map(|(_, s)| s).collect(),
            Err(_) => return,
        };
        for session in &sessions {
            session.hang_up(wait.max(Duration::from_millis(500)));
        }
        let deadline = Instant::now() + wait;
        while Instant::now() < deadline && sessions.iter().any(|s| !s.exited()) {
            std::thread::sleep(Duration::from_millis(20));
        }
        if !wait.is_zero() {
            for session in &sessions {
                session.signal(libc::SIGKILL);
            }
        }
    }
}

impl Drop for Terminals {
    fn drop(&mut self) {
        // Nobody can see these terminals any more.
        self.close_all(Duration::ZERO);
    }
}

fn clamp_size(cols: u16, rows: u16) -> (u16, u16) {
    (
        if cols == 0 { 80 } else { cols.clamp(2, 1000) },
        if rows == 0 { 24 } else { rows.clamp(1, 500) },
    )
}

/// Read output until the terminal hangs up, waking the window at most once
/// per [`WAKE_INTERVAL`] and always after the last chunk of a burst.
fn pump_output(session: Arc<Session>, mut reader: std::fs::File) {
    use std::io::Read;
    use std::os::fd::AsRawFd;
    let fd = reader.as_raw_fd();
    let mut buffer = vec![0u8; 16 * 1024];
    let mut pending = false;
    let mut last = Instant::now() - WAKE_INTERVAL;
    loop {
        if session.closed.load(Ordering::Acquire) {
            break;
        }
        let timeout = if pending {
            WAKE_INTERVAL.saturating_sub(last.elapsed())
        } else {
            Duration::from_millis(250)
        };
        let mut poll = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one valid pollfd for the duration of the call.
        let ready = unsafe { libc::poll(&mut poll, 1, timeout.as_millis() as libc::c_int) };
        if ready < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }
        if ready > 0 {
            if poll.revents & libc::POLLIN != 0 {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => {
                        session.append(&buffer[..count]);
                        pending = true;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    // EIO: every process holding the terminal has exited.
                    Err(_) => break,
                }
            } else if poll.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
                break;
            }
        }
        if pending && last.elapsed() >= WAKE_INTERVAL {
            session.wake("terminal.output");
            pending = false;
            last = Instant::now();
        }
    }
    if pending {
        session.wake("terminal.output");
    }
}

/// The user's login shell: `$SHELL`, else the account's shell, else `sh`.
pub fn login_shell() -> PathBuf {
    if let Some(shell) = std::env::var_os("SHELL").filter(|s| !s.is_empty()) {
        let path = PathBuf::from(shell);
        if path.is_absolute() && path.is_file() {
            return path;
        }
    }
    // SAFETY: getpwuid returns a pointer into static storage or null; the
    // string is copied before any other passwd call.
    unsafe {
        let entry = libc::getpwuid(libc::getuid());
        if !entry.is_null() && !(*entry).pw_shell.is_null() {
            let shell = std::ffi::CStr::from_ptr((*entry).pw_shell);
            let path = PathBuf::from(std::ffi::OsStr::from_encoded_bytes_unchecked(
                shell.to_bytes(),
            ));
            if path.is_file() {
                return path;
            }
        }
    }
    PathBuf::from("/bin/sh")
}

/// The user's environment for their own shell: everything the app was
/// started with, minus the AppImage's private library and data paths (which
/// would break the user's own programs), plus terminal identification.
pub fn user_environment() -> Vec<(OsString, OsString)> {
    let appdir = std::env::var_os("APPDIR").filter(|d| !d.is_empty());
    let mut vars = Vec::new();
    for (name, value) in std::env::vars_os() {
        let key = name.to_string_lossy();
        if matches!(
            key.as_ref(),
            "APPDIR" | "APPIMAGE" | "ARGV0" | "OWD" | "TERM" | "COLORTERM" | "TERM_PROGRAM"
        ) || key.starts_with("APPIMAGE_")
        {
            continue;
        }
        let value = match &appdir {
            Some(dir) if value.to_string_lossy().contains(&*dir.to_string_lossy()) => {
                let dir = dir.to_string_lossy();
                let kept: Vec<String> = value
                    .to_string_lossy()
                    .split(':')
                    .filter(|part| !part.is_empty() && !part.starts_with(&*dir))
                    .map(str::to_owned)
                    .collect();
                if kept.is_empty() {
                    continue;
                }
                OsString::from(kept.join(":"))
            }
            _ => value,
        };
        vars.push((name, value));
    }
    vars.push(("TERM".into(), "xterm-256color".into()));
    vars.push(("COLORTERM".into(), "truecolor".into()));
    vars.push(("TERM_PROGRAM".into(), "ShadowCode".into()));
    vars
}

struct Pty {
    master: std::fs::File,
    child: std::process::Child,
    pid: i32,
}

fn set_size(master: &std::fs::File, cols: u16, rows: u16) -> Result<()> {
    use std::os::fd::AsRawFd;
    let size = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: TIOCSWINSZ reads one winsize from a valid pointer.
    let result = unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, &size) };
    ensure!(
        result == 0,
        "Could not resize the terminal: {}",
        std::io::Error::last_os_error()
    );
    Ok(())
}

#[cfg(target_os = "linux")]
fn spawn(shell: &Path, login: bool, workspace: &Path, cols: u16, rows: u16) -> Result<Pty> {
    use std::os::fd::FromRawFd;
    use std::os::unix::{fs::OpenOptionsExt, process::CommandExt};
    // SAFETY: posix_openpt returns a new descriptor we own, or -1.
    let fd = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC) };
    ensure!(
        fd >= 0,
        "Could not open a terminal: {}",
        std::io::Error::last_os_error()
    );
    // SAFETY: `fd` is a fresh descriptor owned by nothing else.
    let master = unsafe { std::fs::File::from_raw_fd(fd) };
    let mut name = [0 as libc::c_char; 128];
    // SAFETY: fd is a pty master; `name` is large enough for /dev/pts/N.
    unsafe {
        ensure!(
            libc::grantpt(fd) == 0 && libc::unlockpt(fd) == 0,
            "Could not prepare the terminal: {}",
            std::io::Error::last_os_error()
        );
        ensure!(
            libc::ptsname_r(fd, name.as_mut_ptr(), name.len()) == 0,
            "Could not name the terminal"
        );
    }
    // SAFETY: ptsname_r wrote a NUL-terminated path into `name`.
    let path = unsafe { std::ffi::CStr::from_ptr(name.as_ptr()) }
        .to_string_lossy()
        .into_owned();
    set_size(&master, cols, rows)?;
    let slave = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOCTTY)
        .open(&path)
        .context("Could not open the terminal device")?;
    let name = shell
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "sh".into());
    let mut command = std::process::Command::new(shell);
    command
        // A leading dash asks the shell to start as a login shell.
        .arg0(if login { format!("-{name}") } else { name })
        .current_dir(workspace)
        .env_clear()
        .envs(user_environment())
        .env("PWD", workspace)
        .stdin(slave.try_clone()?)
        .stdout(slave.try_clone()?)
        .stderr(slave);
    // SAFETY: only async-signal-safe calls between fork and exec.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::ioctl(0, libc::TIOCSCTTY, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = command
        .spawn()
        .with_context(|| format!("Could not start {}", shell.display()))?;
    let pid = child.id() as i32;
    Ok(Pty { master, child, pid })
}

#[cfg(not(target_os = "linux"))]
fn spawn(_shell: &Path, _login: bool, _workspace: &Path, _cols: u16, _rows: u16) -> Result<Pty> {
    bail!("Terminals are available on Linux")
}

/// Signal a shell's whole session: its own process group, the leader, and
/// (on Linux) every process whose session is the shell's, which includes the
/// job-control groups the shell created.
fn signal_session(pid: i32, signal: libc::c_int, leader: bool) {
    if pid <= 0 {
        return;
    }
    // SAFETY: plain kill(2) calls on ids this terminal started. A process
    // group id is not reused while any member lives.
    unsafe {
        libc::kill(-pid, signal);
        if leader {
            libc::kill(pid, signal);
        }
    }
    #[cfg(target_os = "linux")]
    for member in session_members(pid) {
        // SAFETY: as above; members were read from /proc just now.
        unsafe {
            libc::kill(member, signal);
        }
    }
}

#[cfg(target_os = "linux")]
fn session_members(sid: i32) -> Vec<i32> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<i32>().ok())
        .filter(|pid| {
            std::fs::read_to_string(format!("/proc/{pid}/stat"))
                .ok()
                .and_then(|stat| {
                    // Fields after the parenthesised command: state ppid pgrp session.
                    let rest = &stat[stat.rfind(')')? + 2..];
                    rest.split_whitespace().nth(3)?.parse::<i32>().ok()
                })
                == Some(sid)
        })
        .collect()
}

/// Terminal-safe check used by routes: a terminal id is 32 hex digits.
pub fn valid_id(id: &str) -> Result<&str> {
    if id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(id)
    } else {
        bail!("Unknown terminal")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrollback_is_bounded_and_cursors_stay_absolute() {
        let (sender, _) = broadcast::channel(8);
        let session = Session {
            id: "0".repeat(32),
            workspace: PathBuf::from("/"),
            number: 1,
            shell: "sh".into(),
            created: 0.0,
            pid: 0,
            master: tempfile::tempfile().unwrap(),
            writer: Mutex::new(()),
            output: Mutex::new(Output {
                bytes: Vec::new(),
                start: 0,
                exit_code: None,
                exited: false,
            }),
            size: Mutex::new((80, 24)),
            closed: AtomicBool::new(false),
            sender,
        };
        let line = b"0123456789abcdef0123456789abcdef0123456789abcdef012345678901234\n";
        for _ in 0..(SCROLLBACK_BYTES / line.len() * 2) {
            session.append(line);
        }
        let output = session.output().unwrap();
        assert!(output.bytes.len() <= SCROLLBACK_BYTES);
        assert!(output.start > 0);
        // Cut at a line boundary.
        assert_eq!(output.start % line.len() as u64, 0);
        assert_eq!(output.bytes[0], b'0');
    }

    #[test]
    fn sizes_are_clamped() {
        assert_eq!(clamp_size(0, 0), (80, 24));
        assert_eq!(clamp_size(1, 9000), (2, 500));
    }

    #[test]
    fn terminal_ids_are_hex() {
        assert!(valid_id(&"a".repeat(32)).is_ok());
        assert!(valid_id("../etc").is_err());
    }
}
