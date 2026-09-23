//! Real local-model background-server probe in a disposable project/profile.
//! Node is test infrastructure; it is not a ShadowCode runtime dependency.
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
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
    let marker = format!("verified-{}", shadowcode_core::id());
    fs::write(project.join("server.mjs"), format!(
        "import http from 'node:http';\nimport fs from 'node:fs';\nconst server=http.createServer((req,res)=>res.end({}));\nserver.listen(0,'127.0.0.1',()=>{{fs.writeFileSync('server-port.txt',String(server.address().port));console.log('SERVER_READY {}');}});\n",
        serde_json::to_string(&marker)?, marker
    ))?;
    let paths = AppPaths::isolated(&root.path().join("profile"))?;
    Config::patch(
        &paths,
        json!({"model":{"provider":"ollama","endpoint":"http://127.0.0.1:11434/v1","name":model,"default":model,"context_limit":16384},"trusted_workspaces":[project],"permissions":{"approve_shell":true},"agent":{"max_steps":10,"max_task_tokens":80000,"model_retries":0}}),
    )?;
    let engine = Engine::open(paths)?;
    let job = engine.start(StartRequest {
        workspace:project.clone(), session_id:None, model:None, mode:"code".into(), queue:false,
        task:"Start the existing server as a managed project background process named probe-server, with the exact command node server.mjs. Use background_start, then background_output to inspect its retained log until SERVER_READY appears. Report the actual marker following SERVER_READY and leave the server running. Do not use exec, change files or stop the server; the test harness will independently check its HTTP response and clean it up.".into(),
        images: Vec::new(),
        web: false,
        }).await?;
    let mut approvals = 0;
    let finished = tokio::time::timeout(Duration::from_secs(300), async {
        let completion = engine.wait(&job.id);
        tokio::pin!(completion);
        loop {
            tokio::select! {
                result = &mut completion => break result,
                _ = tokio::time::sleep(Duration::from_millis(20)) => {
                    for approval in engine.approvals().list(Some(&job.session_id)) {
                        let allowed = approval.task_id == job.task_id && approval.tool == "background_start"
                            && approval.arguments == json!({"name":"probe-server","command":"node server.mjs"});
                        engine.approvals().decide(&approval.id, &approval.session_id, allowed)?;
                        if allowed { approvals += 1; }
                    }
                }
            }
        }
    }).await;
    // Always shut down the isolated owner, even if verification or the model fails.
    let verification: Result<Value> = async {
        let finished = finished.context("Local model probe timed out")??;
        let tasks = engine.background().list(&project)?;
        let process = tasks.iter().find(|task| task.name == "probe-server").context("Model did not start the requested background process")?;
        let port: u16 = fs::read_to_string(project.join("server-port.txt"))?.trim().parse()?;
        let client = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(3)).build()?;
        let actual = client.get(format!("http://127.0.0.1:{port}/")).send().await?.error_for_status()?.text().await?;
        let events = engine.store().recent_events(&job.session_id, 200)?;
        let inspected = events.iter().any(|event| event["type"] == "tool.completed"
            && event["payload"]["tool"] == "background_output" && event["payload"]["success"] == true
            && event["payload"]["output"]["output"].as_str().is_some_and(|log| log.contains(&marker)));
        let passed = finished.status == "completed" && approvals == 1 && tasks.len() == 1
            && process.status == "RUNNING" && process.origin_task_id.as_deref() == Some(job.task_id.as_str())
            && inspected && actual == marker && finished.summary.contains(&marker);
        Ok(json!({"passed":passed,"model":finished.model,"requested_model":model,"status":finished.status,"summary":finished.summary,
            "approvals":approvals,"steps":finished.steps,"usage":finished.usage,"server_port":port,"http_response":actual,"expected_marker":marker,
            "process_status_after_task":process.status,"model_read_live_log":inspected,
            "tools":events.iter().filter(|e|matches!(e["type"].as_str(),Some("tool.started"|"tool.completed"))).collect::<Vec<_>>() }))
    }.await;
    engine.shutdown().await?;
    let mut report = verification?;
    let all_stopped = engine
        .background()
        .list(&project)?
        .iter()
        .all(|task| task.status == "CANCELLED");
    let port = report["server_port"]
        .as_u64()
        .context("Missing server port")? as u16;
    let closed = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .is_err();
    report["shutdown_stopped_server"] = json!(all_stopped && closed);
    report["passed"] = json!(report["passed"] == true && all_stopped && closed);
    if let Some(output) = output {
        fs::write(output, serde_json::to_vec_pretty(&report)?)?;
    }
    println!("{}", serde_json::to_string_pretty(&report)?);
    ensure!(
        report["passed"] == true,
        "Local background probe did not satisfy its acceptance checks"
    );
    Ok(())
}
