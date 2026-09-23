//! Stable provider identities and model resolution. Discovery never replaces a
//! user's configured endpoint, credentials, or context limit.
use crate::{
    config::{Config, ModelConfig},
    models,
    store::Store,
    workspace::hash,
};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::collections::HashSet;

fn endpoint_key(endpoint: &str) -> String {
    reqwest::Url::parse(endpoint)
        .map(|url| url.to_string().trim_end_matches('/').to_owned())
        .unwrap_or_else(|_| endpoint.trim_end_matches('/').to_owned())
}
pub fn same_target(a: &ModelConfig, b: &ModelConfig) -> bool {
    a.provider == b.provider
        && a.name == b.name
        && endpoint_key(&a.endpoint) == endpoint_key(&b.endpoint)
}
pub fn model_id(provider: &str, endpoint: &str, name: &str) -> String {
    format!(
        "model:{}",
        hash(
            json!([provider, endpoint_key(endpoint), name])
                .to_string()
                .as_bytes()
        )
    )
}
pub fn recommended_context(provider: &str, reported: Option<u64>) -> usize {
    let cap = if matches!(provider, "ollama" | "local" | "llamacpp" | "vllm") {
        16384
    } else {
        128000
    };
    reported
        .filter(|value| *value >= 2048)
        .map(|value| value.min(cap as u64) as usize)
        .unwrap_or(cap)
}
pub fn validate(model: &ModelConfig) -> Result<()> {
    ensure!(
        !model.default.is_empty() && model.default.len() <= 1024,
        "Model ID must contain 1–1024 bytes"
    );
    ensure!(
        !model.name.is_empty() && model.name.len() <= 1024,
        "Model name must contain 1–1024 bytes"
    );
    Config {
        model: model.clone(),
        ..Default::default()
    }
    .validate()
}
pub fn record_detected(store: &Store, providers: &[Value]) -> Result<()> {
    for provider in providers {
        let name = provider["provider"]
            .as_str()
            .context("Detected provider has no name")?;
        let endpoint = provider["endpoint"]
            .as_str()
            .context("Detected provider has no endpoint")?;
        for row in provider["models"].as_array().into_iter().flatten() {
            let model_name = row["id"]
                .as_str()
                .filter(|name| !name.is_empty())
                .context("Detected model has no ID")?;
            let model = ModelConfig {
                default: model_id(name, endpoint, model_name),
                name: model_name.into(),
                provider: name.into(),
                endpoint: endpoint.into(),
                api_key_env: models::preset(name)["api_key_env"]
                    .as_str()
                    .unwrap_or("OPENAI_API_KEY")
                    .into(),
                keep_alive: "30m".into(),
                context_limit: recommended_context(name, row["context_limit"].as_u64()),
            };
            validate(&model)?;
            store.upsert_detected_model(&json!({"id":model.default,"name":model.name,"provider":model.provider,"endpoint":model.endpoint,"context_limit":model.context_limit,"metadata":{"detected":true,"capabilities":row["capabilities"]}}))?;
        }
    }
    Ok(())
}
pub fn from_row(row: &Value) -> Result<ModelConfig> {
    let provider = row["provider"].as_str().context("Model provider missing")?;
    let model = ModelConfig {
        default: row["id"].as_str().context("Model ID missing")?.into(),
        name: row["name"].as_str().context("Model name missing")?.into(),
        provider: provider.into(),
        endpoint: row["endpoint"].as_str().unwrap_or("").into(),
        api_key_env: row
            .pointer("/metadata/api_key_env")
            .and_then(Value::as_str)
            .unwrap_or(match provider {
                "ollama" => "OLLAMA_API_KEY",
                "grok" => "XAI_API_KEY",
                _ => "OPENAI_API_KEY",
            })
            .into(),
        keep_alive: row
            .pointer("/metadata/keep_alive")
            .and_then(Value::as_str)
            .unwrap_or("30m")
            .into(),
        context_limit: row["context_limit"].as_u64().unwrap_or(16384) as usize,
    };
    validate(&model)?;
    Ok(model)
}
pub fn resolve(store: &Store, id: &str, default: &ModelConfig) -> Result<ModelConfig> {
    if let Some(model) = crate::cli_agent::resolve_vendor(id) {
        validate(&model)?;
        return Ok(model);
    }
    if id.starts_with("local:gguf:") {
        // Resolved by stable id only. The context limit is the window the
        // server will be started with, so engine and server agree.
        let entry = crate::local_engine::known(id).with_context(|| {
            format!("Local model {id} is not in this computer's catalog. Open the model picker or Settings › Local models to refresh it.")
        })?;
        ensure!(
            entry.availability != "unavailable",
            "{} cannot run on this computer: {}",
            entry.name,
            entry.reason
        );
        let model = ModelConfig {
            default: id.into(),
            name: entry.name,
            provider: "llamacpp".into(),
            endpoint: String::new(),
            api_key_env: "UNUSED".into(),
            keep_alive: "30m".into(),
            context_limit: entry.context_tokens.max(1024) as usize,
        };
        validate(&model)?;
        return Ok(model);
    }
    ensure!(
        !id.starts_with("llamacpp:"),
        "'{id}' is an old local model name. Pick the model again in the composer."
    );
    if crate::cli_agent::is_cli_provider(&default.provider)
        && (id == default.default || id == default.name)
    {
        validate(default)?;
        return Ok(default.clone());
    }
    if id == default.default {
        validate(default)?;
        return Ok(default.clone());
    }
    let rows = store.models()?;
    // Exact registry IDs take precedence over another model's friendly name.
    if let Some(row) = rows.iter().find(|row| row["id"] == id) {
        return from_row(row);
    }
    let mut candidates = Vec::new();
    if id == default.name {
        candidates.push(default.clone());
    }
    for row in rows.iter().filter(|row| row["name"] == id) {
        let model = from_row(row)?;
        if !candidates.iter().any(|existing| {
            same_target(existing, &model) && existing.api_key_env == model.api_key_env
        }) {
            candidates.push(model);
        }
    }
    ensure!(candidates.len() <= 1, "Model name '{id}' is ambiguous; choose its provider in the model picker or use its registry ID");
    candidates
        .pop()
        .context("Model is not registered; add its provider and endpoint first")
}
pub fn catalog(store: &Store, default: &ModelConfig) -> Result<Vec<Value>> {
    let rows = store.models()?;
    let configured: HashSet<_> = rows
        .iter()
        .filter(|row| {
            row["id"] == default.default
                || row
                    .pointer("/metadata/api_key_env")
                    .and_then(Value::as_str)
                    .is_some()
        })
        .map(|row| {
            model_id(
                row["provider"].as_str().unwrap_or(""),
                row["endpoint"].as_str().unwrap_or(""),
                row["name"].as_str().unwrap_or(""),
            )
        })
        .collect();
    let mut seen = HashSet::new();
    let mut configured_seen = HashSet::new();
    // Keep old IDs as working aliases in SQLite, but don't display both an old
    // discovered alias and its new provider-scoped ID in the picker.
    let mut ordered = rows;
    ordered.sort_by_key(|row| {
        (
            row["id"] != default.default,
            !row["id"].as_str().unwrap_or("").starts_with("model:"),
        )
    });
    let mut result = Vec::new();
    for row in ordered {
        let key = model_id(
            row["provider"].as_str().unwrap_or(""),
            row["endpoint"].as_str().unwrap_or(""),
            row["name"].as_str().unwrap_or(""),
        );
        let custom = row["id"] == default.default
            || row
                .pointer("/metadata/api_key_env")
                .and_then(Value::as_str)
                .is_some();
        if custom
            && !configured_seen.insert((
                key.clone(),
                row["metadata"]["api_key_env"]
                    .as_str()
                    .unwrap_or("")
                    .to_owned(),
                row["context_limit"].as_u64(),
            ))
        {
            continue;
        }
        if !custom && (configured.contains(&key) || !seen.insert(key)) {
            continue;
        }
        result.push(row);
    }
    result.sort_by_key(|row| {
        (
            row["name"].as_str().unwrap_or("").to_owned(),
            row["provider"].as_str().unwrap_or("").to_owned(),
        )
    });
    Ok(result)
}
