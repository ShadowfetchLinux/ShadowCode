//! One minimal real turn through a signed-in subscription CLI, driven exactly
//! like the desktop drives it (Service -> Engine -> runtime -> adapter).
//!
//! Uses an isolated profile and a scratch project; the vendor CLI keeps its
//! own login. The prompt asks for a one-word answer and no tools, so the turn
//! uses a negligible part of the plan allowance.
//!
//! Usage: cargo run --example live_vendor_turn -- cli:grok:grok-4.7 [--second]
//! `--second` sends a follow-up to check native session resume.
use serde_json::{json, Value};
use shadowcode_core::{
    config::Config,
    paths::AppPaths,
    service::{Request, Service},
};
use std::time::Duration;

async fn call(service: &Service, method: &str, path: &str, body: Value) -> anyhow::Result<Value> {
    service
        .dispatch(Request {
            method: method.into(),
            path: path.into(),
            body,
        })
        .await
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let target = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: live_vendor_turn <picker id> [--second]"))?;
    let second = std::env::args().any(|a| a == "--second");
    let root = tempfile::tempdir()?;
    let project = root.path().join("project");
    std::fs::create_dir(&project)?;
    std::fs::write(project.join("README.md"), "# scratch\n")?;
    let paths = AppPaths::isolated(&root.path().join("profile"))?;
    Config::patch(&paths, json!({"trusted_workspaces":[project]}))?;
    let service = Service::open(paths, Some(project.clone()))?;
    let picker = call(&service, "GET", "/api/picker", json!({})).await?;
    let row = picker["targets"]
        .as_array()
        .and_then(|rows| rows.iter().find(|r| r["id"] == target.as_str()).cloned())
        .ok_or_else(|| anyhow::anyhow!("{target} is not a picker row"))?;
    println!(
        "row: {} | {} | {} | usage: {}",
        row["name"], row["availability_label"], row["subtitle"], row["usage"]["label"]
    );
    if std::env::args().any(|a| a == "--picker-only") {
        let vendor = row["provider"]
            .as_str()
            .unwrap_or("")
            .trim_start_matches("cli:")
            .to_owned();
        println!("usage json: {}", row["usage"]);
        println!("vendor status: {}", picker["vendors"][&vendor]);
        return Ok(());
    }
    let prompts: &[&str] = if second {
        &[
            "Reply with exactly the word ALPHA and nothing else. Do not use any tools.",
            "What single word did you reply with last time? Reply with just that word. Do not use any tools.",
        ]
    } else {
        &["Reply with exactly the word ALPHA and nothing else. Do not use any tools."]
    };
    let mut session: Option<String> = None;
    for prompt in prompts {
        let mut body = json!({"task": prompt, "model": target, "workspace": project});
        if let Some(sid) = &session {
            body["session_id"] = json!(sid);
        }
        let started = std::time::Instant::now();
        let job = call(&service, "POST", "/api/jobs", body).await?;
        if job["needs_consent"] == true {
            println!("consent requested: {}", job["handoff"]);
            break;
        }
        let id = job["id"].as_str().unwrap_or_default().to_owned();
        session = job["session_id"].as_str().map(str::to_owned);
        let done = tokio::time::timeout(Duration::from_secs(240), service.engine.wait(&id))
            .await
            .map_err(|_| anyhow::anyhow!("turn did not finish in 240 s"))??;
        let done = json!(done);
        println!(
            "status={} in {:.1}s | answer={:?} | error={:?}",
            done["status"],
            started.elapsed().as_secs_f64(),
            done["result"]["summary"]
                .as_str()
                .or(done["result"]["text"].as_str())
                .map(|s| s.chars().take(200).collect::<String>()),
            done["error"].as_str()
        );
        if let Some(sid) = &session {
            let events = service.engine.store().events_after(sid, 0, None, 10_000)?;
            let kinds: Vec<String> = events
                .iter()
                .filter_map(|e| e["type"].as_str().map(str::to_owned))
                .collect();
            println!("events: {}", kinds.join(","));
            for e in &events {
                if e["type"] == "vendor.session"
                    || e["type"] == "limit.reached"
                    || e["type"] == "usage.updated"
                    || e["type"] == "agent.warning"
                {
                    println!("  {} {}", e["type"], e["payload"]);
                }
            }
        }
    }
    // --switch <picker id>: continue the same conversation on another
    // provider; the first attempt must ask for consent, the second hands off.
    let args: Vec<String> = std::env::args().collect();
    if let (Some(pos), Some(sid)) = (args.iter().position(|a| a == "--switch"), &session) {
        let other = args.get(pos + 1).cloned().unwrap_or_default();
        let body = json!({"task":"What single word did you reply with earlier in this conversation? Reply with just that word. Do not use any tools.","model":other,"workspace":project,"session_id":sid});
        let first = call(&service, "POST", "/api/jobs", body.clone()).await?;
        println!(
            "switch without consent: needs_consent={} status={} handoff={}",
            first["needs_consent"], first["status"], first["handoff"]
        );
        let mut consented = body;
        consented["handoff_consent"] = json!(true);
        let job = call(&service, "POST", "/api/jobs", consented).await?;
        let id = job["id"].as_str().unwrap_or_default().to_owned();
        let done =
            json!(tokio::time::timeout(Duration::from_secs(240), service.engine.wait(&id)).await??);
        println!(
            "after consent: status={} answer={:?}",
            done["status"],
            done["result"]["summary"].as_str()
        );
        for e in service.engine.store().events_after(sid, 0, None, 10_000)? {
            if e["type"] == "agent.handoff" {
                println!("  agent.handoff {}", e["payload"]);
            }
        }
    }
    if let Some(sid) = &session {
        let s = call(&service, "GET", &format!("/api/sessions/{sid}"), json!({})).await?;
        println!(
            "execution_target={} native_sessions={}",
            s["execution_target"], s["native_sessions"]
        );
    }
    service.engine.shutdown().await.ok();
    Ok(())
}
