//! Read-only probe of the installed Codex app-server: login state, plan,
//! rate-limit windows, and the model catalog. Starts no thread or turn.
//! Usage: cargo run -p shadowcode-core --example probe_codex [-- /path/to/codex]
use std::{path::PathBuf, time::Duration};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let binary = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .or_else(|| shadowcode_core::cli_agent::resolve_binary("codex"))
        .ok_or_else(|| anyhow::anyhow!("codex is not on PATH"))?;
    let probe = shadowcode_core::cli_agent::codex_probe::probe(&binary, None, Duration::from_secs(30)).await?;
    println!("auth_mode={} logged_in={} subscription={} plan={:?}", probe.auth_mode(), probe.logged_in(), probe.subscription_login(), probe.plan_type());
    if let Some(limits) = &probe.rate_limits {
        let snap = &limits["rateLimits"];
        println!(
            "primary: used {}% window {:?} min resets_at {:?} | credits {:?}",
            snap["primary"]["usedPercent"], snap["primary"]["windowDurationMins"], snap["primary"]["resetsAt"], snap["credits"]
        );
        if let Some(pools) = limits["rateLimitsByLimitId"].as_object() {
            for (id, pool) in pools {
                println!("pool {id}: name={:?} slug={:?} used={}%", pool["limitName"], pool["normalModelSlug"], pool["primary"]["usedPercent"]);
            }
        }
    }
    for model in shadowcode_core::cli_agent::codex_probe::models_from_list(&probe.models) {
        println!("model {} ({}) default={} vision={}", model.id, model.label, model.is_default, model.vision);
    }
    for (method, error) in &probe.errors {
        println!("error {method}: {error}");
    }
    Ok(())
}
