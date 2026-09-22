//! Read-only live-provider probe; all profile data is isolated in a temp dir.
use anyhow::{ensure, Result};
use serde_json::json;
use shadowcode_core::{config::ModelConfig, models::ModelClient, paths::AppPaths};
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<()> {
    let model = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "gpt-oss:20b".into());
    let root = tempfile::tempdir()?;
    let paths = AppPaths::isolated(root.path())?;
    let client = ModelClient::new(
        ModelConfig {
            provider: "ollama".into(),
            name: model.clone(),
            default: model,
            endpoint: "http://127.0.0.1:11434/v1".into(),
            api_key_env: "OLLAMA_API_KEY".into(),
            context_limit: 4096,
            ..Default::default()
        },
        &paths,
    )?;
    let tools = vec![
        json!({"type":"function","function":{"name":"read_file","description":"Read a workspace file","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}}}),
    ];
    let mut messages = vec![
        json!({"role":"system","content":"Use the supplied tool to inspect files. Never invent file contents."}),
        json!({"role":"user","content":"Read example.rs using read_file, then state the exact integer returned by add(2, 3)."}),
    ];
    let first = client
        .chat(&messages, &tools, CancellationToken::new(), |_| {})
        .await?;
    ensure!(
        first.tool_calls.len() == 1,
        "Expected one tool call, received {}",
        first.tool_calls.len()
    );
    let call = &first.tool_calls[0];
    ensure!(
        call.name == "read_file" && call.arguments["path"] == "example.rs",
        "Unexpected tool call"
    );
    messages.push(json!({"role":"assistant","content":first.text,"tool_calls":[{"id":call.id,"type":"function","function":{"name":call.name,"arguments":call.arguments.to_string()}}]}));
    messages.push(json!({"role":"tool","name":call.name,"tool_call_id":call.id,"content":"pub fn add(a: i32, b: i32) -> i32 { a + b }"}));
    let answer = client
        .chat(&messages, &tools, CancellationToken::new(), |text| {
            print!("{text}")
        })
        .await?;
    ensure!(
        answer.tool_calls.is_empty() && answer.text.contains('5'),
        "Expected an answer containing 5"
    );
    println!("\nNative provider probe passed: streamed tool call, supplied result, completed response; {} total tokens",first.usage.total_tokens+answer.usage.total_tokens);
    Ok(())
}
