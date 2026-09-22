//! Optional read-only health checks. State and pending proposal approvals belong
//! to one Service/profile and are additionally scoped to the selected workspace.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
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
#[derive(Clone)]
struct Proposal {
    id: String,
    summary: String,
}
#[derive(Default)]
struct State {
    last_run: Option<f64>,
    last_result: Option<Value>,
    pending: Option<Proposal>,
}
#[derive(Default)]
pub struct Guardian {
    state: Mutex<HashMap<PathBuf, State>>,
}
pub fn from_config_value(value: &Value) -> GuardianConfig {
    serde_json::from_value(value.clone()).unwrap_or_default()
}
impl Guardian {
    pub fn status(&self, cfg: &GuardianConfig, workspace: &Path) -> Result<Value> {
        let state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("Guardian lock poisoned"))?;
        let current = state.get(&workspace.canonicalize()?);
        Ok(json!({"enabled":cfg.enabled,"default":"off",
            "last_run":current.and_then(|s|s.last_run),"last_result":current.and_then(|s|s.last_result.clone()),
            "pending_patch_approval":current.is_some_and(|s|s.pending.is_some()),
            "note":"Health checks inspect local diagnostics. Proposals are review drafts, not generated patches; no code, push, PR, or merge is performed."}))
    }
    pub fn run_health_check(&self, cfg: &GuardianConfig, workspace: &Path) -> Result<Value> {
        ensure!(cfg.enabled, "Guardian is disabled (default off)");
        let workspace = workspace.canonicalize()?;
        ensure!(workspace.is_dir(), "Workspace is not a directory");
        let doctor = crate::sandbox::doctor_check();
        let hint = if workspace.join("Cargo.toml").is_file() {
            Some("cargo test")
        } else if workspace.join("package.json").is_file() {
            Some("npm test")
        } else {
            None
        };
        let result = json!({"ok":true,"readonly":true,"workspace":workspace,
            "doctor":doctor,"tests":{"hint":hint,"present":hint.is_some(),"executed":false},
            "wrote_main_tree":false,"auto_pr":false,
            "note":"Local diagnostics checked. Suggested test commands have not been run."});
        let mut all = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("Guardian lock poisoned"))?;
        let state = all.entry(workspace).or_default();
        state.last_run = Some(crate::now());
        state.last_result = Some(result.clone());
        Ok(result)
    }
    pub fn request_prepare_patch(
        &self,
        cfg: &GuardianConfig,
        workspace: &Path,
        summary: &str,
    ) -> Result<Value> {
        ensure!(
            cfg.enabled && cfg.allow_prepare_patch,
            "Enable Guardian proposal drafts before requesting one"
        );
        ensure!(
            !summary.trim().is_empty() && summary.len() <= 64000,
            "A bounded proposal summary is required"
        );
        let proposal = Proposal {
            id: crate::id(),
            summary: summary.into(),
        };
        self.state
            .lock()
            .map_err(|_| anyhow::anyhow!("Guardian lock poisoned"))?
            .entry(workspace.canonicalize()?)
            .or_default()
            .pending = Some(proposal.clone());
        Ok(
            json!({"ok":true,"needs_approval":true,"approval_id":proposal.id,"summary":proposal.summary,
            "note":"Approve this exact proposal to save a draft outside the project. No code changes will be generated."}),
        )
    }
    pub fn approve_prepare_patch(
        &self,
        cfg: &GuardianConfig,
        workspace: &Path,
        root: &Path,
        approval_id: &str,
    ) -> Result<Value> {
        ensure!(
            cfg.enabled && cfg.allow_prepare_patch,
            "Guardian proposal drafts are disabled"
        );
        let workspace = workspace.canonicalize()?;
        ensure!(
            !root.starts_with(&workspace),
            "Proposal storage must be outside the workspace"
        );
        let mut all = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("Guardian lock poisoned"))?;
        let state = all
            .get_mut(&workspace)
            .context("No proposal approval for this workspace")?;
        let proposal = state
            .pending
            .as_ref()
            .context("No pending proposal approval")?;
        ensure!(
            proposal.id == approval_id,
            "Proposal approval is stale or belongs to a different request"
        );
        crate::paths::private_directory(root)?;
        ensure!(
            !root.canonicalize()?.starts_with(&workspace),
            "Proposal storage must be outside the workspace"
        );
        let draft = tempfile::Builder::new()
            .prefix("proposal-")
            .tempdir_in(root)?;
        let document = draft.path().join("PROPOSAL.md");
        fs::write(&document,format!("# Guardian proposal\n\n{}\n\nDraft for review; no code changes or tests have been performed.\n",proposal.summary))?;
        let directory = draft.keep();
        state.pending = None;
        Ok(
            json!({"ok":true,"proposal_dir":directory,"proposal":document,
            "main_tree_written":false,"pushed":false,"pr_opened":false,
            "note":"Saved the approved proposal draft. No patch or Git worktree was created."}),
        )
    }
}
pub fn interval(cfg: &GuardianConfig) -> Duration {
    Duration::from_secs(cfg.interval_sec.clamp(60, 86400))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn approvals_are_bound_to_profile_workspace_and_original_summary() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("project");
        fs::create_dir(&workspace).unwrap();
        let cfg = GuardianConfig {
            enabled: true,
            allow_prepare_patch: true,
            ..Default::default()
        };
        let a = Guardian::default();
        let b = Guardian::default();
        let store = root.path().join("drafts");
        let request = a
            .request_prepare_patch(&cfg, &workspace, "Inspect regression")
            .unwrap();
        let id = request["approval_id"].as_str().unwrap();
        assert!(b
            .approve_prepare_patch(&cfg, &workspace, &store, id)
            .is_err());
        assert!(a
            .approve_prepare_patch(&cfg, root.path(), &store, id)
            .is_err());
        assert!(a
            .approve_prepare_patch(&cfg, &workspace, &store, "different")
            .is_err());
        let result = a
            .approve_prepare_patch(&cfg, &workspace, &store, id)
            .unwrap();
        assert!(fs::read_to_string(result["proposal"].as_str().unwrap())
            .unwrap()
            .contains("Inspect regression"));
        assert!(a
            .approve_prepare_patch(&cfg, &workspace, &store, id)
            .is_err());
        assert!(a
            .request_prepare_patch(&GuardianConfig::default(), &workspace, "no")
            .is_err());
        assert_eq!(fs::read_dir(&workspace).unwrap().count(), 0);
    }
    #[test]
    fn health_check_is_opt_in_and_does_not_change_project() {
        let root = tempfile::tempdir().unwrap();
        let guardian = Guardian::default();
        fs::write(root.path().join("Cargo.toml"), "fixture").unwrap();
        assert!(guardian
            .run_health_check(&GuardianConfig::default(), root.path())
            .is_err());
        let cfg = GuardianConfig {
            enabled: true,
            ..Default::default()
        };
        let result = guardian.run_health_check(&cfg, root.path()).unwrap();
        assert_eq!(result["tests"]["executed"], false);
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
}
