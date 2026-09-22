//! Optional shell isolation. Availability is probed before a user command runs;
//! commands are never replayed outside bubblewrap after a runtime failure.
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SandboxMode {
    Off { reason: String },
    Bubblewrap,
}
#[derive(Clone, Debug)]
pub struct BubblewrapProfile {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub network: bool,
    pub scratch_dir: Option<PathBuf>,
    pub workspace_cow: bool,
}
#[derive(Clone, Debug, Default)]
pub struct ScratchSession {
    pub path: PathBuf,
}
static SCRATCH: LazyLock<Mutex<HashMap<PathBuf, tempfile::TempDir>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static COW_STATUS: Mutex<Option<String>> = Mutex::new(None);

pub fn which(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let candidate = dir.join(name);
            let metadata = candidate.metadata().ok()?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if metadata.permissions().mode() & 0o111 == 0 {
                    return None;
                }
            }
            metadata.is_file().then_some(candidate)
        })
    })
}
pub fn detect() -> SandboxMode {
    match which("bwrap") {
        Some(_) => SandboxMode::Bubblewrap,
        None => SandboxMode::Off {
            reason: "bubblewrap is not installed".into(),
        },
    }
}

/// Only directories created and retained by this process can be discarded.
pub fn create_scratch(base: &Path) -> Result<ScratchSession> {
    crate::paths::private_directory(base)?;
    let owned = tempfile::Builder::new()
        .prefix("shadowcode-scratch-")
        .tempdir_in(base)?;
    let path = owned.path().canonicalize()?;
    SCRATCH
        .lock()
        .map_err(|_| anyhow::anyhow!("Scratch lock poisoned"))?
        .insert(path.clone(), owned);
    Ok(ScratchSession { path })
}
pub fn discard_scratch(path: &Path) -> Result<Value> {
    let mut scratch = SCRATCH
        .lock()
        .map_err(|_| anyhow::anyhow!("Scratch lock poisoned"))?;
    let owned = scratch
        .remove(path)
        .context("Refusing to remove an unregistered scratch directory")?;
    owned.close()?;
    Ok(
        json!({"ok":true,"discarded":path,"note":"Removed this command's managed temporary directory."}),
    )
}
pub fn last_scratch() -> Option<PathBuf> {
    SCRATCH.lock().ok().and_then(|g| g.keys().next().cloned())
}

fn probe(program: &Path, workspace: &Path) -> Result<()> {
    let mut child = Command::new(program)
        .args(profile_args(workspace, "true", false, None))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(status) = child.try_wait()? {
            ensure!(
                status.success(),
                "bubblewrap is installed but its namespace/mount probe failed ({status})"
            );
            return Ok(());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("bubblewrap availability probe timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Scratch availability is not evidence of workspace copy-on-write.
pub fn probe_workspace_cow() -> Value {
    let result = (|| -> Result<()> {
        let program = which("bwrap").context("bubblewrap is not installed")?;
        let temp = tempfile::tempdir()?;
        probe(&program, temp.path())
    })();
    let available = result.is_ok();
    let detail = match result {
        Ok(()) => "Bubblewrap works. Workspace writes are live; copy-on-write and approve-before-keep are not implemented.".into(),
        Err(error) => format!("{error}. Workspace copy-on-write is unavailable."),
    };
    if let Ok(mut state) = COW_STATUS.lock() {
        *state = Some(detail.clone());
    }
    json!({"ok":false,"workspace_cow":false,"shell_available":available,
        "mode":if available {"scratch-only"} else {"unavailable"},
        "detail":detail,"kernel_proof":false})
}
pub fn cow_status_note() -> String {
    COW_STATUS
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .unwrap_or_else(|| "Workspace writes are live; copy-on-write is unavailable".into())
}

pub fn build_shell_profile(
    workspace: &Path,
    command: &str,
    allow_network: bool,
) -> Result<BubblewrapProfile> {
    let program = which("bwrap").context("bubblewrap is not installed")?;
    probe(&program, workspace)?;
    let base =
        std::env::temp_dir().join(format!("shadowcode-scratch-{}", unsafe { libc::geteuid() }));
    let scratch = create_scratch(&base)?;
    build_shell_profile_with_scratch(workspace, command, allow_network, Some(scratch.path))
}
pub fn build_shell_profile_with_scratch(
    workspace: &Path,
    command: &str,
    allow_network: bool,
    scratch: Option<PathBuf>,
) -> Result<BubblewrapProfile> {
    Ok(BubblewrapProfile {
        program: which("bwrap").context("bubblewrap is not installed")?,
        args: profile_args(workspace, command, allow_network, scratch.as_deref()),
        network: allow_network,
        scratch_dir: scratch,
        workspace_cow: false,
    })
}
pub fn profile_args(
    workspace: &Path,
    command: &str,
    allow_network: bool,
    scratch: Option<&Path>,
) -> Vec<String> {
    let ws = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());
    let mut args: Vec<String> = [
        "--die-with-parent",
        "--unshare-pid",
        "--unshare-ipc",
        "--unshare-uts",
        "--new-session",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    for root in ["/usr", "/bin", "/lib", "/lib64", "/etc", "/home"] {
        if Path::new(root).exists() {
            args.extend(["--ro-bind".into(), root.into(), root.into()]);
        }
    }
    // Bind the project after home, otherwise /home hides a project mount.
    args.extend([
        "--tmpfs".into(),
        "/tmp".into(),
        "--tmpfs".into(),
        "/var/tmp".into(),
        "--dev".into(),
        "/dev".into(),
        "--proc".into(),
        "/proc".into(),
        "--bind".into(),
        ws.to_string_lossy().into_owned(),
        ws.to_string_lossy().into_owned(),
        "--chdir".into(),
        ws.to_string_lossy().into_owned(),
    ]);
    if let Some(scratch) = scratch {
        args.extend([
            "--bind".into(),
            scratch.to_string_lossy().into_owned(),
            "/shadowcode-scratch".into(),
            "--setenv".into(),
            "SHADOWCODE_SCRATCH".into(),
            "/shadowcode-scratch".into(),
        ]);
    }
    if !allow_network {
        args.push("--unshare-net".into());
    }
    args.extend(["--".into(), "/bin/sh".into(), "-c".into(), command.into()]);
    args
}
pub fn doctor_checks() -> Vec<Value> {
    let report = probe_workspace_cow();
    vec![
        json!({"id":"bubblewrap","status":if report["shell_available"]==true {"pass"} else {"info"},
        "title":"Optional shell isolation","detail":report["detail"],
        "fix":"In automatic mode, unavailable bubblewrap is detected before running a command. A command is never replayed after a sandbox failure."}),
        json!({"id":"workspace-cow","status":"info","title":"Live workspace writes",
        "detail":"Shell writes affect the current workspace. Scratch storage is temporary, not copy-on-write.",
        "fix":"Use an isolated Git worktree for changes you want to review before integrating."}),
    ]
}
pub fn doctor_check() -> Value {
    json!({"checks":doctor_checks()})
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[test]
    fn cleanup_refuses_user_data_and_symlinks() {
        let base = tempfile::tempdir().unwrap();
        let user = base.path().join("project");
        fs::create_dir(&user).unwrap();
        fs::write(user.join("important"), "keep").unwrap();
        assert!(discard_scratch(&user).is_err());
        let scratch = create_scratch(base.path()).unwrap();
        fs::write(scratch.path.join("temp"), "x").unwrap();
        discard_scratch(&scratch.path).unwrap();
        assert!(!scratch.path.exists());
        assert!(user.join("important").exists());
    }
    #[test]
    fn workspace_mount_overrides_read_only_home_and_network_is_scoped() {
        let args = profile_args(Path::new("/home/test/project"), "echo hi", false, None);
        let ws = args
            .windows(3)
            .position(|w| w == ["--bind", "/home/test/project", "/home/test/project"])
            .unwrap();
        if Path::new("/home").exists() {
            assert!(
                args.windows(3)
                    .position(|w| w == ["--ro-bind", "/home", "/home"])
                    .unwrap()
                    < ws
            );
        }
        assert!(args.iter().any(|a| a == "--unshare-net"));
        assert!(!profile_args(Path::new("/tmp"), "true", true, None)
            .iter()
            .any(|a| a == "--unshare-net"));
    }
    #[test]
    fn scratch_probe_never_claims_copy_on_write() {
        let report = probe_workspace_cow();
        assert_eq!(report["ok"], false);
        assert_eq!(report["kernel_proof"], false);
    }

    #[test]
    fn missing_bubblewrap_reports_honest_fallback() {
        let previous = std::env::var_os("PATH");
        // Empty PATH: bwrap cannot be resolved even if installed on the host.
        std::env::set_var("PATH", "");
        let mode = detect();
        let report = probe_workspace_cow();
        let checks = doctor_checks();
        match previous {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }
        assert!(matches!(
            mode,
            SandboxMode::Off { reason } if reason.contains("bubblewrap")
        ));
        assert_eq!(report["shell_available"], false);
        assert_eq!(report["mode"], "unavailable");
        assert_eq!(report["kernel_proof"], false);
        let bubble = checks
            .iter()
            .find(|c| c["id"] == "bubblewrap")
            .expect("doctor includes bubblewrap");
        assert_eq!(bubble["status"], "info");
        assert!(bubble["detail"]
            .as_str()
            .unwrap_or("")
            .to_ascii_lowercase()
            .contains("bubblewrap"));
    }
}
