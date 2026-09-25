//! Code intelligence for the native agent loop: the tree-sitter index
//! (`crate::symbol_index`), full-text and semantic search, the ranked repo
//! map, and persistent language servers (`crate::lsp`).
//!
//! Settings live under `code_intel` in the user's config.yaml. A project's
//! own `.shadow/config` cannot set them (it cannot name programs to run).
pub mod chunks;
pub mod embeddings;
pub mod langs;
pub mod repo_map;
pub mod search;

use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// A user-chosen language server command for one language.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerOverride {
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CodeIntelConfig {
    /// Start language servers for edited files (when installed).
    pub lsp: bool,
    /// Attach errors introduced by an edit to edit tool results.
    pub diagnostics_on_edit: bool,
    /// How long an edit waits for a language server to report.
    pub diagnostics_wait_ms: u64,
    /// Stop a language server after this many idle minutes.
    pub lsp_idle_minutes: u64,
    /// Most language servers alive at once, across projects.
    pub max_servers: usize,
    /// Commands per language (`rust`, `typescript`, `python`, `go`, `c`).
    pub servers: BTreeMap<String, ServerOverride>,
    /// Token budget for the ranked repo map in the system prompt; 0 turns it off.
    pub repo_map_tokens: usize,
    /// Fuse embedding similarity into search_code when a model is installed.
    pub semantic_search: bool,
    /// Installed embedding model to use; empty picks the first installed one.
    pub embedding_model: String,
}

impl Default for CodeIntelConfig {
    fn default() -> Self {
        Self {
            lsp: true,
            diagnostics_on_edit: true,
            diagnostics_wait_ms: 3_000,
            lsp_idle_minutes: 10,
            max_servers: 4,
            servers: BTreeMap::new(),
            repo_map_tokens: 1_024,
            semantic_search: true,
            embedding_model: String::new(),
        }
    }
}

pub const LANGUAGES: &[&str] = &["rust", "typescript", "python", "go", "c"];

impl CodeIntelConfig {
    /// Read `code_intel` from the loaded configuration (defaults when absent).
    pub fn from_config(config: &crate::config::Config) -> Result<Self> {
        match config.extra.get("code_intel") {
            None | Some(Value::Null) => Ok(Self::default()),
            Some(value) => {
                let parsed: Self = serde_json::from_value(value.clone())
                    .map_err(|e| anyhow::anyhow!("Invalid code_intel settings: {e}"))?;
                parsed.validate()?;
                Ok(parsed)
            }
        }
    }
    /// Like `from_config`, but an invalid section falls back to defaults so a
    /// typo never breaks a running task.
    pub fn lenient(config: &crate::config::Config) -> Self {
        Self::from_config(config).unwrap_or_default()
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (200..=30_000).contains(&self.diagnostics_wait_ms),
            "code_intel.diagnostics_wait_ms must be between 200 and 30000"
        );
        ensure!(
            (1..=240).contains(&self.lsp_idle_minutes),
            "code_intel.lsp_idle_minutes must be between 1 and 240"
        );
        ensure!(
            (1..=16).contains(&self.max_servers),
            "code_intel.max_servers must be between 1 and 16"
        );
        ensure!(
            self.repo_map_tokens <= 16_384,
            "code_intel.repo_map_tokens must be at most 16384"
        );
        for (language, server) in &self.servers {
            ensure!(
                LANGUAGES.contains(&language.as_str()),
                "code_intel.servers: unknown language {language:?}"
            );
            ensure!(
                !server.command.trim().is_empty()
                    && server.command.len() <= 4096
                    && server.args.len() <= 32
                    && !server.command.contains('\0')
                    && server
                        .args
                        .iter()
                        .all(|a| a.len() <= 4096 && !a.contains('\0')),
                "code_intel.servers.{language}: invalid command"
            );
        }
        ensure!(
            self.embedding_model.is_empty()
                || embeddings::catalog_entry(&self.embedding_model).is_some(),
            "code_intel.embedding_model is not a known embedding model"
        );
        Ok(())
    }
}

/// Where managed language servers, embedding models and vectors live.
pub fn data_dir(paths: &crate::paths::AppPaths) -> std::path::PathBuf {
    paths.data.join("code-intel")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_and_validation() {
        let mut config = crate::config::Config::default();
        assert_eq!(
            CodeIntelConfig::from_config(&config).unwrap(),
            CodeIntelConfig::default()
        );
        config.extra.insert(
            "code_intel".into(),
            serde_json::json!({"repo_map_tokens": 0, "servers": {"python": {"command": "/bin/fake", "args": ["--stdio"]}}}),
        );
        let parsed = CodeIntelConfig::from_config(&config).unwrap();
        assert_eq!(parsed.repo_map_tokens, 0);
        assert!(parsed.lsp);
        assert_eq!(parsed.servers["python"].args, ["--stdio"]);
        config.extra.insert(
            "code_intel".into(),
            serde_json::json!({"servers": {"cobol": {"command": "x"}}}),
        );
        assert!(CodeIntelConfig::from_config(&config).is_err());
        assert_eq!(
            CodeIntelConfig::lenient(&config),
            CodeIntelConfig::default()
        );
        config.extra.insert(
            "code_intel".into(),
            serde_json::json!({"diagnostics_wait_ms": 5}),
        );
        assert!(CodeIntelConfig::from_config(&config).is_err());
    }
}
