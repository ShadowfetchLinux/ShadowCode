//! Optional bubblewrap profile for shell/exec. Not an OS guarantee when bwrap
//! is missing or user namespaces are blocked — Doctor reports that clearly.
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

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
}

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

/// Build bwrap argv that wraps `/bin/sh -c <command>`. Workspace is visible and
/// writable; home/system secret roots are not writable; network is off unless
/// allowed. Never claims this is a full OS sandbox.
pub fn build_shell_profile(
    workspace: &Path,
    command: &str,
    allow_network: bool,
) -> anyhow::Result<BubblewrapProfile> {
    let program = which("bwrap").ok_or_else(|| {
        anyhow::anyhow!("bwrap not found on PATH; shell continues without bubblewrap")
    })?;
    Ok(BubblewrapProfile {
        program,
        args: profile_args(workspace, command, allow_network),
        network: allow_network,
    })
}

pub fn profile_args(workspace: &Path, command: &str, allow_network: bool) -> Vec<String> {
    let ws = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());
    let ws_str = ws.to_string_lossy().into_owned();
    let mut args = vec![
        "--die-with-parent".into(),
        "--unshare-pid".into(),
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
    for root in ["/home", "/root"] {
        if Path::new(root).exists() {
            args.extend(["--ro-bind".into(), root.into(), root.into()]);
        }
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

pub fn doctor_check() -> Value {
    match detect() {
        SandboxMode::Bubblewrap => check(
            "bubblewrap",
            "pass",
            "Optional bubblewrap shell",
            "bwrap found; exec may run inside a limited bubblewrap profile when enabled",
            "Bubblewrap limits write/network reach but is not a full OS sandbox. Pop!_OS may block user namespaces — ShadowCode falls back clearly.",
        ),
        SandboxMode::Off { reason } => check(
            "bubblewrap",
            "info",
            "Optional bubblewrap shell",
            reason,
            "Install bubblewrap (bwrap) for optional shell isolation. Without it, shell policy remains heuristic only — not an OS sandbox.",
        ),
    }
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
        let args = profile_args(&ws, "echo hi", false);
        assert!(args.iter().any(|a| a == "--unshare-net"));
        assert!(args.windows(3).any(|w| {
            w[0] == "--bind" && w[1].contains("shadowcode-bwrap-fixture")
        }));
        assert_eq!(
            &args[args.len() - 3..],
            ["/bin/sh", "-c", "echo hi"]
        );
    }

    #[test]
    fn network_permission_skips_unshare_net() {
        let ws = PathBuf::from("/tmp/shadowcode-bwrap-fixture");
        let args = profile_args(&ws, "curl example", true);
        assert!(!args.iter().any(|a| a == "--unshare-net"));
    }
}
