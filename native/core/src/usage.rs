//! Token and cost accounting per model turn, per job and per session.
//!
//! Each model response becomes one "turn" [`Usage`]: tokens as the provider
//! reported them (or estimated, marked `estimated`), cached input tokens, and
//! a cost in US dollars. The cost comes from, in order:
//! 1. the provider (OpenRouter's `usage.cost`), or the vendor CLI (Claude's
//!    `total_cost_usd`);
//! 2. zero for a model running on this computer;
//! 3. OpenRouter's published per-token prices from the cached model list
//!    (marked `cost_estimated`);
//! 4. otherwise unknown (`cost_usd: null`).
//!
//! Turns add up into the job's `usage`, finished jobs add up into the
//! session's `usage`, and every turn emits a `usage.updated` event.
use crate::{config::ModelConfig, models::Usage, store::Store};
use anyhow::Result;
use serde_json::{json, Value};
use std::path::Path;

/// Set who reported a model turn and what it cost. `state_dir` holds the
/// cached OpenRouter price list.
pub fn price_turn(usage: &mut Usage, model: &ModelConfig, state_dir: &Path) {
    usage.turns = usage.turns.max(1);
    // OpenRouter is always a paid cloud route (its endpoint is only loopback
    // in tests).
    let local = model.provider != crate::openrouter::PROVIDER
        && (model.provider == "mock" || crate::config::runs_on_this_computer(model));
    if usage.source.is_empty() {
        usage.source = if local { "local" } else { "provider" }.into();
    }
    if usage.cost_usd.is_some() {
        return;
    }
    if local {
        usage.cost_usd = Some(0.0);
        return;
    }
    if model.provider == crate::openrouter::PROVIDER {
        if let Some(listed) = crate::openrouter::model_in(state_dir, &model.name) {
            if let (Some(input), Some(output)) = (listed.prompt_price, listed.completion_price) {
                // Cached input is billed at the full input price here, so an
                // estimate is an upper bound.
                usage.cost_usd = Some(
                    input * usage.prompt_tokens as f64 + output * usage.completion_tokens as f64,
                );
                usage.cost_estimated = true;
            }
        }
    }
}

/// Read a stored usage object leniently (older rows only have token counts).
pub fn parse(value: &Value) -> Usage {
    let value = match value {
        Value::String(text) => serde_json::from_str(text).unwrap_or(Value::Null),
        other => other.clone(),
    };
    serde_json::from_value(value).unwrap_or_default()
}

/// The session's total including the running job: finished tasks as stored,
/// minus any earlier total for this task, plus the job's current usage.
pub fn session_total(store: &Store, session_id: &str, task_id: &str, job: &Usage) -> Result<Usage> {
    let mut total = store
        .session(session_id)?
        .map(|s| parse(&s["usage_json"]))
        .unwrap_or_default();
    if let Some(task) = store.task(task_id)? {
        total.subtract(&parse(&task["usage_json"]));
    }
    total.add(job);
    Ok(total)
}

/// Payload for the `usage.updated` event.
pub fn event(turn: &Usage, job: &Usage, session: &Usage, purpose: &str) -> Value {
    json!({"purpose": purpose, "turn": turn, "job": job, "session": session})
}

/// Short text for `/cost`: tokens, cached tokens and cost.
pub fn describe(usage: &Usage) -> String {
    let cost = match usage.cost_usd {
        Some(cost) if usage.cost_estimated => format!("about ${cost:.4} (estimated)"),
        Some(cost) => format!("${cost:.4}"),
        None => "unknown (the provider did not report a price)".into(),
    };
    format!(
        "{} input tokens ({} cached), {} output tokens{}; cost {cost}",
        usage.prompt_tokens,
        usage.cached_tokens,
        usage.completion_tokens,
        if usage.estimated {
            " (some counts estimated)"
        } else {
            ""
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(provider: &str, endpoint: &str) -> ModelConfig {
        ModelConfig {
            default: String::new(),
            name: "acme/coder".into(),
            provider: provider.into(),
            endpoint: endpoint.into(),
            api_key_env: String::new(),
            keep_alive: "5m".into(),
            context_limit: 32_768,
        }
    }

    fn turn(prompt: u64, completion: u64) -> Usage {
        Usage {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: prompt + completion,
            ..Default::default()
        }
    }

    #[test]
    fn local_turns_are_free_and_unknown_prices_stay_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let mut local = turn(100, 10);
        price_turn(
            &mut local,
            &model("llamacpp", "http://127.0.0.1:8080/v1"),
            dir.path(),
        );
        assert_eq!(
            (local.cost_usd, local.source.as_str(), local.turns),
            (Some(0.0), "local", 1)
        );
        let mut hosted = turn(100, 10);
        price_turn(
            &mut hosted,
            &model("openai_compatible", "https://api.example.com/v1"),
            dir.path(),
        );
        assert_eq!(
            (hosted.cost_usd, hosted.source.as_str()),
            (None, "provider")
        );
        let mut reported = turn(100, 10);
        reported.cost_usd = Some(0.25);
        price_turn(&mut reported, &model("openrouter", ""), dir.path());
        assert_eq!(reported.cost_usd, Some(0.25));
        assert!(!reported.cost_estimated);
    }

    #[test]
    fn totals_mark_mixed_sources_and_partial_costs() {
        let mut job = Usage::default();
        let mut first = turn(100, 10);
        first.cost_usd = Some(0.5);
        first.source = "provider".into();
        first.turns = 1;
        first.cached_tokens = 60;
        job.add(&first);
        assert_eq!(job, first);
        let mut second = turn(50, 5);
        second.source = "provider".into();
        second.turns = 1;
        job.add(&second);
        assert_eq!(job.cost_usd, Some(0.5));
        assert!(job.cost_estimated, "one turn had no cost");
        assert_eq!(
            (job.prompt_tokens, job.cached_tokens, job.turns),
            (150, 60, 2)
        );
        let mut vendor = turn(1, 1);
        vendor.source = "vendor".into();
        vendor.turns = 1;
        vendor.cost_usd = Some(0.25);
        job.add(&vendor);
        assert_eq!(job.source, "mixed");
        assert_eq!(job.cost_usd, Some(0.75));
        let mut again = job.clone();
        again.subtract(&vendor);
        assert_eq!(again.cost_usd, Some(0.5));
        assert_eq!(again.turns, 2);
        // Older stored rows only have token counts.
        let old = parse(&json!("{\"prompt_tokens\":7,\"total_tokens\":9}"));
        assert_eq!(
            (old.prompt_tokens, old.total_tokens, old.cost_usd),
            (7, 9, None)
        );
        assert!(describe(&job).contains("cost about $0.7500 (estimated)"));
    }
}
