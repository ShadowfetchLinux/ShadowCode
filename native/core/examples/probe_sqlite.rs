//! Real local-model SQLite probe in a disposable project and read-only task.
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
        .unwrap_or_else(|| "gpt-oss:20b".into());
    let output = std::env::args().nth(2);
    let root = tempfile::tempdir()?;
    let project = root.path().join("project");
    fs::create_dir(&project)?;
    let file = project.join("billing.db");
    let db = rusqlite::Connection::open(&file)?;
    db.execute_batch("CREATE TABLE invoices(id INTEGER PRIMARY KEY, total_cents INTEGER, status TEXT); INSERT INTO invoices VALUES(1,413,'paid'),(2,587,'paid'),(3,9900,'open');")?;
    drop(db);
    let original = fs::read(&file)?;
    let paths = AppPaths::isolated(&root.path().join("profile"))?;
    Config::patch(
        &paths,
        json!({"model":{"provider":"ollama","endpoint":"http://127.0.0.1:11434/v1","name":model,"default":model,"context_limit":8192},"trusted_workspaces":[project],"permissions":{"level":"read_only"},"agent":{"max_steps":10,"max_task_tokens":50000,"model_retries":0}}),
    )?;
    let engine = Engine::open(paths)?;
    let job = engine.start(StartRequest {
        workspace:project.clone(),session_id:None,model:None,mode:"review".into(),queue:false,
        task:"Inspect billing.db using the native SQLite table/query tools. Discover its schema, then use SQL SUM to calculate total_cents for paid invoices only. Report the actual integer as total_cents=NUMBER. Do not use a shell or change any data/files.".into()
    
            images: Vec::new(),
        }).await?;
    let finished = tokio::time::timeout(Duration::from_secs(240), engine.wait(&job.id)).await;
    engine.shutdown().await?;
    let finished = finished??;
    let events = engine.store().recent_events(&job.session_id, 200)?;
    let queries: Vec<_> = events
        .iter()
        .filter(|event| {
            event["type"] == "tool.completed"
                && event["payload"]["tool"] == "mcp_sqlite_query"
                && event["payload"]["success"] == true
        })
        .collect();
    let total_verified = queries.iter().any(|event| {
        event["payload"]["output"]["rows"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|row| {
                row.as_object()
                    .is_some_and(|values| values.values().any(|value| value.as_i64() == Some(1000)))
            })
    });
    let unchanged = fs::read(&file)? == original;
    let passed = finished.status == "completed"
        && total_verified
        && unchanged
        && finished.summary.replace(',', "").contains("1000");
    let report = json!({"passed":passed,"model":finished.model,"requested_model":model,"sqlite":rusqlite::version(),"status":finished.status,"summary":finished.summary,"sql_queries":queries.len(),"verified_total_cents":if total_verified{Some(1000)}else{None},"database_unchanged":unchanged,"steps":finished.steps,"usage":finished.usage,"tools":events.iter().filter(|e|matches!(e["type"].as_str(),Some("tool.started"|"tool.completed"))).collect::<Vec<_>>(),"verification":events.iter().filter(|e|e["type"].as_str().is_some_and(|kind|kind.starts_with("verification."))).collect::<Vec<_>>()});
    if let Some(output) = output {
        fs::write(output, serde_json::to_vec_pretty(&report)?)?;
    }
    println!("{}", serde_json::to_string_pretty(&report)?);
    ensure!(
        passed,
        "Local SQLite probe did not satisfy its acceptance checks"
    );
    Ok(())
}
