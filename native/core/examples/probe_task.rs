//! Real local-model coding probe. Uses a disposable project and isolated profile.
use anyhow::{ensure, Result};
use serde_json::json;
use shadowcode_core::{
    checkpoint,
    config::Config,
    engine::{Engine, StartRequest},
    paths::AppPaths,
    process::{self, ProcessSpec},
    workspace::Workspace,
};
use std::{fs, time::Duration};
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<()> {
    let model = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "gpt-oss:20b".into());
    let root = tempfile::tempdir()?;
    let project = root.path().join("project");
    fs::create_dir_all(project.join("src"))?;
    fs::write(
        project.join("Cargo.toml"),
        "[package]\nname = \"native-probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    let original="pub fn add(a: i32, b: i32) -> i32 { a - b }\n\n#[cfg(test)]\nmod tests {\n    #[test] fn adds() { assert_eq!(super::add(2, 3), 5); assert_eq!(super::add(-2, 1), -1); }\n}\n";
    fs::write(project.join("src/lib.rs"), original)?;
    let original_manifest = fs::read_to_string(project.join("Cargo.toml"))?;
    let paths = AppPaths::isolated(&root.path().join("profile"))?;
    Config::patch(
        &paths,
        json!({"model":{"provider":"ollama","endpoint":"http://127.0.0.1:11434/v1","name":model,"default":model,"api_key_env":"OLLAMA_API_KEY","context_limit":8192},"agent":{"max_steps":14,"max_task_tokens":100000,"tool_timeout_sec":60}}),
    )?;
    let engine = Engine::open(paths)?;
    let mut events = engine.subscribe();
    let observer = tokio::spawn(async move {
        while let Ok(event) = events.recv().await {
            match event["type"].as_str().unwrap_or("") {
                "tool.started" => println!(
                    "tool: {} {}",
                    event["payload"]["tool"], event["payload"]["arguments"]
                ),
                "tool.completed" => println!(
                    "result: {} success={} {}",
                    event["payload"]["tool"],
                    event["payload"]["success"],
                    event["payload"]["error"]
                ),
                "context.compacted" => println!("context compacted"),
                "agent.completed" => println!("completed: {}", event["payload"]["summary"]),
                _ => {}
            }
        }
    });
    let controller = engine.clone();
    let approved_workspace = project.clone();
    let approvals = tokio::spawn(async move {
        loop {
            for approval in controller.approvals().list(None) {
                let allow = approval.tool == "exec"
                    && approval.arguments["command"] == "cargo test --offline --lib"
                    && (matches!(approval.arguments["cwd"].as_str(), None | Some("" | "."))
                        || approval.arguments["cwd"].as_str() == approved_workspace.to_str());
                println!("approval: {} allow={allow}", approval.command);
                let _ = controller
                    .approvals()
                    .decide(&approval.id, &approval.session_id, allow);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    });
    let request=StartRequest{workspace:project.clone(),task:"Fix the add function in src/lib.rs so it adds its two inputs. Inspect the file using tools, make the smallest change, then run exactly `cargo test --offline --lib` using exec with no cwd parameter. Preserve the existing tests and package files. Finish only after seeing the test result.".into(),session_id:None,model:None,mode:"code".into(),queue:false
            images: Vec::new(),
        };
    let first = engine.start(request).await?;
    let completed =
        tokio::time::timeout(Duration::from_secs(240), engine.wait(&first.id)).await??;
    ensure!(
        completed.status == "completed",
        "Native coding task failed: {}",
        completed.summary
    );
    let updated = fs::read_to_string(project.join("src/lib.rs"))?;
    ensure!(
        updated == original.replace("a - b", "a + b"),
        "Expected the minimal addition fix; received: {updated}"
    );
    ensure!(
        fs::read_to_string(project.join("Cargo.toml"))? == original_manifest,
        "Model changed package metadata instead of preserving the fixture"
    );
    let evidence = engine.store().recent_events(&first.session_id, 500)?;
    ensure!(
        evidence.iter().any(|e| e["type"] == "tool.completed"
            && e["payload"]["tool"] == "exec"
            && e["payload"]["success"] == true),
        "Model did not complete verification"
    );
    // Independently rerun the unchanged assertions, outside the model loop.
    let verified = process::run(
        ProcessSpec::command("cargo", &["test", "--offline", "--lib"], project.clone()),
        CancellationToken::new(),
        None,
    )
    .await?;
    ensure!(
        verified.ok,
        "Independent verification failed: {}",
        verified.stderr
    );
    let second=engine.start(StartRequest{workspace:project.clone(),task:"Read src/lib.rs again and tell me what add(-2, 1) now returns. Do not change files or run commands.".into(),session_id:Some(first.session_id.clone()),model:None,mode:"review".into(),queue:false
            images: Vec::new(),
        }).await?;
    let continued =
        tokio::time::timeout(Duration::from_secs(180), engine.wait(&second.id)).await??;
    ensure!(
        continued.status == "completed" && continued.summary.contains("-1"),
        "Continuation failed: {}",
        continued.summary
    );
    ensure!(
        engine
            .store()
            .recent_events(&first.session_id, 500)?
            .iter()
            .any(|e| e["task_id"] == second.task_id
                && e["type"] == "tool.completed"
                && e["payload"]["tool"] == "read_file"
                && e["payload"]["success"] == true),
        "Continuation did not inspect current files"
    );
    engine.shutdown().await?;
    approvals.abort();
    observer.abort();
    checkpoint::restore(&engine.store(), &Workspace::open(&project)?, &first.task_id)?;
    ensure!(
        fs::read_to_string(project.join("src/lib.rs"))? == original,
        "Checkpoint failed to restore the original file"
    );
    println!("Native coding probe passed for {model}: real file edit, scoped command approval, tests, continuation, read-only mode, and rewind. Usage: {} tokens; {} model steps.",completed.usage.total_tokens+continued.usage.total_tokens,completed.steps+continued.steps);
    Ok(())
}
