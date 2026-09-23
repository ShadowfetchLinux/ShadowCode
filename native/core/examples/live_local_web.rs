//! Live check: a GGUF model from the Ollama store, run by the managed
//! llama.cpp runtime, uses the web tools in a real native agent task.
//!
//! Requires SHADOWCODE_LLAMA_SERVER (or an installed runtime) and an Ollama
//! store. Imports by reference into an isolated profile; nothing is copied.
//! Usage: cargo run --example live_local_web -- qwen3:14b [url]
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
    let tag = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "qwen3:14b".into());
    let url = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "https://example.com".into());
    let root = tempfile::tempdir()?;
    let project = root.path().join("project");
    std::fs::create_dir(&project)?;
    let paths = AppPaths::isolated(&root.path().join("profile"))?;
    Config::patch(
        &paths,
        json!({"trusted_workspaces":[project],"network":{"mode":"online"}}),
    )?;
    let service = Service::open(paths, Some(project.clone()))?;
    let imported = call(
        &service,
        "POST",
        "/api/local-models/import-ollama",
        json!({"tag": tag}),
    )
    .await?;
    let models = imported["local_engine"]["models"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let entry = models
        .iter()
        .find(|m| m["name"].as_str() == Some(tag.as_str()))
        .or(models.first())
        .ok_or_else(|| anyhow::anyhow!("import produced no model"))?;
    let id = entry["id"].as_str().unwrap_or_default().to_owned();
    println!(
        "model {} | compatible={} | tools={} | fits={}",
        entry["name"], entry["compatible"], entry["tools"], entry["fits"]
    );
    let started = std::time::Instant::now();
    let job = call(
        &service,
        "POST",
        "/api/jobs",
        json!({
            "task": format!("Use web_fetch to open {url} and tell me the exact page title. Cite the URL you used."),
            "model": id,
            "workspace": project,
            "web": true,
        }),
    )
    .await?;
    let job_id = job["id"].as_str().unwrap_or_default().to_owned();
    let sid = job["session_id"].as_str().unwrap_or_default().to_owned();
    let done =
        json!(tokio::time::timeout(Duration::from_secs(300), service.engine.wait(&job_id)).await??);
    println!(
        "status={} in {:.1}s\nanswer: {}",
        done["status"],
        started.elapsed().as_secs_f64(),
        done["result"]["summary"].as_str().unwrap_or("")
    );
    for e in service.engine.store().events_after(&sid, 0, None, 10_000)? {
        match e["type"].as_str() {
            Some("tool.started") => println!(
                "  tool.started {} {}",
                e["payload"]["tool"], e["payload"]["arguments"]
            ),
            Some("web.source") => println!("  web.source {}", e["payload"]),
            Some("tool.completed") if e["payload"]["sources"].is_array() => {
                println!("  tool.completed sources={}", e["payload"]["sources"])
            }
            _ => {}
        }
    }
    service.engine.shutdown().await.ok();
    Ok(())
}
