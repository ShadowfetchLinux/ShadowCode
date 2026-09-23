//! Vendor catalog: the one place that knows, per subscription runtime, whether
//! it is Ready / Sign in / Setup required / Unavailable, which models it
//! offers, what it accepts on the wire, and what usage it reported.
//!
//! Every fact comes from an official interface of the installed runtime
//! (`codex app-server`, `claude auth status`, Cursor and Grok ACP handshakes,
//! `agy models`). Refreshes are bounded: a vendor is re-probed at most once per
//! `MIN_REFRESH_SECS` unless forced, and failures back off exponentially. The
//! catalog never marks a vendor Ready because a binary or credential file
//! exists, and never invents usage.
use super::{
    acp_probe, codex_probe, doctor,
    picker::{self, Availability, PickerTarget},
    resolve_binary,
    usage::UsageSnapshot,
    CliAgentsConfig, Vendor,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::sync::Mutex;

pub const MIN_REFRESH_SECS: f64 = 5.0 * 60.0;
const MAX_BACKOFF_SECS: f64 = 60.0 * 60.0;
const PROBE_TIMEOUT: Duration = Duration::from_secs(40);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VendorModel {
    /// Exact id the runtime accepts (Codex model id, ACP modelId, agy slug,
    /// Claude alias). `auto` / `default` mean the runtime's own choice.
    pub id: String,
    pub label: String,
    pub is_default: bool,
    /// The catalog says this model takes image input (Codex
    /// `inputModalities`); protocol-level support is separate.
    pub vision: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AccountInfo {
    pub email: Option<String>,
    pub plan: Option<String>,
    /// `chatgpt`, `apiKey`, `cursor_login`, `grok.com`, …
    pub auth_mode: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VendorStatus {
    pub vendor: Vendor,
    pub availability: Availability,
    pub detail: String,
    pub version: Option<String>,
    pub binary: Option<PathBuf>,
    pub account: Option<AccountInfo>,
    pub models: Vec<VendorModel>,
    /// The runtime's protocol accepts image bytes from ShadowCode.
    pub accepts_images: bool,
    /// Approval prompts reach ShadowCode (false: the runtime applies its own
    /// permission settings and never asks).
    pub asks_approval: bool,
    pub fetched_at: f64,
    pub error: Option<String>,
    /// Raw provider usage payload (Codex rate limits) used to derive per-model
    /// snapshots; None when the provider exposes nothing.
    #[serde(skip)]
    pub usage_raw: Option<Value>,
    /// Why usage is unavailable, when it is.
    pub usage_note: Option<String>,
    #[serde(skip)]
    pub failures: u32,
    #[serde(skip)]
    pub next_allowed: f64,
}

impl VendorStatus {
    fn setup_required(vendor: Vendor, configured: &str, now: f64) -> Self {
        Self {
            vendor,
            availability: Availability::SetupRequired,
            detail: format!("Not installed: `{configured}` was not found on PATH. {}", vendor.install_hint()),
            version: None,
            binary: None,
            account: None,
            models: Vec::new(),
            accepts_images: false,
            asks_approval: vendor.asks_approval(),
            fetched_at: now,
            error: None,
            usage_raw: None,
            usage_note: None,
            failures: 0,
            next_allowed: now,
        }
    }
    fn disabled(vendor: Vendor, now: f64) -> Self {
        let mut status = Self::setup_required(vendor, vendor.binary(), now);
        status.availability = Availability::Unavailable;
        status.detail = "Disabled in Settings › Advanced; ShadowCode will not start this runtime".into();
        status
    }
    /// Usage for one model row of this vendor.
    pub fn usage_for(&self, model: &str, now: f64) -> UsageSnapshot {
        let provider = self.vendor.provider();
        let snap = match (&self.usage_raw, self.vendor) {
            (Some(raw), Vendor::Codex) => {
                let model = if model.is_empty() || model == "default" || model == "auto" {
                    self.models.iter().find(|m| m.is_default).map(|m| m.id.as_str())
                } else {
                    Some(model)
                };
                UsageSnapshot::from_codex(raw, model, self.fetched_at)
            }
            _ => UsageSnapshot::unavailable_because(
                &provider,
                self.usage_note.as_deref().unwrap_or(""),
            ),
        };
        if snap.is_stale(now) {
            snap.mark_stale(now)
        } else {
            snap
        }
    }
    /// Compact doctor-style JSON kept for the Accounts page and Doctor.
    pub fn to_doctor_json(&self) -> Value {
        let state = match self.availability {
            Availability::Ready => "ready",
            Availability::SignIn => "not_logged_in",
            Availability::SetupRequired => "not_installed",
            Availability::Unavailable => "unavailable",
        };
        let status = match self.availability {
            Availability::Ready => "pass",
            Availability::Unavailable => "info",
            _ => "warn",
        };
        json!({
            "id": format!("cli-{}", self.vendor.id()),
            "label": format!("{} via `{}` CLI", self.vendor.product_label(), self.vendor.binary()),
            "state": state,
            "status": status,
            "availability": self.availability,
            "availability_label": self.availability.label(),
            "detail": self.detail,
            "version": self.version,
            "binary": self.binary,
            "fix": match self.availability {
                Availability::SignIn => self.vendor.login_hint(),
                Availability::SetupRequired => self.vendor.install_hint(),
                _ => "",
            },
            "account": self.account,
            "models": self.models,
            "accepts_images": self.accepts_images,
            "asks_approval": self.asks_approval,
            "fetched_at": self.fetched_at,
            "error": self.error,
            "usage_note": self.usage_note,
            "login_command": self.vendor.login_command(),
            "logout_command": self.vendor.logout_command(),
            "shared_cli_note": self.vendor.shared_cli_note(),
        })
    }
}

#[derive(Default)]
pub struct VendorCatalog {
    entries: Mutex<HashMap<Vendor, VendorStatus>>,
}

impl VendorCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Forget everything known about a vendor (after connect/disconnect).
    pub async fn clear(&self, vendor: Vendor) {
        self.entries.lock().await.remove(&vendor);
    }

    pub async fn cached(&self, vendor: Vendor) -> Option<VendorStatus> {
        self.entries.lock().await.get(&vendor).cloned()
    }

    /// Refresh one vendor unless a recent probe exists (or backoff applies).
    pub async fn refresh(&self, vendor: Vendor, config: &CliAgentsConfig, force: bool) -> VendorStatus {
        let now = crate::now();
        if let Some(existing) = self.entries.lock().await.get(&vendor) {
            let fresh = now - existing.fetched_at < MIN_REFRESH_SECS;
            let backing_off = existing.next_allowed > now;
            if (!force && fresh) || (backing_off && !force) {
                return existing.clone();
            }
        }
        let previous = self.entries.lock().await.get(&vendor).cloned();
        let mut status = probe_vendor(vendor, config, now).await;
        if status.error.is_some() {
            let failures = previous.as_ref().map(|p| p.failures + 1).unwrap_or(1);
            status.failures = failures;
            status.next_allowed = now + (30.0 * 2f64.powi(failures.min(7) as i32)).min(MAX_BACKOFF_SECS);
            // Keep the last known usage/models so stale data stays visible.
            if let Some(previous) = previous {
                if status.usage_raw.is_none() {
                    status.usage_raw = previous.usage_raw;
                    status.fetched_at = previous.fetched_at.min(now);
                }
                if status.models.is_empty() {
                    status.models = previous.models;
                }
            }
        }
        self.entries.lock().await.insert(vendor, status.clone());
        status
    }

    /// Refresh every enabled vendor concurrently.
    pub async fn refresh_all(&self, config: &CliAgentsConfig, force: bool) -> Vec<VendorStatus> {
        let futures = Vendor::ALL
            .into_iter()
            .map(|vendor| self.refresh(vendor, config, force));
        futures_util::future::join_all(futures).await
    }

    /// Doctor-style map `{vendor: {...}}` for the Accounts page.
    pub async fn status_json(&self, config: &CliAgentsConfig, force: bool) -> Value {
        let mut map = serde_json::Map::new();
        for status in self.refresh_all(config, force).await {
            map.insert(status.vendor.id().to_owned(), status.to_doctor_json());
        }
        Value::Object(map)
    }

    /// Picker rows for every enabled vendor: one row per discovered model,
    /// or a single Default/Sign in/Setup required row when none are known.
    pub async fn picker_rows(&self, config: &CliAgentsConfig, force: bool) -> Vec<PickerTarget> {
        let now = crate::now();
        let mut rows = Vec::new();
        for status in self.refresh_all(config, force).await {
            let vendor = status.vendor;
            if !config.vendor_enabled(vendor) {
                continue;
            }
            let ready = status.availability == Availability::Ready;
            if ready && !status.models.is_empty() {
                for model in &status.models {
                    let usage = status.usage_for(&model.id, now);
                    let mut row = picker::vendor_target(
                        vendor,
                        &model.id,
                        &model.label,
                        Availability::Ready,
                        &status.detail,
                        usage,
                        status.accepts_images && model.vision,
                    );
                    row.is_default = model.is_default;
                    rows.push(row);
                }
            } else {
                let usage = if ready {
                    status.usage_for("default", now)
                } else {
                    UsageSnapshot::unavailable_because(&vendor.provider(), &status.detail)
                };
                rows.push(picker::vendor_target(
                    vendor,
                    "default",
                    "Default",
                    status.availability,
                    &status.detail,
                    usage,
                    status.accepts_images,
                ));
            }
        }
        rows
    }
}

fn version_of(binary: &Path) -> impl std::future::Future<Output = Option<String>> + '_ {
    doctor::version(binary, None)
}

async fn probe_vendor(vendor: Vendor, config: &CliAgentsConfig, now: f64) -> VendorStatus {
    if !config.vendor_enabled(vendor) {
        return VendorStatus::disabled(vendor, now);
    }
    let configured = config.binary(vendor);
    let Some(binary) = resolve_binary(configured) else {
        return VendorStatus::setup_required(vendor, configured, now);
    };
    let version = version_of(&binary).await;
    let mut status = VendorStatus {
        vendor,
        availability: Availability::Unavailable,
        detail: String::new(),
        version,
        binary: Some(binary.clone()),
        account: None,
        models: Vec::new(),
        accepts_images: false,
        asks_approval: vendor.asks_approval(),
        fetched_at: now,
        error: None,
        usage_raw: None,
        usage_note: None,
        failures: 0,
        next_allowed: now,
    };
    match vendor {
        Vendor::Codex => probe_codex(&binary, &mut status).await,
        Vendor::Claude => probe_claude(&binary, &mut status).await,
        Vendor::Cursor => probe_acp_vendor(&binary, &["acp"], &mut status).await,
        Vendor::Grok => probe_acp_vendor(&binary, &["agent", "stdio"], &mut status).await,
        Vendor::Antigravity => probe_antigravity(&binary, &mut status).await,
    }
    status
}

fn ready_detail(status: &VendorStatus) -> String {
    let version = status.version.as_deref().unwrap_or("version unknown");
    let mut parts = vec![format!("Ready · {version}")];
    if let Some(account) = &status.account {
        if let Some(plan) = &account.plan {
            parts.push(format!("plan {plan}"));
        }
        if let Some(email) = &account.email {
            parts.push(email.clone());
        }
    }
    parts.join(" · ")
}

async fn probe_codex(binary: &Path, status: &mut VendorStatus) {
    match codex_probe::probe(binary, None, PROBE_TIMEOUT).await {
        Ok(probe) => {
            status.accepts_images = true; // documented `localImage` input
            status.models = codex_probe::models_from_list(&probe.models)
                .into_iter()
                .map(|m| VendorModel {
                    id: m.id,
                    label: m.label,
                    is_default: m.is_default,
                    vision: m.vision,
                })
                .collect();
            if probe.logged_in() {
                status.account = Some(AccountInfo {
                    email: probe.email(),
                    plan: probe.plan_type(),
                    auth_mode: Some(probe.auth_mode()),
                });
                if probe.subscription_login() {
                    status.usage_raw = probe.rate_limits.clone();
                    if status.usage_raw.is_none() {
                        status.usage_note = Some(
                            "Codex did not report rate limits for this login".into(),
                        );
                    }
                } else {
                    status.usage_note = Some(
                        "Signed in with an API key: usage is billed per token, not a plan allowance".into(),
                    );
                }
                status.availability = Availability::Ready;
                status.detail = ready_detail(status);
            } else {
                status.availability = Availability::SignIn;
                status.detail = format!(
                    "Installed ({}) but not signed in",
                    status.version.as_deref().unwrap_or("version unknown")
                );
            }
            for (method, error) in probe.errors {
                status.detail.push_str(&format!(" · {method}: {error}"));
            }
        }
        Err(error) => {
            // Fall back to the documented login status command; models and
            // usage stay unknown rather than guessed.
            let state = doctor::codex_login_status(binary, None).await;
            status.error = Some(format!("app-server probe failed: {error}"));
            status.usage_note = Some("Codex app-server did not answer; usage not refreshed".into());
            match state {
                doctor::LoginState::LoggedIn => {
                    status.availability = Availability::Ready;
                    status.detail = format!("{} (app-server unavailable: {error})", ready_detail(status));
                }
                doctor::LoginState::NotLoggedIn => {
                    status.availability = Availability::SignIn;
                    status.detail = "Installed but not signed in".into();
                }
                doctor::LoginState::Unknown => {
                    status.availability = Availability::Unavailable;
                    status.detail = format!("Codex did not respond: {error}");
                }
            }
        }
    }
}

async fn probe_claude(binary: &Path, status: &mut VendorStatus) {
    status.accepts_images = true; // documented image source blocks
    status.usage_note = Some(
        "Claude Code does not expose plan usage to other apps; open claude.ai to see it".into(),
    );
    match doctor::claude_login_state(binary, None).await {
        doctor::LoginState::LoggedIn => {
            status.availability = Availability::Ready;
            status.models = claude_models(binary).await;
            status.detail = ready_detail(status);
        }
        doctor::LoginState::NotLoggedIn => {
            status.availability = Availability::SignIn;
            status.detail = format!(
                "Installed ({}) but not signed in",
                status.version.as_deref().unwrap_or("version unknown")
            );
        }
        doctor::LoginState::Unknown => {
            status.availability = Availability::Unavailable;
            status.error = Some("`claude auth status` did not report a login state".into());
            status.detail = "Claude Code did not report its login state".into();
        }
    }
}

/// Claude Code has no model-list command. The row set is its own default
/// plus the aliases the installed CLI documents in `--help` for `--model`.
async fn claude_models(binary: &Path) -> Vec<VendorModel> {
    let mut models = vec![VendorModel {
        id: "default".into(),
        label: "Default".into(),
        is_default: true,
        vision: true,
    }];
    let help = doctor::help_text(binary, None).await.unwrap_or_default();
    for alias in ["fable", "opus", "sonnet", "haiku"] {
        if help.contains(&format!("'{alias}'")) {
            models.push(VendorModel {
                id: alias.into(),
                label: format!("{} (alias)", capitalize(alias)),
                is_default: false,
                vision: true,
            });
        }
    }
    models
}

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

async fn probe_acp_vendor(binary: &Path, args: &[&str], status: &mut VendorStatus) {
    let vendor = status.vendor;
    let workspace = std::env::temp_dir();
    status.usage_note = Some(match vendor {
        Vendor::Cursor => "Cursor reports the plan tier only, not remaining allowance".into(),
        _ => "Grok reports per-session tokens only, not plan allowance".into(),
    });
    match acp_probe::probe(binary, args, &workspace, None, PROBE_TIMEOUT).await {
        Ok(probe) => {
            status.accepts_images = probe.accepts_images;
            status.models = probe
                .models
                .iter()
                .map(|m| VendorModel {
                    id: m.id.clone(),
                    label: m.label.clone(),
                    is_default: m.current || (m.id == "auto" && probe.current_model.is_none()),
                    vision: probe.accepts_images,
                })
                .collect();
            if status.models.iter().all(|m| !m.is_default) {
                if let Some(first) = status.models.first_mut() {
                    first.is_default = true;
                }
            }
            let login_error = probe
                .session_error
                .as_deref()
                .or(match probe.authenticated {
                    Some(false) => Some("authenticate rejected"),
                    _ => None,
                });
            if probe.session_started && probe.authenticated != Some(false) {
                status.availability = Availability::Ready;
                status.account = Some(AccountInfo {
                    email: None,
                    plan: None,
                    auth_mode: probe.auth_methods.first().cloned(),
                });
                status.detail = ready_detail(status);
                if let Some(email) = cursor_email(vendor, binary).await {
                    if let Some(account) = status.account.as_mut() {
                        account.email = Some(email);
                    }
                    status.detail = ready_detail(status);
                }
            } else if let Some(error) = login_error {
                let lower = error.to_ascii_lowercase();
                if lower.contains("login") || lower.contains("auth") || lower.contains("sign") {
                    status.availability = Availability::SignIn;
                    status.detail = format!("Installed but not signed in ({error})");
                } else {
                    status.availability = Availability::Unavailable;
                    status.error = Some(error.to_owned());
                    status.detail = format!("{} could not open a session: {error}", vendor.product_label());
                }
            } else {
                status.availability = Availability::Unavailable;
                status.error = Some("no session".into());
                status.detail = format!("{} did not open a session", vendor.product_label());
            }
        }
        Err(error) => {
            status.error = Some(error.to_string());
            status.availability = Availability::Unavailable;
            status.detail = format!("{} did not respond: {error}", vendor.product_label());
            if vendor == Vendor::Grok {
                // Cheaper documented fallback.
                if let Some((logged_in, models)) = doctor::grok_models(binary, None).await {
                    status.models = models
                        .into_iter()
                        .map(|m| VendorModel { id: m.id, label: m.label, is_default: m.current, vision: false })
                        .collect();
                    status.availability = if logged_in { Availability::Ready } else { Availability::SignIn };
                    status.detail = if logged_in { ready_detail(status) } else { "Installed but not signed in".into() };
                    status.error = None;
                }
            }
        }
    }
}

/// `cursor-agent status` prints the signed-in email; used for display only.
async fn cursor_email(vendor: Vendor, binary: &Path) -> Option<String> {
    if vendor != Vendor::Cursor {
        return None;
    }
    let text = doctor::short_text(binary, &["status"], None).await?;
    text.lines()
        .find_map(|line| line.split("Logged in as").nth(1))
        .map(|rest| rest.trim().trim_end_matches('.').to_owned())
        .filter(|s| s.contains('@'))
}

async fn probe_antigravity(binary: &Path, status: &mut VendorStatus) {
    status.accepts_images = false; // stream-json input is text only
    status.asks_approval = false;
    status.usage_note = Some(
        "Antigravity CLI shows usage only in its interactive /usage panel; nothing machine-readable is exposed".into(),
    );
    match doctor::short_text(binary, &["models"], None).await {
        Some(text) => {
            let models = super::discovery::parse_agy_models(&text);
            let lower = text.to_ascii_lowercase();
            if !models.is_empty() {
                status.availability = Availability::Ready;
                status.models = models
                    .into_iter()
                    .enumerate()
                    .map(|(i, m)| VendorModel { id: m.id, label: m.label, is_default: i == 0, vision: false })
                    .collect();
                status.detail = ready_detail(status);
            } else if lower.contains("login") || lower.contains("sign in") || lower.contains("auth") {
                status.availability = Availability::SignIn;
                status.detail = "Installed but not signed in".into();
            } else {
                status.availability = Availability::Unavailable;
                status.error = Some("`agy models` returned no models".into());
                status.detail = format!("Antigravity did not list models: {}", super::clip(&text, 200));
            }
        }
        None => {
            status.availability = Availability::Unavailable;
            status.error = Some("`agy models` did not respond".into());
            status.detail = "Antigravity CLI did not respond".into();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_for_codex_rows_uses_pools_and_others_stay_unavailable() {
        let now = crate::now();
        let mut status = VendorStatus::setup_required(Vendor::Codex, "codex", now);
        status.availability = Availability::Ready;
        status.models = vec![
            VendorModel { id: "gpt-6-astra".into(), label: "GPT-6-Astra".into(), is_default: true, vision: true },
            VendorModel { id: "gpt-5.6-luna".into(), label: "GPT-5.6-Luna".into(), is_default: false, vision: true },
        ];
        status.usage_raw = Some(json!({
            "rateLimits": {"limitId":"codex","primary":{"usedPercent":40,"windowDurationMins":10080},"planType":"pro"},
            "rateLimitsByLimitId": {
                "codex": {"limitId":"codex","primary":{"usedPercent":40,"windowDurationMins":10080},"planType":"pro"},
                "base_model_inference": {"limitId":"base_model_inference","limitName":"gpt-reserve","normalModelSlug":"gpt-5.6-luna","primary":{"usedPercent":5,"windowDurationMins":10080}}
            }
        }));
        assert_eq!(status.usage_for("default", now).remaining_percent, Some(60.0));
        assert_eq!(status.usage_for("gpt-5.6-luna", now).remaining_percent, Some(95.0));
        assert!(status.usage_for("gpt-6-astra", now).pool_shared);
        let cursor = VendorStatus::setup_required(Vendor::Cursor, "cursor-agent", now);
        assert_eq!(cursor.usage_for("auto", now).state, "unavailable");
        let doctor = cursor.to_doctor_json();
        assert_eq!(doctor["state"], "not_installed");
        assert_eq!(doctor["availability_label"], "Setup required");
    }
}
