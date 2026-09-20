//! Routing picks a registered configuration once when a task is queued. It
//! never changes permissions or retries a failed request against another host.
use crate::{
    config::{Config, ModelConfig},
    model_registry,
    store::Store,
};
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const PURPOSES: [&str; 8] = [
    "planner",
    "coder",
    "reviewer",
    "tester",
    "architecture",
    "small_edits",
    "vision",
    "local",
];
pub fn purpose(value: &str, mode: &str) -> Result<&'static str> {
    Ok(match value {
        "" => match mode {
            "plan" => "planner",
            "review" => "reviewer",
            _ => "coder",
        },
        "planner" | "planning" | "plan" | "researcher" => "planner",
        "coder" | "coding" | "code" | "debugger" => "coder",
        "reviewer" | "review" => "reviewer",
        "tester" | "test" => "tester",
        "architecture" => "architecture",
        "small_edits" => "small_edits",
        "vision" => "vision",
        "local" => "local",
        _ => anyhow::bail!("Unknown task purpose: {value}"),
    })
}
pub fn validate(value: &Value) -> Result<()> {
    let object = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("Routing must be an object"))?;
    for (key, value) in object {
        if key == "enabled" {
            ensure!(value.is_boolean(), "routing.enabled must be a boolean");
        } else {
            ensure!(
                PURPOSES.contains(&key.as_str()),
                "Unknown routing purpose: {key}"
            );
            ensure!(
                value.as_str().is_some_and(|value| value.len() <= 1024),
                "Routing model IDs must be strings of at most 1024 bytes"
            );
        }
    }
    Ok(())
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Decision {
    pub purpose: String,
    pub source: String,
    pub requested: String,
    pub model_id: String,
    pub model_name: String,
    pub provider: String,
    pub context_limit: usize,
    pub fallback_reason: Option<String>,
}
pub fn select(
    store: &Store,
    config: &Config,
    override_model: Option<ModelConfig>,
    role: &str,
) -> Result<(ModelConfig, Decision)> {
    let mut decision = Decision {
        purpose: role.into(),
        ..Default::default()
    };
    let model = if let Some(model) = override_model {
        decision.source = "explicit".into();
        decision.requested = model.default.clone();
        model
    } else {
        let requested = config
            .routing
            .get(role)
            .and_then(Value::as_str)
            .unwrap_or("");
        // "mock" was the legacy per-role placeholder, not an installed model.
        if config.routing["enabled"] != true || matches!(requested, "" | "default" | "mock") {
            decision.source = "default".into();
            decision.requested = config.model.default.clone();
            config.model.clone()
        } else {
            decision.requested = requested.into();
            match model_registry::resolve(store, requested, &config.model) {
                Ok(model) if model.provider != "mock" => {
                    decision.source = "purpose".into();
                    model
                }
                result => {
                    decision.source = "fallback".into();
                    decision.fallback_reason = Some(match result {
                        Err(error) => error.to_string(),
                        _ => "The routed model is an offline preview".into(),
                    });
                    config.model.clone()
                }
            }
        }
    };
    model_registry::validate(&model)?;
    decision.model_id = model.default.clone();
    decision.model_name = model.name.clone();
    decision.provider = model.provider.clone();
    decision.context_limit = model.context_limit;
    Ok((model, decision))
}
pub fn view(store: &Store, config: &Config) -> Result<Value> {
    let mut table = serde_json::Map::new();
    let mut decisions = serde_json::Map::new();
    for role in PURPOSES {
        let (_, decision) = select(store, config, None, role)?;
        table.insert(role.into(), json!(decision.model_id));
        decisions.insert(role.into(), json!(decision));
    }
    Ok(
        json!({"enabled":config.routing["enabled"]==true,"default":config.model.default,"default_name":config.model.name,"table":table,"config":config.routing,"decisions":decisions,"models":model_registry::catalog(store,&config.model)?}),
    )
}
