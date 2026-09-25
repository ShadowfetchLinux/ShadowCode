//! Prompt-cache breakpoints for providers that only cache when asked.
//!
//! OpenAI-style providers (OpenAI, DeepSeek, Grok, most OpenRouter models)
//! cache repeated prompt prefixes on their own; ShadowCode only records the
//! `cached_tokens` they report. Anthropic (Claude) and Google (Gemini) models
//! reached through OpenRouter cache only the parts marked with
//! `cache_control`, so the request is marked here:
//!
//! - Claude: the system prompt, plus a rolling breakpoint on the newest
//!   message and one on the end of the previous request, so each agent step
//!   reads the prefix the step before it wrote. Claude caches tools, then the
//!   system prompt, then messages, so the system breakpoint also covers the
//!   tool schemas. At most 4 breakpoints are allowed; this uses 3.
//! - Gemini: OpenRouter uses only one breakpoint, so it goes on the system
//!   prompt (and with it the tool schemas), which stays the same every step.
//!
//! Prompts shorter than the provider's minimum (about 1,024 tokens for Claude)
//! are simply not cached; the markers do no harm.
use crate::config::ModelConfig;
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// The provider caches automatically (or not at all); send nothing.
    Automatic,
    Anthropic,
    Gemini,
}

fn via_openrouter(config: &ModelConfig) -> bool {
    config.provider == crate::openrouter::PROVIDER
        || reqwest::Url::parse(&config.endpoint)
            .ok()
            .and_then(|url| {
                url.host_str()
                    .map(|h| h.eq_ignore_ascii_case("openrouter.ai"))
            })
            .unwrap_or(false)
}

pub fn style(config: &ModelConfig) -> Style {
    if !via_openrouter(config) {
        return Style::Automatic;
    }
    let name = config.name.to_ascii_lowercase();
    if name.starts_with("anthropic/") || name.contains("claude") {
        Style::Anthropic
    } else if name.starts_with("google/gemini") {
        Style::Gemini
    } else {
        Style::Automatic
    }
}

/// Mark the last text part of `message` (turning plain string content into
/// one text part). Returns false when there is no text to mark.
fn mark(message: &mut Value) -> bool {
    let breakpoint = json!({"type": "ephemeral"});
    let content = &mut message["content"];
    match content {
        Value::String(text) if !text.is_empty() => {
            let text = std::mem::take(text);
            *content = json!([{"type": "text", "text": text, "cache_control": breakpoint}]);
            true
        }
        Value::Array(parts) => match parts.iter_mut().rev().find(|p| p["type"] == "text") {
            Some(part) => {
                part["cache_control"] = breakpoint;
                true
            }
            None => false,
        },
        _ => false,
    }
}

/// A message that can carry a breakpoint (assistant turns are left alone).
fn markable(message: &Value) -> bool {
    matches!(message["role"].as_str(), Some("system" | "user" | "tool"))
}

/// Add `cache_control` breakpoints to provider-bound messages. Returns how
/// many were placed.
pub fn apply(config: &ModelConfig, messages: &mut [Value]) -> usize {
    let style = style(config);
    if style == Style::Automatic {
        return 0;
    }
    let mut placed = 0;
    if let Some(system) = messages.iter_mut().find(|m| m["role"] == "system") {
        placed += usize::from(mark(system));
    }
    if style == Style::Gemini {
        return placed;
    }
    // Rolling breakpoint: the newest markable message.
    let newest = messages.iter().rposition(markable).filter(|&i| i > 0);
    // The previous request ended just before the newest assistant reply.
    let previous = messages
        .iter()
        .rposition(|m| m["role"] == "assistant")
        .and_then(|a| messages[..a].iter().rposition(markable))
        .filter(|&i| i > 0 && Some(i) != newest);
    for index in [previous, newest].into_iter().flatten() {
        placed += usize::from(mark(&mut messages[index]));
    }
    placed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(provider: &str, name: &str, endpoint: &str) -> ModelConfig {
        ModelConfig {
            default: String::new(),
            name: name.into(),
            provider: provider.into(),
            endpoint: endpoint.into(),
            api_key_env: String::new(),
            keep_alive: "5m".into(),
            context_limit: 200_000,
        }
    }

    fn history() -> Vec<Value> {
        vec![
            json!({"role":"system","content":"You are ShadowCode."}),
            json!({"role":"user","content":"Fix the bug"}),
            json!({"role":"assistant","content":"","tool_calls":[{"id":"c1","type":"function","function":{"name":"read_file","arguments":"{}"}}]}),
            json!({"role":"tool","tool_call_id":"c1","name":"read_file","content":"fn main() {}"}),
        ]
    }

    fn marked(messages: &[Value]) -> Vec<usize> {
        messages
            .iter()
            .enumerate()
            .filter(|(_, m)| m.to_string().contains("cache_control"))
            .map(|(i, _)| i)
            .collect()
    }

    #[test]
    fn claude_via_openrouter_gets_system_and_rolling_breakpoints() {
        let claude = model("openrouter", "anthropic/claude-sonnet-4.5", "");
        assert_eq!(style(&claude), Style::Anthropic);
        let mut messages = history();
        assert_eq!(apply(&claude, &mut messages), 3);
        assert_eq!(marked(&messages), [0, 1, 3]);
        assert_eq!(messages[3]["content"][0]["text"], "fn main() {}");
        assert_eq!(
            messages[3]["content"][0]["cache_control"],
            json!({"type":"ephemeral"})
        );
        assert!(messages[2]["content"].is_string(), "assistant untouched");
        // First request: only the system prompt and the user message.
        let mut first = history()[..2].to_vec();
        assert_eq!(apply(&claude, &mut first), 2);
        // Image messages keep their image and mark the text part.
        let mut images = vec![
            json!({"role":"system","content":"s"}),
            json!({"role":"user","content":[{"type":"text","text":"look"},{"type":"image_url","image_url":{"url":"data:"}}]}),
        ];
        apply(&claude, &mut images);
        assert!(images[1]["content"][0]["cache_control"].is_object());
        assert!(images[1]["content"][1].get("cache_control").is_none());
    }

    #[test]
    fn gemini_gets_one_breakpoint_and_others_get_none() {
        let gemini = model("openrouter", "google/gemini-2.5-pro", "");
        let mut messages = history();
        assert_eq!(apply(&gemini, &mut messages), 1);
        assert_eq!(marked(&messages), [0]);
        for other in [
            model("openrouter", "openai/gpt-5", ""),
            model("openrouter", "qwen/qwen3-coder", ""),
            model(
                "openai_compatible",
                "claude-sonnet",
                "https://api.example.com/v1",
            ),
            model("llamacpp", "claude-distill", "http://127.0.0.1:8080/v1"),
        ] {
            let mut messages = history();
            assert_eq!(apply(&other, &mut messages), 0, "{}", other.name);
            assert_eq!(messages, history());
        }
        let custom = model(
            "openai_compatible",
            "anthropic/claude-opus-4.1",
            "https://openrouter.ai/api/v1",
        );
        assert_eq!(style(&custom), Style::Anthropic);
    }
}
