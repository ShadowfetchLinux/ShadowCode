//! Vendor CLI agent backends: run the user's Claude, Codex, or Grok
//! subscription by spawning the official vendor CLI as a subprocess.
//!
//! ShadowCode never reads, stores, proxies, or re-implements vendor OAuth
//! tokens. The user logs in once with the vendor CLI (`claude auth login`,
//! `codex login`, `grok login`); ShadowCode only spawns the binary in the
//! trusted workspace and translates its event stream into the existing
//! transcript model. In this mode the VENDOR agent runs the agentic loop with
//! its own tools and sandbox; ShadowCode is the workspace, transcript, review,
//! approval, and steering shell. ShadowCode's native tools and bubblewrap are
//! never injected into the vendor process.
//!
//! Every adapter is a pure line-oriented state machine (`CliAdapter`), so the
//! protocol translation is tested from recorded fixture streams without the
//! real CLIs or logins. `runner` owns the process, stdin/stdout plumbing,
//! approvals, cancellation, and steering.
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;

pub mod acp;
pub mod claude;
pub mod codex;
pub mod doctor;
#[cfg(unix)]
pub mod runner;

/// Provider prefix used in `ModelConfig.provider` for vendor CLI backends.
pub const PROVIDER_PREFIX: &str = "cli:";
/// Longest accepted NDJSON line from a vendor process. Longer lines are
/// dropped as malformed rather than buffered without bound.
pub const MAX_LINE_BYTES: usize = 4_000_000;
/// Consecutive malformed lines tolerated before the run is failed.
pub const MAX_MALFORMED_LINES: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Vendor {
    Codex,
    Grok,
    Claude,
}
impl Vendor {
    pub const ALL: [Vendor; 3] = [Vendor::Codex, Vendor::Grok, Vendor::Claude];
    pub fn id(self) -> &'static str {
        match self {
            Vendor::Codex => "codex",
            Vendor::Grok => "grok",
            Vendor::Claude => "claude",
        }
    }
    pub fn provider(self) -> String {
        format!("{PROVIDER_PREFIX}{}", self.id())
    }
    pub fn label(self) -> &'static str {
        match self {
            Vendor::Codex => "Codex (vendor agent)",
            Vendor::Grok => "Grok (vendor agent)",
            Vendor::Claude => "Claude (vendor agent)",
        }
    }
    pub fn binary(self) -> &'static str {
        self.id()
    }
    pub fn login_hint(self) -> &'static str {
        match self {
            Vendor::Codex => "Run `codex login` in a terminal, then re-run Doctor.",
            Vendor::Grok => "Run `grok login` in a terminal, then re-run Doctor.",
            Vendor::Claude => "Run `claude auth login` in a terminal, then re-run Doctor.",
        }
    }
    pub fn install_hint(self) -> &'static str {
        match self {
            Vendor::Codex => "Install the Codex CLI (`npm i -g @openai/codex`) so `codex` is on PATH.",
            Vendor::Grok => "Install the Grok CLI so `grok` is on PATH (see https://docs.x.ai/build).",
            Vendor::Claude => "Install Claude Code so `claude` is on PATH (see https://code.claude.com/docs/en/headless).",
        }
    }
    pub fn from_provider(provider: &str) -> Option<Vendor> {
        match provider.strip_prefix(PROVIDER_PREFIX)? {
            "codex" => Some(Vendor::Codex),
            "grok" => Some(Vendor::Grok),
            "claude" => Some(Vendor::Claude),
            _ => None,
        }
    }
    pub fn parse(id: &str) -> Option<Vendor> {
        Self::from_provider(&format!("{PROVIDER_PREFIX}{id}"))
    }
}

pub fn is_cli_provider(provider: &str) -> bool {
    Vendor::from_provider(provider).is_some()
}

/// A vendor permission prompt routed through ShadowCode's Allow/Deny approvals.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApprovalPrompt {
    /// Opaque protocol-level id the adapter needs to answer the request.
    pub request_id: String,
    /// `command`, `file_change`, `permissions`, or `tool`.
    pub kind: String,
    /// Tool label shown in the approval card, e.g. `codex.command_execution`.
    pub tool: String,
    /// Human-readable summary (shell command, file list, or tool name).
    pub command: String,
    pub reason: String,
    pub arguments: Value,
}

/// Translated vendor events. These map onto the existing transcript
/// vocabulary (`model.stream`, `tool.started`, `tool.completed`, ...).
#[derive(Clone, Debug, PartialEq)]
pub enum Update {
    /// Streamed assistant text.
    Text(String),
    ToolStarted {
        id: String,
        name: String,
        detail: Value,
    },
    ToolCompleted {
        id: String,
        name: String,
        success: bool,
        output: Value,
    },
    /// Files the vendor agent reported changing.
    FilesChanged { paths: Vec<String>, detail: Value },
    /// A vendor prompt that needs a user decision.
    Approval(ApprovalPrompt),
    /// Non-fatal diagnostic (shown as a warning, never fails the task).
    Warning(String),
    /// Token usage reported by the vendor, when the protocol carries it.
    Usage { input: u64, output: u64 },
    /// The current turn finished. `text` is the final assistant message when
    /// the protocol delivers one that was not already streamed.
    TurnCompleted {
        text: Option<String>,
        interrupted: bool,
    },
    /// The current turn failed; the run ends with this error.
    TurnFailed(String),
}

/// Result of feeding one line into an adapter.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Step {
    /// Lines to write to the vendor's stdin, in order, each without newline.
    pub send: Vec<String>,
    pub updates: Vec<Update>,
}
impl Step {
    pub fn send(line: String) -> Self {
        Self {
            send: vec![line],
            updates: Vec::new(),
        }
    }
    pub fn update(update: Update) -> Self {
        Self {
            send: Vec::new(),
            updates: vec![update],
        }
    }
    pub fn merge(mut self, other: Step) -> Self {
        self.send.extend(other.send);
        self.updates.extend(other.updates);
        self
    }
}

/// Launch options shared by every adapter.
#[derive(Clone, Debug)]
pub struct LaunchOptions {
    pub binary: String,
    pub workspace: std::path::PathBuf,
    /// Vendor model id; empty or `default` keeps the vendor's own default.
    pub model: String,
    /// Plan/review tasks request the vendor's read-only or plan mode where the
    /// protocol offers one.
    pub read_only: bool,
}

/// A pure protocol translator. It never touches processes or the network.
pub trait CliAdapter: Send {
    fn vendor(&self) -> Vendor;
    /// Program and arguments to spawn.
    fn command(&self, options: &LaunchOptions) -> (String, Vec<String>);
    /// Lines to write immediately after spawn (handshake).
    fn on_start(&mut self, options: &LaunchOptions) -> Vec<String>;
    /// True once the handshake finished and prompts can be sent.
    fn ready(&self) -> bool;
    /// Queue a user turn. Adapters buffer it until `ready()` and flush it
    /// from `on_line`, so callers may prompt right after `on_start`.
    fn prompt(&mut self, text: &str) -> Result<Vec<String>>;
    /// Translate one stdout line. Malformed lines return `Ok` with a
    /// `Warning`; only protocol-fatal conditions return `Err`.
    fn on_line(&mut self, line: &str) -> Result<Step>;
    /// Answer a previously surfaced `ApprovalPrompt`.
    fn approve(&mut self, request_id: &str, approve: bool) -> Result<Vec<String>>;
    /// Interrupt the running turn (Pause). Empty when unsupported.
    fn interrupt(&mut self) -> Vec<String>;
    /// True when the vendor process is expected to exit on its own after the
    /// final result (one-shot protocols); the runner then waits for exit
    /// instead of terminating the process.
    fn one_shot(&self) -> bool {
        false
    }
}

/// Construct the adapter for a vendor. `codex_exec_fallback` selects the
/// one-shot `codex exec --json` translator when `codex app-server` is not
/// available.
pub fn adapter_for(vendor: Vendor, codex_exec_fallback: bool) -> Box<dyn CliAdapter> {
    match vendor {
        Vendor::Codex if codex_exec_fallback => Box::new(codex::CodexExecAdapter::default()),
        Vendor::Codex => Box::new(codex::CodexAppServerAdapter::default()),
        Vendor::Grok => Box::new(acp::AcpAdapter::new(Vendor::Grok)),
        Vendor::Claude => Box::new(claude::ClaudeAdapter::default()),
    }
}

/// Per-vendor knobs from `config.cli_agents`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CliAgentsConfig {
    /// Master switch for spawning vendor CLIs.
    pub enabled: bool,
    /// Anthropic tolerates driving the official `claude` binary with the
    /// user's own login but does not guarantee it; this flag lets users opt
    /// out entirely (see docs/NATIVE_CLI_BACKENDS.md).
    pub claude_enabled: bool,
    pub codex_binary: String,
    pub grok_binary: String,
    pub claude_binary: String,
    /// Seconds a vendor approval prompt waits for the user before it is denied.
    pub approval_timeout_sec: u64,
    /// Seconds without any stdout line before the run is considered stalled.
    pub stall_timeout_sec: u64,
}
impl Default for CliAgentsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            claude_enabled: true,
            codex_binary: "codex".into(),
            grok_binary: "grok".into(),
            claude_binary: "claude".into(),
            approval_timeout_sec: 600,
            stall_timeout_sec: 900,
        }
    }
}
impl CliAgentsConfig {
    pub fn from_value(value: &Value) -> Result<Self> {
        let config: Self = serde_json::from_value(value.clone())
            .map_err(|e| anyhow::anyhow!("Invalid cli_agents configuration: {e}"))?;
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> Result<()> {
        for (name, value) in [
            ("codex_binary", &self.codex_binary),
            ("grok_binary", &self.grok_binary),
            ("claude_binary", &self.claude_binary),
        ] {
            if value.trim().is_empty() || value.len() > 1024 || value.contains(['\n', '\0']) {
                bail!("cli_agents.{name} must be a non-empty executable name or path");
            }
        }
        if !(10..=86400).contains(&self.approval_timeout_sec) {
            bail!("cli_agents.approval_timeout_sec must be between 10 and 86400");
        }
        if !(30..=86400).contains(&self.stall_timeout_sec) {
            bail!("cli_agents.stall_timeout_sec must be between 30 and 86400");
        }
        Ok(())
    }
    pub fn binary(&self, vendor: Vendor) -> &str {
        match vendor {
            Vendor::Codex => &self.codex_binary,
            Vendor::Grok => &self.grok_binary,
            Vendor::Claude => &self.claude_binary,
        }
    }
    pub fn vendor_enabled(&self, vendor: Vendor) -> bool {
        self.enabled && (vendor != Vendor::Claude || self.claude_enabled)
    }
}

/// Model configuration for a vendor CLI. Empty endpoint, unused key name.
/// ShadowCode never reads a vendor credential for this target.
pub fn vendor_model(vendor: Vendor, model_name: Option<&str>) -> crate::config::ModelConfig {
    let name = model_name
        .map(str::trim)
        .filter(|name| !name.is_empty() && *name != vendor.provider())
        .unwrap_or("default")
        .to_owned();
    crate::config::ModelConfig {
        default: vendor.provider(),
        provider: vendor.provider(),
        endpoint: String::new(),
        api_key_env: "UNUSED".into(),
        name,
        context_limit: 200_000,
        keep_alive: "30m".into(),
    }
}

/// Resolve `cli:codex` / `cli:grok` / `cli:claude` (and their default names).
pub fn resolve_vendor(id: &str) -> Option<crate::config::ModelConfig> {
    let trimmed = id.trim();
    if let Some(vendor) = Vendor::from_provider(trimmed) {
        return Some(vendor_model(vendor, None));
    }
    for vendor in Vendor::ALL {
        if trimmed == vendor.id()
            || trimmed == vendor.label()
            || trimmed.eq_ignore_ascii_case(vendor.label())
        {
            return Some(vendor_model(vendor, None));
        }
    }
    None
}

/// Picker rows for enabled vendor backends.
pub fn catalog_models(config: &CliAgentsConfig) -> Vec<Value> {
    Vendor::ALL
        .into_iter()
        .filter(|vendor| config.vendor_enabled(*vendor))
        .map(|vendor| {
            json!({
                "id": vendor.provider(),
                "name": vendor.label(),
                "provider": vendor.provider(),
                "endpoint": "",
                "context_limit": 200000,
                "metadata": {
                    "vendor_agent": true,
                    "kind": vendor.id(),
                    "label": vendor.label(),
                    "login_hint": vendor.login_hint(),
                }
            })
        })
        .collect()
}

/// Resolve a vendor to its configured binary, checking PATH when the value is
/// a bare name.
pub fn resolve_binary(configured: &str) -> Option<std::path::PathBuf> {
    let candidate = Path::new(configured);
    if candidate.components().count() > 1 {
        return candidate.is_file().then(|| candidate.to_path_buf());
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(configured))
        .find(|p| p.is_file())
}

/// Truncate vendor output before it enters the transcript or store.
pub(crate) fn clip(text: &str, limit: usize) -> String {
    crate::tools::truncate(text, limit).to_owned()
}

/// Redact what the vendor printed before it is shown or stored.
pub(crate) fn redact(text: &str) -> String {
    crate::redaction::redact_text(text).text
}

pub(crate) fn redact_value(mut value: Value) -> Value {
    crate::redaction::redact_value(&mut value);
    value
}
