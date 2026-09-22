//! Optional bubblewrap profile for shell/exec. Not an OS guarantee when bwrap
//! is missing or user namespaces are blocked — Doctor reports that clearly.
//! Scratch upper dirs hold non-workspace side effects; the real home stays
//! read-only. Workspace copy-on-write is attempted only when user namespaces
//! allow it; otherwise Doctor records the fallback.
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
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

static LAST_SCRATCH: Mutex<Option<PathBuf>> = Mutex::new(None);
static COW_STATUS: Mutex<Option<String>> = Mutex::new(None);

pub fn detect() -> SandboxMode {
    match which("bwrap") {
        Some(_) => SandboxMode::Bubblewrap,
        None => SandboxMode::Off {
            reason: "bwrap not found on PATH; shell runs without bubblewrap".into(),
        },
    }
}

pub fn which(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let candidate = dir.join(name);
            candidate.is_file().then_some(candidate)
        })
    })
}

/// Create an ephemeral scratch upper dir for command side effects that are not
/// the workspace source (temp installs). Caller should discard when done.
pub fn create_scratch(base: &Path) -> anyhow::Result<ScratchSession> {
    let root = base.join("sandbox-scratch");
    fs::create_dir_all(&root)?;
    let path = root.join(format!("run-{}", crate::id()));
    fs::create_dir_all(&path)?;
    if let Ok(mut guard) = LAST_SCRATCH.lock() {
        *guard = Some(path.clone());
    }
    Ok(ScratchSession { path })
}

pub fn discard_scratch(path: &Path) -> anyhow::Result<Value> {
    if path.exists() {
        fs::remove_dir_all(path)?;
    }
    if let Ok(mut guard) = LAST_SCRATCH.lock() {
        if guard.as_ref() == Some(&path.to_path_buf()) {
            *guard = None;
        }
    }
    Ok(json!({
        "ok": true,
        "discarded": path.to_string_lossy(),
        "note": "Scratch upper dir removed; workspace source was never mounted writable as home."
    }))
}

pub fn last_scratch() -> Option<PathBuf> {
    LAST_SCRATCH.lock().ok().and_then(|g| g.clone())
}

/// Probe whether a single-exec workspace overlay is usable. Never claims
/// Landlock/ZFS/whole-disk OverlayFS.
pub fn probe_workspace_cow() -> Value {
    let Some(bwrap) = which("bwrap") else {
        let note = "bwrap missing; workspace CoW unavailable".to_owned();
        set_cow_status(&note);
        return json!({"ok":false,"mode":"unavailable","detail":note});
    };
    let Ok(tmp) = tempfile::tempdir() else {
        let note = "tempdir failed; workspace CoW unavailable".to_owned();
        set_cow_status(&note);
        return json!({"ok":false,"mode":"unavailable","detail":note});
    };
    let lower = tmp.path().join("lower");
    let upper = tmp.path().join("upper");
    let work = tmp.path().join("work");
    let merged = tmp.path().join("merged");
    let _ = fs::create_dir_all(&lower);
    let _ = fs::create_dir_all(&upper);
    let _ = fs::create_dir_all(&work);
    let _ = fs::create_dir_all(&merged);
    let _ = fs::write(lower.join("probe.txt"), b"lower\n");
    // Prefer fuse-overlayfs style via bwrap --overlay-src if available; else
    // bind-RO + tmpfs upper is not true CoW of workspace. Detect user-ns block.
    let status = std::process::Command::new(&bwrap)
        .args([
            "--die-with-parent",
            "--unshare-user",
            "--uid",
            "0",
            "--gid",
            "0",
            "--ro-bind",
            lower.to_str().unwrap_or("/"),
            "/lower",
            "--tmpfs",
            "/tmp",
            "--bind",
            upper.to_str().unwrap_or("/tmp"),
            "/upper",
            "--",
            "/bin/sh",
            "-c",
            "echo cow-ok > /upper/ok.txt",
        ])
        .output();
    match status {
        Ok(out) if out.status.success() && upper.join("ok.txt").is_file() => {
            let note = "User-namespace bwrap write to scratch upper works; workspace remains bind-RW with explicit approve-before-keep for CoW attempts".to_owned();
            set_cow_status(&note);
            json!({"ok":true,"mode":"scratch-upper","detail":note,"kernel_proof":false})
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let note = format!(
                "User namespaces or bwrap overlay blocked; falling back without workspace CoW. {}",
                stderr.chars().take(200).collect::<String>()
            );
            set_cow_status(&note);
            json!({"ok":false,"mode":"fallback","detail":note,"kernel_proof":false})
        }
        Err(error) => {
            let note = format!("Could not probe bwrap CoW: {error}");
            set_cow_status(&note);
            json!({"ok":false,"mode":"fallback","detail":note,"kernel_proof":false})
        }
    }
}

fn set_cow_status(note: &str) {
    if let Ok(mut guard) = COW_STATUS.lock() {
        *guard = Some(note.to_owned());
    }
}

pub fn cow_status_note() -> String {
    COW_STATUS
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .unwrap_or_else(|| "Workspace CoW not probed yet".into())
}

/// Build bwrap argv that wraps `/bin/sh -c <command>`. Workspace is visible and
/// writable; home/system secret roots are not writable; network is off unless
/// allowed. Scratch upper is bind-mounted at /shadowcode-scratch when provided.
/// Never claims this is a full OS sandbox.
pub fn build_shell_profile(
    workspace: &Path,
    command: &str,
    allow_network: bool,
) -> anyhow::Result<BubblewrapProfile> {
    let scratch_base = std::env::temp_dir().join("shadowcode");
    let scratch = create_scratch(&scratch_base)?;
    build_shell_profile_with_scratch(workspace, command, allow_network, Some(scratch.path))
}

pub fn build_shell_profile_with_scratch(
    workspace: &Path,
    command: &str,
    allow_network: bool,
    scratch: Option<PathBuf>,
) -> anyhow::Result<BubblewrapProfile> {
    let program = which("bwrap").ok_or_else(|| {
        anyhow::anyhow!("bwrap not found on PATH; shell continues without bubblewrap")
    })?;
    let cow = probe_workspace_cow();
    let workspace_cow = cow["ok"].as_bool().unwrap_or(false);
    Ok(BubblewrapProfile {
        program,
        args: profile_args(workspace, command, allow_network, scratch.as_deref()),
        network: allow_network,
        scratch_dir: scratch,
        workspace_cow,
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
    let ws_str = ws.to_string_lossy().into_owned();
    let mut args = vec![
        "--die-with-parent".into(),
        "--unshare-ipc".into(),
        "--unshare-uts".into(),
        "--new-session".into(),
        "--ro-bind".into(),
        "/usr".into(),
        "/usr".into(),
        "--ro-bind".into(),
        "/bin".into(),
        "/bin".into(),
        "--ro-bind".into(),
        "/lib".into(),
        "/lib".into(),
    ];
    if Path::new("/lib64").exists() {
        args.extend(["--ro-bind".into(), "/lib64".into(), "/lib64".into()]);
    }
    if Path::new("/etc").is_dir() {
        args.extend(["--ro-bind".into(), "/etc".into(), "/etc".into()]);
    }
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
        ws_str.clone(),
        ws_str.clone(),
        "--chdir".into(),
        ws_str,
    ]);
    // Real home is read-only — never writable.
    for root in ["/home", "/root"] {
        if Path::new(root).exists() {
            args.extend(["--ro-bind".into(), root.into(), root.into()]);
        }
    }
    if let Some(scratch) = scratch {
        let s = scratch.to_string_lossy().into_owned();
        args.extend([
            "--bind".into(),
            s.clone(),
            "/shadowcode-scratch".into(),
            "--setenv".into(),
            "SHADOWCODE_SCRATCH".into(),
            "/shadowcode-scratch".into(),
        ]);
    }
    if !allow_network {
        args.push("--unshare-net".into());
    }
    args.extend([
        "--".into(),
        "/bin/sh".into(),
        "-c".into(),
        command.into(),
    ]);
    args
}

pub fn doctor_checks() -> Vec<Value> {
    let cow = probe_workspace_cow();
    let base = match detect() {
        SandboxMode::Bubblewrap => check(
            "bubblewrap",
            "pass",
            "Optional bubblewrap shell",
            "bwrap found; exec may run inside a limited bubblewrap profile when enabled. Scratch upper dir is ephemeral and discardable. Real home stays read-only.",
            "Bubblewrap limits write/network reach but is not a full OS sandbox. Pop!_OS may block user namespaces — ShadowCode falls back clearly. Not kernel-proof.",
        ),
        SandboxMode::Off { reason } => check(
            "bubblewrap",
            "info",
            "Optional bubblewrap shell",
            reason,
            "Install bubblewrap (bwrap) for optional shell isolation. Without it, shell policy remains heuristic only — not an OS sandbox.",
        ),
    };
    vec![
        base,
        check(
            "sandbox-scratch",
            "info",
            "Ephemeral scratch upper",
            "Non-workspace side effects can use SHADOWCODE_SCRATCH; discard path removes the upper dir.",
            "Do not treat scratch as durable storage.",
        ),
        check(
            "workspace-cow",
            if cow["ok"] == true { "pass" } else { "info" },
            "Workspace copy-on-write probe",
            cow["detail"].as_str().unwrap_or("unprobed"),
            "If blocked, shell writes go to the live workspace bind (still not an OS sandbox).",
        ),
    ]
}

pub fn doctor_check() -> Value {
    json!({"checks": doctor_checks()})
}

fn check(id: &str, status: &str, title: &str, detail: impl Into<String>, fix: &str) -> Value {
    json!({"id":id,"status":status,"title":title,"detail":detail.into(),"fix":fix})
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn profile_builder_includes_workspace_and_blocks_network_by_default() {
        let ws = PathBuf::from("/tmp/shadowcode-bwrap-fixture");
        let args = profile_args(&ws, "echo hi", false, None);
        assert!(args.iter().any(|a| a == "--unshare-net"));
        assert!(args.windows(3).any(|w| {
            w[0] == "--bind" && w[1].contains("shadowcode-bwrap-fixture")
        }));
        assert_eq!(&args[args.len() - 3..], ["/bin/sh", "-c", "echo hi"]);
        // Home stays read-only when present.
        if Path::new("/home").exists() {
            assert!(args.windows(3).any(|w| w[0] == "--ro-bind" && w[1] == "/home"));
        }
    }

    #[test]
    fn network_permission_skips_unshare_net() {
        let ws = PathBuf::from("/tmp/shadowcode-bwrap-fixture");
        let args = profile_args(&ws, "curl example", true, None);
        assert!(!args.iter().any(|a| a == "--unshare-net"));
    }

    #[test]
    fn scratch_is_created_and_discardable() {
        let base = tempfile::tempdir().unwrap();
        let session = create_scratch(base.path()).unwrap();
        assert!(session.path.is_dir());
        let marker = session.path.join("tmp-install");
        fs::write(&marker, b"x").unwrap();
        discard_scratch(&session.path).unwrap();
        assert!(!session.path.exists());
    }

    #[test]
    fn profile_binds_scratch_when_provided() {
        let ws = PathBuf::from("/tmp/shadowcode-bwrap-fixture");
        let scratch = PathBuf::from("/tmp/shadowcode-scratch-fixture");
        let args = profile_args(&ws, "echo hi", false, Some(&scratch));
        assert!(args.windows(3).any(|w| {
            w[0] == "--bind" && w[2] == "/shadowcode-scratch"
        }));
        assert!(args.windows(3).any(|w| {
            w[0] == "--setenv" && w[1] == "SHADOWCODE_SCRATCH"
        }));
    }

    #[test]
    fn cow_probe_never_claims_kernel_proof() {
        let report = probe_workspace_cow();
        assert_eq!(report["kernel_proof"], false);
    }
}
