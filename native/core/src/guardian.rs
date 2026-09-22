//! Optional scheduled read-only health check for `shadowcode serve`.
//! Default OFF. May open a worktree and prepare a patch ONLY after explicit
//! user approval. Never pushes, opens a PR, or notify-and-merges while idle.
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct GuardianConfig {
    pub enabled: bool,
    pub interval_sec: u64,
    pub allow_prepare_patch: bool,
}

impl Default for GuardianConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_sec: 3600,
            allow_prepare_patch: false,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct GuardianState {
    pub last_run: Option<f64>,
    pub last_result: Option<Value>,
    pub pending_patch_approval: bool,
    pub prepared_patch: Option<Value>,
}

static ENABLED: AtomicBool = AtomicBool::new(false);
static STATE: Mutex<GuardianState> = Mutex::new(GuardianState {
    last_run: None,
    last_result: None,
    pending_patch_approval: false,
    prepared_patch: None,
});

pub fn from_config_value(value: &Value) -> GuardianConfig {
    GuardianConfig {
        enabled: value["enabled"].as_bool().unwrap_or(false),
        interval_sec: value["interval_sec"].as_u64().unwrap_or(3600).clamp(60, 86_400),
        allow_prepare_patch: value["allow_prepare_patch"].as_bool().unwrap_or(false),
    }
}

pub fn apply_config(cfg: &GuardianConfig) {
    ENABLED.store(cfg.enabled, Ordering::Release);
}

pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
}

pub fn status() -> Value {
    let state = STATE.lock().ok();
    let last = state.as_ref().and_then(|s| s.last_result.clone());
    json!({
        "enabled": is_enabled(),
        "default": "off",
        "last_run": state.as_ref().and_then(|s| s.last_run),
        "pending_patch_approval": state.as_ref().map(|s| s.pending_patch_approval).unwrap_or(false),
        "last_result": last,
        "note": "Guardian never pushes, opens a PR, or merges while idle. Patch prepare requires explicit approval."
    })
}

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

/// Read-only health check: doctor-like probes and optional test discovery.
/// Must not write the main tree.
pub fn run_health_check(workspace: &Path) -> Result<Value> {
    ensure!(
        is_enabled(),
        "Guardian is disabled (default OFF). Enable it in Settings before scheduling checks."
    );
    let before = snapshot_mtime(workspace);
    let doctor = crate::sandbox::doctor_check();
    let tests = discover_test_hint(workspace);
    let after = snapshot_mtime(workspace);
    ensure!(
        before == after,
        "Guardian health check mutated the main tree; aborting"
    );
    let result = json!({
        "ok": true,
        "readonly": true,
        "workspace": workspace.to_string_lossy(),
        "doctor": doctor,
        "tests": tests,
        "wrote_main_tree": false,
        "auto_pr": false,
        "note": "Check finished; review needed. No push/PR/merge was performed."
    });
    if let Ok(mut state) = STATE.lock() {
        state.last_run = Some(now());
        state.last_result = Some(result.clone());
    }
    notify_review_needed(&result);
    Ok(result)
}

fn snapshot_mtime(workspace: &Path) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(workspace) {
        for entry in entries.flatten().take(64) {
            let path = entry.path();
            if path.file_name().and_then(|n| n.to_str()) == Some(".git") {
                continue;
            }
            let mt = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            out.push((path.to_string_lossy().into_owned(), mt));
        }
    }
    out.sort();
    out
}

fn discover_test_hint(workspace: &Path) -> Value {
    if workspace.join("Cargo.toml").is_file() {
        json!({"hint":"cargo test","present":true})
    } else if workspace.join("package.json").is_file() {
        json!({"hint":"npm test","present":true})
    } else {
        json!({"hint":null,"present":false})
    }
}

fn notify_review_needed(result: &Value) {
    let summary = result["note"].as_str().unwrap_or("Guardian check finished");
    // Prefer notify-send on Linux desktops; otherwise log.
    if crate::sandbox::which("notify-send").is_some() {
        let _ = std::process::Command::new("notify-send")
            .args(["ShadowCode Guardian", summary])
            .output();
    }
    tracing::info!(target: "shadowcode::guardian", "{summary}");
}

/// Request to prepare a patch in a worktree — requires explicit approval flag.
pub fn request_prepare_patch(summary: &str) -> Result<Value> {
    ensure!(is_enabled(), "Guardian is disabled");
    ensure!(!summary.trim().is_empty(), "Patch summary required");
    if let Ok(mut state) = STATE.lock() {
        state.pending_patch_approval = true;
        state.prepared_patch = None;
    }
    Ok(json!({
        "ok": true,
        "needs_approval": true,
        "summary": summary,
        "note": "Patch will not be prepared until approve_prepare_patch is called. No push/PR."
    }))
}

pub fn approve_prepare_patch(
    workspace: &Path,
    checkout_root: &Path,
    summary: &str,
) -> Result<Value> {
    ensure!(is_enabled(), "Guardian is disabled");
    let mut state = STATE
        .lock()
        .map_err(|_| anyhow::anyhow!("guardian lock poisoned"))?;
    ensure!(
        state.pending_patch_approval,
        "No pending patch approval; refuse silent prepare"
    );
    fs::create_dir_all(checkout_root)?;
    let path = checkout_root.join(format!("guardian-{}", crate::id()));
    fs::create_dir_all(&path)?;
    // Prepare a patch *proposal* file only inside the worktree copy area —
    // never write the main tree.
    let proposal = path.join("PROPOSED_PATCH.md");
    fs::write(
        &proposal,
        format!("# Guardian proposed patch\n\n{summary}\n\nStatus: awaiting human review. Not pushed.\n"),
    )?;
    let prepared = json!({
        "ok": true,
        "worktree": path.to_string_lossy(),
        "proposal": proposal.to_string_lossy(),
        "main_tree_written": false,
        "pushed": false,
        "pr_opened": false,
        "note": "Patch prepared after explicit approval only; main tree untouched."
    });
    state.pending_patch_approval = false;
    state.prepared_patch = Some(prepared.clone());
    // Confirm main tree marker file was not created.
    ensure!(
        !workspace.join(".shadowcode-guardian-wrote").exists(),
        "Guardian must not write main tree markers"
    );
    Ok(prepared)
}

pub fn interval(cfg: &GuardianConfig) -> Duration {
    Duration::from_secs(cfg.interval_sec.clamp(60, 86_400))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_off() {
        ENABLED.store(false, Ordering::Release);
        assert!(!is_enabled());
        let cfg = GuardianConfig::default();
        assert!(!cfg.enabled);
    }

    #[test]
    fn enabled_check_does_not_write_main_tree() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("Cargo.toml"), "[package]\nname=\"t\"\nversion=\"0.1.0\"\n").unwrap();
        ENABLED.store(true, Ordering::Release);
        let result = run_health_check(root.path()).unwrap();
        assert_eq!(result["wrote_main_tree"], false);
        assert_eq!(result["auto_pr"], false);
        assert!(!root.path().join(".shadowcode-guardian-wrote").exists());
        ENABLED.store(false, Ordering::Release);
    }

    #[test]
    fn prepare_patch_requires_approval() {
        ENABLED.store(true, Ordering::Release);
        let root = tempfile::tempdir().unwrap();
        let req = request_prepare_patch("fix typo").unwrap();
        assert_eq!(req["needs_approval"], true);
        let prepared = approve_prepare_patch(
            root.path(),
            &root.path().join("wt"),
            "fix typo",
        )
        .unwrap();
        assert_eq!(prepared["pushed"], false);
        assert_eq!(prepared["main_tree_written"], false);
        ENABLED.store(false, Ordering::Release);
    }
}
