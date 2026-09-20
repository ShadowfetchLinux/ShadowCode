use crate::paths::{atomic_write, AppPaths};
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PermissionLevel {
    ReadOnly,
    Workspace,
    Elevated,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelConfig {
    pub default: String,
    pub provider: String,
    pub endpoint: String,
    pub api_key_env: String,
    pub name: String,
    pub context_limit: usize,
}
impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            default: "mock".into(),
            provider: "mock".into(),
            endpoint: String::new(),
            api_key_env: "OPENAI_API_KEY".into(),
            name: "mock-coder".into(),
            context_limit: 128_000,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct PermissionsConfig {
    pub level: PermissionLevel,
    pub require_approval_for_dangerous: bool,
    pub network: bool,
    pub allow_root: bool,
    pub profile: String,
    /// Native shell tools require explicit approval unless their exact command
    /// is approved for this task. File tools remain usable in workspace mode.
    pub approve_shell: bool,
}
impl Default for PermissionsConfig {
    fn default() -> Self {
        Self {
            level: PermissionLevel::Workspace,
            require_approval_for_dangerous: true,
            network: false,
            allow_root: false,
            profile: String::new(),
            approve_shell: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentConfig {
    pub max_steps: usize,
    pub tool_timeout_sec: u64,
    pub parallel_reads: bool,
    pub compact_ratio: f64,
    pub model_retries: usize,
    pub retry_backoff_sec: f64,
    pub max_fix_retries: usize,
    pub max_output_bytes: usize,
    pub max_task_tokens: u64,
}
impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_steps: 64,
            tool_timeout_sec: 60,
            parallel_reads: true,
            compact_ratio: 0.7,
            model_retries: 3,
            retry_backoff_sec: 1.0,
            max_fix_retries: 3,
            max_output_bytes: 256_000,
            max_task_tokens: 1_000_000,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub model: ModelConfig,
    pub permissions: PermissionsConfig,
    pub agent: AgentConfig,
    pub ui: Value,
    pub onboarding: Value,
    pub routing: Value,
    pub mcp: Value,
    pub hooks: crate::hooks::HookConfig,
    pub git: Value,
    pub logging: Value,
    pub trusted_workspaces: Vec<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            model: ModelConfig::default(),
            permissions: PermissionsConfig::default(),
            agent: AgentConfig::default(),
            ui: json!({"theme":"light","notify":true,"notify_after_sec":4,"ability":"none","host":"127.0.0.1","port":7430}),
            onboarding: json!({"completed":false,"workspace":""}),
            routing: json!({"enabled":false,"planner":"","coder":"","reviewer":"","tester":""}),
            mcp: json!({"servers":[]}),
            hooks: crate::hooks::HookConfig::default(),
            git: json!({"auto_commit":false,"allow_destructive":false}),
            logging: json!({"level":"info"}),
            trusted_workspaces: Vec::new(),
            extra: BTreeMap::new(),
        }
    }
}

impl Config {
    pub fn load(paths: &AppPaths, workspace: Option<&Path>) -> Result<Self> {
        let mut base = serde_json::to_value(Self::default())?;
        if paths.config_file().exists() {
            merge(&mut base, read_yaml(&paths.config_file())?);
        }
        let mut config: Self =
            serde_json::from_value(base.clone()).context("Invalid user configuration")?;
        config.validate()?;
        if let Some(workspace) = workspace {
            let canonical = workspace.canonicalize()?;
            let overlay = canonical.join(".shadow/config/config.yaml");
            if config.is_trusted(&canonical) && overlay.exists() {
                ensure!(
                    overlay.canonicalize()?.starts_with(&canonical),
                    "Project configuration escapes the workspace"
                );
                let project = read_yaml(&overlay)?;
                // Repository files cannot grant permissions, alter credentials,
                // register executable hooks/MCP servers, or redirect model traffic.
                if let Some(agent) = project.get("agent") {
                    merge(&mut base["agent"], agent.clone());
                }
                if project
                    .pointer("/permissions/level")
                    .and_then(Value::as_str)
                    == Some("read_only")
                {
                    base["permissions"]["level"] = json!("read_only");
                }
                config = serde_json::from_value(base)?;
                config.validate()?;
            }
        }
        Ok(config)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1024..=4_000_000).contains(&self.model.context_limit),
            "Context limit must be between 1024 and 4000000"
        );
        ensure!(
            (1..=1000).contains(&self.agent.max_steps),
            "Task step limit must be between 1 and 1000"
        );
        ensure!(
            (1..=3600).contains(&self.agent.tool_timeout_sec),
            "Tool timeout must be between 1 and 3600 seconds"
        );
        ensure!(
            self.agent.compact_ratio.is_finite()
                && (0.2..=0.95).contains(&self.agent.compact_ratio),
            "Invalid context compaction ratio"
        );
        ensure!(
            self.agent.model_retries <= 10 && self.agent.max_fix_retries <= 10,
            "Retry limit is too large"
        );
        ensure!(
            self.agent.retry_backoff_sec.is_finite()
                && (0.0..=30.0).contains(&self.agent.retry_backoff_sec),
            "Invalid retry delay"
        );
        ensure!(
            (4096..=4_000_000).contains(&self.agent.max_output_bytes),
            "Invalid tool output limit"
        );
        ensure!(
            self.agent.max_task_tokens > 0,
            "Token budget must be positive"
        );
        ensure!(
            valid_secret_name(&self.model.api_key_env),
            "Invalid API key environment name"
        );
        if !self.model.endpoint.is_empty() {
            let url =
                reqwest::Url::parse(&self.model.endpoint).context("Invalid model endpoint")?;
            ensure!(
                matches!(url.scheme(), "http" | "https") && url.host_str().is_some(),
                "Endpoint must use HTTP or HTTPS"
            );
            ensure!(
                url.username().is_empty() && url.password().is_none(),
                "Use a stored API key instead of credentials in an endpoint URL"
            );
        }
        ensure!(
            matches!(
                self.ui.get("theme").and_then(Value::as_str),
                Some("light" | "dark" | "system")
            ),
            "Unknown theme"
        );
        ensure!(
            self.mcp.get("servers").is_some_and(Value::is_array),
            "MCP servers must be an array"
        );
        #[cfg(unix)]
        crate::mcp::registry::validate_config(&self.mcp)?;
        crate::routing::validate(&self.routing)?;
        self.hooks.validate()?;
        Ok(())
    }
    pub fn is_trusted(&self, workspace: &Path) -> bool {
        self.trusted_workspaces
            .iter()
            .any(|p| Path::new(p).canonicalize().ok().as_deref() == Some(workspace))
    }
    pub fn save(&self, paths: &AppPaths) -> Result<()> {
        self.validate()?;
        atomic_write(
            &paths.config_file(),
            serde_yaml_ng::to_string(self)?.as_bytes(),
            true,
        )
    }
    pub fn patch(paths: &AppPaths, values: Value) -> Result<Self> {
        ensure!(values.is_object(), "Configuration patch must be an object");
        let mut current = serde_json::to_value(Self::load(paths, None)?)?;
        merge(&mut current, values);
        let config: Self = serde_json::from_value(current)?;
        config.save(paths)?;
        Ok(config)
    }
}

fn read_yaml(path: &Path) -> Result<Value> {
    ensure!(
        fs::metadata(path)?.len() <= 1_000_000,
        "Configuration is too large"
    );
    let value: Value = serde_yaml_ng::from_str(&fs::read_to_string(path)?)
        .with_context(|| format!("Invalid YAML in {}", path.display()))?;
    if value.is_null() {
        return Ok(json!({}));
    }
    ensure!(value.is_object(), "Configuration must be a mapping");
    Ok(value)
}

pub fn merge(base: &mut Value, overlay: Value) {
    match (base, overlay) {
        (Value::Object(base), Value::Object(overlay)) => {
            for (key, value) in overlay {
                merge(base.entry(key).or_insert(Value::Null), value);
            }
        }
        (base, overlay) => *base = overlay,
    }
}

pub(crate) fn valid_secret_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn secrets(paths: &AppPaths) -> Result<BTreeMap<String, String>> {
    let mut result = BTreeMap::new();
    if !paths.secrets_file().exists() {
        return Ok(result);
    }
    let text = fs::read_to_string(paths.secrets_file())?;
    ensure!(text.len() <= 1_000_000, "Secret file is too large");
    for line in text.lines().map(str::trim).filter(|l| !l.starts_with('#')) {
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            if valid_secret_name(key) {
                let value = value.trim();
                let value = if value.starts_with('"') {
                    serde_json::from_str::<String>(value)
                        .unwrap_or_else(|_| value.trim_matches('"').to_owned())
                } else {
                    value.trim_matches('\'').to_owned()
                };
                result.insert(key.into(), value);
            }
        }
    }
    Ok(result)
}

pub fn secret(paths: &AppPaths, name: &str) -> Result<Option<String>> {
    Ok(std::env::var(name)
        .ok()
        .filter(|s| !s.is_empty())
        .or(secrets(paths)?.remove(name)))
}

pub fn set_secret(paths: &AppPaths, name: &str, value: &str) -> Result<()> {
    ensure!(valid_secret_name(name), "Invalid secret name");
    if value.contains(['\n', '\r', '\0']) || value.len() > 16_384 {
        bail!("Invalid API key");
    }
    let mut values = secrets(paths)?;
    if value.is_empty() {
        values.remove(name);
    } else {
        values.insert(name.into(), value.into());
    }
    let mut out = String::new();
    for (name, value) in values {
        out.push_str(&format!("{name}={}\n", serde_json::to_string(&value)?));
    }
    atomic_write(&paths.secrets_file(), out.as_bytes(), true)
}
