//! The small common runtime contract (spec §10), as one enum.
//!
//! Every execution target is either a vendor CLI (Codex, Claude Code,
//! Cursor, Antigravity, Grok — the vendor runs the agent loop) or ShadowCode's
//! own native loop (`Local`: a GGUF model through the managed llama.cpp
//! runtime, or a configured compatible endpoint). This facade names the ten
//! operations the rest of the app needs and dispatches with a `match`; the
//! work stays where it already lives:
//!
//! | operation            | Vendor                                   | Local (native loop)                 |
//! |----------------------|------------------------------------------|-------------------------------------|
//! | availability / auth  | `VendorCatalog::refresh` (official probes)| `local_engine::catalog` runtime state|
//! | discover models      | catalog picker rows                      | GGUF catalog entries                |
//! | capabilities         | protocol image support × model modality  | projector / chat template           |
//! | start / resume       | `native_session:<vendor>` session meta   | the conversation's message tape     |
//! | submit turn          | `Engine::start_consented` (one job)      | same                                |
//! | streaming            | durable task events (`Engine::subscribe`)| same                                |
//! | approvals            | `ApprovalHub::decide` → adapter reply     | `ApprovalHub::decide` → tool        |
//! | cancel               | `Engine::cancel` → SIGTERM process group | `Engine::cancel` → token            |
//! | usage snapshot       | official plan usage (catalog)            | "No subscription quota"             |
//! | shutdown             | `Engine::shutdown`                       | same                                |
//!
//! Provider protocols stay inside the adapters (`cli_agent::*`).
use crate::{
    cli_agent::{picker::Availability, usage::UsageSnapshot, Vendor},
    config::{Config, ModelConfig, PermissionLevel},
    engine::{Engine, Job, StartRequest},
};
use anyhow::Result;
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Runtime {
    Vendor(Vendor),
    Local,
}

/// What a target can actually do, from the runtime and the model together.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EffectiveCapabilities {
    /// Images reach the model (runtime protocol and model both accept them).
    pub vision: bool,
    /// The target can use tools (false = chat only).
    pub tools: bool,
    /// Approval prompts reach ShadowCode.
    pub asks_approval: bool,
    /// Runs on a cloud service (consent rules apply).
    pub cloud: bool,
}

impl Runtime {
    pub fn for_model(model: &ModelConfig) -> Self {
        match Vendor::from_provider(&model.provider) {
            Some(vendor) => Self::Vendor(vendor),
            None => Self::Local,
        }
    }
    /// From a picker id: `cli:*` ids are vendors, everything else runs in
    /// the native loop.
    pub fn for_target(id: &str) -> Self {
        match crate::cli_agent::resolve_vendor(id) {
            Some(model) => Self::for_model(&model),
            None => Self::Local,
        }
    }

    /// Availability and the reason shown on the row.
    pub async fn availability(&self, engine: &Engine, config: &Config) -> (Availability, String) {
        match self {
            Self::Vendor(vendor) => {
                let status = engine
                    .vendors()
                    .refresh(*vendor, &config.cli_agents, false)
                    .await;
                (status.availability, status.detail)
            }
            Self::Local if config.model.provider == "llamacpp" => {
                let catalog = crate::local_engine::catalog_with(
                    &config.local_engine,
                    Some(engine.local_runtime()),
                );
                let runtime = if catalog["runtime"].is_object() {
                    &catalog["runtime"]
                } else {
                    &catalog["llama"]
                };
                let detail = runtime["detail"].as_str().unwrap_or("").to_owned();
                match runtime["state"].as_str() {
                    Some("ready") => (Availability::Ready, detail),
                    Some("unavailable") => (Availability::Unavailable, detail),
                    _ => (Availability::SetupRequired, detail),
                }
            }
            // A configured endpoint is only known to work when a turn runs.
            Self::Local => (
                Availability::Ready,
                format!("Configured endpoint {}", config.model.endpoint),
            ),
        }
    }

    /// Model rows this runtime offers (picker JSON).
    pub async fn discover_models(&self, engine: &Engine, config: &Config) -> Vec<Value> {
        match self {
            Self::Vendor(vendor) => engine
                .vendors()
                .picker_rows(&config.cli_agents, false)
                .await
                .into_iter()
                .filter(|row| row.provider == vendor.provider())
                .map(|row| row.to_json())
                .collect(),
            Self::Local => crate::local_engine::catalog_with(
                &config.local_engine,
                Some(engine.local_runtime()),
            )["models"]
                .as_array()
                .cloned()
                .unwrap_or_default(),
        }
    }

    /// Effective capabilities of `model` on this runtime.
    pub async fn capabilities(
        &self,
        engine: &Engine,
        config: &Config,
        model: &ModelConfig,
    ) -> EffectiveCapabilities {
        match self {
            Self::Vendor(vendor) => {
                let status = engine
                    .vendors()
                    .refresh(*vendor, &config.cli_agents, false)
                    .await;
                let model_vision = status
                    .models
                    .iter()
                    .find(|m| m.id == model.name)
                    .map(|m| m.vision)
                    .unwrap_or(true);
                EffectiveCapabilities {
                    vision: status.accepts_images && model_vision,
                    tools: true,
                    asks_approval: status.asks_approval,
                    cloud: true,
                }
            }
            Self::Local => {
                let catalog = crate::local_engine::catalog_with(
                    &config.local_engine,
                    Some(engine.local_runtime()),
                );
                let entry = catalog["models"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|m| m["id"] == model.default.as_str());
                EffectiveCapabilities {
                    vision: entry.is_some_and(|m| m["vision"] == true),
                    tools: entry.is_none_or(|m| m["tools"] != false),
                    asks_approval: true,
                    cloud: !crate::cli_agent::handoff::is_local(model),
                }
            }
        }
    }

    /// Native session this conversation resumes on this runtime, if any.
    pub fn resume_id(&self, engine: &Engine, session_id: &str) -> Result<Option<String>> {
        match self {
            Self::Vendor(vendor) => engine
                .store()
                .session_meta(session_id, &format!("native_session:{}", vendor.id())),
            // The native loop continues from the conversation's message tape.
            Self::Local => Ok(None),
        }
    }

    /// Submit one turn (starts or resumes the session). Streaming activity
    /// arrives as durable task events; see `Engine::subscribe`.
    pub async fn submit_turn(
        engine: &Engine,
        request: StartRequest,
        purpose: &str,
        limit: Option<PermissionLevel>,
        handoff_consent: bool,
    ) -> Result<Job> {
        engine
            .start_consented(request, purpose, limit, handoff_consent)
            .await
    }

    /// Answer an approval prompt (vendor prompt or native tool).
    pub fn approve(
        engine: &Engine,
        approval_id: &str,
        session_id: &str,
        allow: bool,
    ) -> Result<()> {
        engine.approvals().decide(approval_id, session_id, allow)?;
        Ok(())
    }

    /// Cancel a queued or running turn.
    pub async fn cancel(engine: &Engine, job_id: &str) -> Result<Job> {
        engine.cancel(job_id).await
    }

    /// Usage for a row of this runtime. Never invented: vendors report
    /// official plan usage or "unavailable"; local rows have no quota.
    pub async fn usage_snapshot(
        &self,
        engine: &Engine,
        config: &Config,
        model: &ModelConfig,
    ) -> UsageSnapshot {
        match self {
            Self::Vendor(vendor) => engine
                .vendors()
                .refresh(*vendor, &config.cli_agents, false)
                .await
                .usage_for(&model.name, crate::now()),
            Self::Local if crate::cli_agent::handoff::is_local(model) => UsageSnapshot::local(),
            Self::Local => UsageSnapshot::unavailable_because(
                &model.provider,
                "Billed by the provider per request; ShadowCode has no plan usage for this endpoint",
            ),
        }
    }

    /// Stop every runtime process and running turn.
    pub async fn shutdown(engine: &Engine) -> Result<()> {
        engine.vendors().logins().cancel_all();
        engine.shutdown().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_selects_vendor_or_native() {
        assert_eq!(
            Runtime::for_target("cli:codex"),
            Runtime::Vendor(Vendor::Codex)
        );
        assert_eq!(
            Runtime::for_target("cli:cursor:gpt-5.5[context=272k,reasoning=medium,fast=false]"),
            Runtime::Vendor(Vendor::Cursor)
        );
        // Bare "grok" is the xAI API preset, run by the native loop.
        assert_eq!(Runtime::for_target("grok"), Runtime::Local);
        assert_eq!(Runtime::for_target("local:gguf:abc"), Runtime::Local);
        let model = crate::cli_agent::vendor_model(Vendor::Antigravity, Some("gemini-3"));
        assert_eq!(
            Runtime::for_model(&model),
            Runtime::Vendor(Vendor::Antigravity)
        );
        assert_eq!(model.default, "cli:antigravity:gemini-3");
    }
}
