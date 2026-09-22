//! Live Ollama regression for ordinary hardware questions, with an isolated profile.
//! All approval-requiring operations are denied; the hardware probe is read-only.
use anyhow::{ensure, Result};
use serde_json::json;
use shadowcode_core::{
    config::Config,
    engine::{Engine, StartRequest},
    paths::AppPaths,
};
use std::{fs, time::Duration};

#[tokio::main]
async fn main() -> Result<()> {
    let model = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "qwen3:14b".into());
    let root = tempfile::tempdir()?;
    let project = root.path().join("project");
    fs::create_dir(&project)?;
    let paths = AppPaths::isolated(&root.path().join("profile"))?;
    Config::patch(
        &paths,
        json!({
            "model":{"provider":"ollama","endpoint":"http://127.0.0.1:11434/v1",
                "name":model,"default":model,"context_limit":16384},
            "permissions":{"approve_shell":true},
            "agent":{"max_steps":6,"max_task_tokens":100000}
        }),
    )?;
    let engine = Engine::open(paths)?;
    let controller = engine.clone();
    let approvals = tokio::spawn(async move {
        loop {
            for approval in controller.approvals().list(None) {
                let _ = controller
                    .approvals()
                    .decide(&approval.id, &approval.session_id, false);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    });
    let result: Result<()> = async {
        let mut session_id = None;
        for prompt in ["hi", "How many screens do I have on my computer right now"] {
            let job = engine
                .start(StartRequest {
                    workspace: project.clone(),
                    task: prompt.into(),
                    session_id,
                    model: None,
                    mode: "code".into(),
                    queue: false,
                    images: vec![],
                })
                .await?;
            let done =
                tokio::time::timeout(Duration::from_secs(180), engine.wait(&job.id)).await??;
            println!("{prompt}\n{}: {}", done.status, done.summary);
            ensure!(done.status == "completed", "Task failed");
            let events = engine.store().recent_events(&job.session_id, 300)?;
            let tools: Vec<_> = events
                .iter()
                .filter(|event| {
                    event["task_id"] == job.task_id && event["type"] == "tool.completed"
                })
                .collect();
            for event in &tools {
                println!("evidence: {}", event["payload"]);
            }
            if prompt == "hi" {
                ensure!(tools.is_empty(), "A greeting should not inspect the host");
            } else {
                ensure!(
                    tools
                        .iter()
                        .any(|event| event["payload"]["tool"] == "system_info"
                            && event["payload"]["success"] == true),
                    "No native hardware inspection occurred"
                );
            }
            session_id = Some(job.session_id);
        }
        Ok(())
    }
    .await;
    engine.shutdown().await?;
    approvals.abort();
    result
}
