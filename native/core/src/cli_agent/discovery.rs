//! Discover vendor models from official CLI interfaces only.
//! Output is parsed as a list of names; no pretend catalog is added.
use super::{
    clip, doctor, picker, redact, resolve_binary, usage::UsageSnapshot, CliAgentsConfig, Vendor,
};
use anyhow::Result;
use serde_json::{json, Value};
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Debug, PartialEq)]
pub struct DiscoveredModel {
    pub id: String,
    pub label: String,
    pub auto: bool,
}

pub fn parse_cursor_models(text: &str) -> Vec<DiscoveredModel> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.to_ascii_lowercase().starts_with("available models") {
            continue;
        }
        let (id, label) = match line.split_once(" - ") {
            Some((id, label)) => (id.trim(), label.trim()),
            None => continue,
        };
        if id.is_empty() || !seen.insert(id.to_owned()) {
            continue;
        }
        out.push(DiscoveredModel {
            id: id.to_owned(),
            label: if label.is_empty() { id.to_owned() } else { label.to_owned() },
            auto: id == "auto",
        });
    }
    out
}

pub fn parse_agy_models(text: &str) -> Vec<DiscoveredModel> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.to_ascii_lowercase().starts_with("fetching") {
            continue;
        }
        let (id, label) = match line.split_once('\t') {
            Some((id, label)) => (id.trim(), label.trim()),
            None => match line.split_once("  ") {
                Some((id, label)) => (id.trim(), label.trim()),
                None => continue,
            },
        };
        if id.is_empty() || id.contains(' ') || !seen.insert(id.to_owned()) {
            continue;
        }
        out.push(DiscoveredModel {
            id: id.to_owned(),
            label: if label.is_empty() { id.to_owned() } else { label.to_owned() },
            auto: false,
        });
    }
    out
}

pub fn parse_codex_models(text: &str) -> Vec<DiscoveredModel> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for raw in text.lines() {
        let line = raw.trim().trim_start_matches(['-', '*', '•']);
        let line = line.trim();
        if line.is_empty() || line.contains(' ') && !line.contains("gpt") && !line.contains("o") {
            if let Some(id) = line.split_whitespace().next() {
                if id.starts_with("gpt") || id.starts_with("o") || id == "default" {
                    if seen.insert(id.to_owned()) {
                        out.push(DiscoveredModel {
                            id: id.to_owned(),
                            label: id.to_owned(),
                            auto: id == "default",
                        });
                    }
                }
            }
            continue;
        }
        if (line.starts_with("gpt") || line.starts_with('o') || line == "default")
            && seen.insert(line.to_owned())
        {
            out.push(DiscoveredModel {
                id: line.to_owned(),
                label: line.to_owned(),
                auto: line == "default",
            });
        }
    }
    out
}

fn resolve_with_path(configured: &str, path_env: Option<&OsStr>) -> Option<PathBuf> {
    match path_env {
        None => resolve_binary(configured),
        Some(path) => {
            let candidate = Path::new(configured);
            if candidate.components().count() > 1 {
                return candidate.is_file().then(|| candidate.to_path_buf());
            }
            std::env::split_paths(path)
                .map(|dir| dir.join(configured))
                .find(|p| p.is_file())
        }
    }
}

async fn run_list(binary: &Path, args: &[&str], path_env: Option<&OsStr>) -> Option<String> {
    let mut command = tokio::process::Command::new(binary);
    command
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .env("NO_COLOR", "1")
        .env("TERM", "dumb");
    if let Some(path) = path_env {
        command.env("PATH", path);
    }
    let output = tokio::time::timeout(Duration::from_secs(12), command.output())
        .await
        .ok()?
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    if text.trim().is_empty() {
        text = String::from_utf8_lossy(&output.stderr).into_owned();
    }
    Some(clip(&redact(&text), 8000).to_owned())
}

pub async fn models_for(
    vendor: Vendor,
    config: &CliAgentsConfig,
    path_env: Option<&OsStr>,
) -> Vec<DiscoveredModel> {
    let Some(binary) = resolve_with_path(config.binary(vendor), path_env) else {
        return Vec::new();
    };
    let (args, parse): (&[&str], fn(&str) -> Vec<DiscoveredModel>) = match vendor {
        Vendor::Cursor => (&["--list-models"], parse_cursor_models),
        Vendor::Antigravity => (&["models"], parse_agy_models),
        Vendor::Codex => (&["models"], parse_codex_models),
        Vendor::Claude | Vendor::Grok => return Vec::new(),
    };
    match run_list(&binary, args, path_env).await {
        Some(text) => parse(&text),
        None => Vec::new(),
    }
}

/// Build picker rows from Doctor + official model lists. Missing lists fall
/// back to a single Auto/Default row; they never invent a catalog.
pub async fn picker_targets(config: &CliAgentsConfig) -> Result<Value> {
    let doctor = doctor::status(config).await;
    let mut rows = picker::doctor_rows(config, &doctor);
    for vendor in Vendor::FEATURED {
        if !config.vendor_enabled(vendor) {
            continue;
        }
        let state = doctor
            .get(vendor.id())
            .and_then(|v| v["state"].as_str())
            .unwrap_or("");
        if state != "ready" {
            continue;
        }
        let discovered = models_for(vendor, config, None).await;
        let usage = UsageSnapshot::unavailable(vendor.product_label());
        for model in discovered {
            rows.push(picker::vendor_target(
                vendor,
                &model.id,
                &model.label,
                picker::Availability::Ready,
                doctor
                    .get(vendor.id())
                    .and_then(|v| v["detail"].as_str())
                    .unwrap_or("Ready"),
                usage.clone(),
                vendor.accepts_images(),
            ));
        }
    }
    let rows = picker::merge_discovered(Vec::new(), rows);
    Ok(json!({
        "targets": rows.iter().map(picker::PickerTarget::to_json).collect::<Vec<_>>(),
        "vendors": doctor,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cursor_and_agy_lists_without_inventing() {
        let cursor = parse_cursor_models(
            "Available models\n\nauto - Auto (current, default)\ngpt-5.3-codex - Codex 5.3\n",
        );
        assert_eq!(cursor.len(), 2);
        assert!(cursor[0].auto);
        assert_eq!(cursor[1].id, "gpt-5.3-codex");
        let agy = parse_agy_models(
            "Fetching available models...\ngemini-3.8-flash-high\tGemini 3.8 Flash (High)\n",
        );
        assert_eq!(agy.len(), 1);
        assert_eq!(agy[0].id, "gemini-3.8-flash-high");
        assert!(parse_cursor_models("not a list").is_empty());
    }
}
