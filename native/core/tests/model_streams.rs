use serde_json::json;
use shadowcode_core::{
    config::ModelConfig,
    models::{ModelClient, StreamDecoder},
    paths::AppPaths,
};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_util::sync::CancellationToken;

fn sse(value: serde_json::Value) -> String {
    format!("data: {value}\n\n")
}

#[test]
fn ollama_templates_receive_runtime_guidance_without_promoting_user_or_tool_data() {
    let root = tempfile::tempdir().unwrap();
    let paths = AppPaths::isolated(root.path()).unwrap();
    let mut client = ModelClient::new(
        ModelConfig {
            provider: "ollama".into(),
            name: "fixture".into(),
            ..Default::default()
        },
        &paths,
    )
    .unwrap();
    let messages = vec![
        json!({"role":"system","content":"Application instructions"}),
        json!({"role":"user","content":"Untrusted user text"}),
        json!({"role":"assistant","content":"","tool_calls":[{"id":"a","function":{"name":"read_file","arguments":"{\"path\":\"README.md\"}"}}]}),
        json!({"role":"tool","tool_call_id":"a","name":"read_file","content":"Untrusted file contents"}),
        json!({"role":"system","content":"Completion check failed: repair the file","_shadow_note":true}),
        json!({"role":"assistant","content":"Repair complete"}),
        json!({"role":"system","content":"Context compacted; consult current evidence","_shadow_compaction":true}),
    ];
    let original = messages.clone();
    let body = client.request_body(&messages, &[], 100);
    let converted = body["messages"].as_array().unwrap();
    assert_eq!(
        converted.iter().filter(|m| m["role"] == "system").count(),
        1
    );
    let leading = converted[0]["content"].as_str().unwrap();
    assert!(leading.starts_with("Application instructions"));
    assert!(
        leading.contains("position 4")
            && leading.contains("Completion check failed: repair the file")
    );
    assert!(leading.contains("position 6") && leading.contains("Context compacted"));
    assert!(!leading.contains("Untrusted"));
    assert_eq!(converted[1], messages[1]);
    assert_eq!(
        converted[2]["tool_calls"][0]["function"]["arguments"]["path"],
        "README.md"
    );
    assert_eq!(converted[3]["role"], "tool");
    assert_eq!(converted[3]["tool_name"], "read_file");
    assert_eq!(converted[3]["content"], "Untrusted file contents");
    assert!(converted[3].get("tool_call_id").is_none());
    assert_eq!(converted[4], messages[5]);
    assert_eq!(messages, original, "Stored history stays chronological");
    client.config.provider = "local".into();
    let compatible = client.request_body(&messages, &[], 100);
    assert_eq!(compatible["messages"][4]["role"], "system");
    assert_eq!(compatible["messages"][4]["content"], messages[4]["content"]);
    assert!(!compatible.to_string().contains("_shadow_"));
}

#[test]
fn compatible_stream_reassembles_utf8_and_interleaved_tool_arguments() {
    let wire=[
        sse(json!({"choices":[{"delta":{"content":"Hello 🌒","tool_calls":[{"index":0,"id":"call_a","function":{"name":"read_file","arguments":"{\"pa"}},{"index":1,"id":"call_b","function":{"name":"search_text","arguments":"{\"pattern\":"}}]}}]})),
        sse(json!({"choices":[{"delta":{"tool_calls":[{"index":1,"function":{"arguments":"\"TODO\"}"}},{"index":0,"function":{"arguments":"th\":\"src/main.rs\"}"}}]}}]})),
        sse(json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]})),
        sse(json!({"choices":[],"usage":{"prompt_tokens":11,"completion_tokens":7}})),
        "data: [DONE]\n\n".into(),
    ].concat();
    // Every byte boundary is a legal network chunk boundary.
    for chunk_size in [1, 2, 3, 7, 31, 4096] {
        let mut decoder = StreamDecoder::new(false);
        let mut visible = String::new();
        for chunk in wire.as_bytes().chunks(chunk_size) {
            for delta in decoder.push(chunk).unwrap() {
                visible.push_str(&delta);
            }
        }
        decoder.flush().unwrap();
        let result = decoder.finish().unwrap();
        assert_eq!(visible, "Hello 🌒");
        assert_eq!(result.text, visible);
        assert_eq!(result.tool_calls.len(), 2);
        assert_eq!(result.tool_calls[0].id, "call_a");
        assert_eq!(result.tool_calls[0].arguments["path"], "src/main.rs");
        assert_eq!(result.tool_calls[1].arguments["pattern"], "TODO");
        assert_eq!(result.usage.total_tokens, 18);
    }
}

#[test]
fn compatible_stream_continues_unindexed_argument_deltas() {
    let wire = [
        sse(json!({"choices":[{"delta":{"tool_calls":[{"id":"call_a","function":{"name":"read_file","arguments":"{\"pa"}}]}}]})),
        sse(json!({"choices":[{"delta":{"tool_calls":[{"function":{"arguments":"th\":\"README.md\"}"}}]}}]})),
        sse(json!({"choices":[{"delta":{"tool_calls":[{"id":"call_b","function":{"name":"search_text","arguments":"{\"pattern\":\"TODO\"}"}}]}}]})),
        sse(json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]})),
        "data: [DONE]\n\n".into(),
    ]
    .concat();
    for chunk_size in [1, 5, 4096] {
        let mut decoder = StreamDecoder::new(false);
        for chunk in wire.as_bytes().chunks(chunk_size) {
            decoder.push(chunk).unwrap();
        }
        decoder.flush().unwrap();
        let result = decoder.finish().unwrap();
        assert_eq!(result.tool_calls.len(), 2);
        assert_eq!(result.tool_calls[0].id, "call_a");
        assert_eq!(result.tool_calls[0].arguments["path"], "README.md");
        assert_eq!(result.tool_calls[1].id, "call_b");
        assert_eq!(result.tool_calls[1].arguments["pattern"], "TODO");
    }
}

#[test]
fn compatible_stream_ignores_repeated_call_identity() {
    let wire = [
        sse(json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_a","function":{"name":"read_file","arguments":"{\"path\":"}}]}}]})),
        sse(json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_a","function":{"name":"read_file","arguments":"\"README.md\"}"}}]}}]})),
        sse(json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]})),
        "data: [DONE]\n\n".into(),
    ]
    .concat();
    let mut decoder = StreamDecoder::new(false);
    decoder.push(wire.as_bytes()).unwrap();
    decoder.flush().unwrap();
    let result = decoder.finish().unwrap();
    assert_eq!(result.tool_calls.len(), 1);
    assert_eq!(result.tool_calls[0].id, "call_a");
    assert_eq!(result.tool_calls[0].name, "read_file");
    assert_eq!(result.tool_calls[0].arguments["path"], "README.md");
}

#[test]
fn incomplete_invalid_and_cut_short_calls_are_never_accepted() {
    let start = sse(
        json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"x","function":{"name":"exec","arguments":"{\"command\":"}}]}}]}),
    );
    let mut decoder = StreamDecoder::new(false);
    decoder.push(start.as_bytes()).unwrap();
    assert!(decoder.finish().is_err());
    for reason in ["length", "content_filter", "tool_calls"] {
        let mut decoder = StreamDecoder::new(false);
        decoder.push(start.as_bytes()).unwrap();
        decoder
            .push(sse(json!({"choices":[{"delta":{},"finish_reason":reason}]})).as_bytes())
            .unwrap();
        assert!(decoder.finish().is_err());
    }
    let mut decoder = StreamDecoder::new(false);
    assert!(decoder.push(b"data: {invalid}\n\n").is_err());
    let mut decoder = StreamDecoder::new(true);
    assert!(decoder.push(&vec![b'a'; 1_000_001]).is_err());
}

#[test]
fn ollama_native_stream_retains_calls_and_usage_but_not_private_thinking() {
    let wire=[json!({"message":{"content":"Inspecting","thinking":"private scratch"},"done":false}),
        json!({"message":{"content":"","tool_calls":[{"function":{"name":"read_file","arguments":{"path":"README.md"}}}]},"done":false}),
        json!({"message":{"content":""},"done":true,"done_reason":"stop","prompt_eval_count":20,"eval_count":4})]
        .iter().map(|v|format!("{v}\n")).collect::<String>();
    let mut decoder = StreamDecoder::new(true);
    for chunk in wire.as_bytes().chunks(13) {
        decoder.push(chunk).unwrap();
    }
    let result = decoder.finish().unwrap();
    assert_eq!(result.text, "Inspecting");
    assert_eq!(result.usage.total_tokens, 24);
    assert_eq!(result.tool_calls[0].name, "read_file");
    assert_eq!(result.tool_calls[0].arguments["path"], "README.md");
}

async fn fixture(
    body: String,
    content_type: &str,
    hold: bool,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let content_type = content_type.to_owned();
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0; 1024];
        loop {
            let count = socket.read(&mut buffer).await.unwrap();
            if count == 0 {
                return;
            }
            request.extend_from_slice(&buffer[..count]);
            if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..end]).to_lowercase();
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("content-length:")
                            .and_then(|s| s.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if request.len() >= end + 4 + length {
                    break;
                }
            }
        }
        socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).as_bytes()).await.unwrap();
        if hold {
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
        for chunk in body.as_bytes().chunks(7) {
            if socket.write_all(chunk).await.is_err() {
                return;
            }
        }
    });
    (endpoint, task)
}

#[tokio::test]
async fn real_http_transport_handles_streaming_and_nonstreaming_fallback() {
    let root = tempfile::tempdir().unwrap();
    let paths = AppPaths::isolated(root.path()).unwrap();
    for (content_type, body) in [
        (
            "text/event-stream",
            sse(json!({"choices":[{"delta":{"content":"connected"},"finish_reason":"stop"}]}))
                + "data: [DONE]\n\n",
        ),
        (
            "application/json",
            json!({"choices":[{"message":{"content":"connected"},"finish_reason":"stop"}]})
                .to_string(),
        ),
    ] {
        let (endpoint, task) = fixture(body, content_type, false).await;
        let config = ModelConfig {
            provider: "local".into(),
            name: "fixture".into(),
            endpoint,
            ..Default::default()
        };
        let client = ModelClient::new(config, &paths).unwrap();
        let mut visible = String::new();
        let result = client
            .chat(
                &[json!({"role":"user","content":"hello"})],
                &[],
                CancellationToken::new(),
                |text| visible.push_str(text),
            )
            .await
            .unwrap();
        assert_eq!(result.text, "connected");
        assert_eq!(visible, "connected");
        task.await.unwrap();
    }
}

#[tokio::test]
async fn cancellation_interrupts_a_provider_that_never_sends_a_body() {
    let root = tempfile::tempdir().unwrap();
    let paths = AppPaths::isolated(root.path()).unwrap();
    let (endpoint, task) = fixture("waiting".into(), "text/event-stream", true).await;
    let client = ModelClient::new(
        ModelConfig {
            provider: "local".into(),
            endpoint,
            ..Default::default()
        },
        &paths,
    )
    .unwrap();
    let cancel = CancellationToken::new();
    let child = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        child.cancel();
    });
    let start = std::time::Instant::now();
    let result = client.chat(&[], &[], cancel, |_| {}).await;
    task.abort();
    assert!(result.unwrap_err().to_string().contains("cancelled"));
    assert!(start.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn malformed_full_response_is_an_error_instead_of_a_panic() {
    let root = tempfile::tempdir().unwrap();
    let paths = AppPaths::isolated(root.path()).unwrap();
    let (endpoint, task) = fixture("{\"choices\":[]}".into(), "application/json", false).await;
    let client = ModelClient::new(
        ModelConfig {
            provider: "local".into(),
            endpoint,
            ..Default::default()
        },
        &paths,
    )
    .unwrap();
    assert!(client
        .chat(&[], &[], CancellationToken::new(), |_| {})
        .await
        .is_err());
    task.await.unwrap();
}

#[test]
fn thousands_of_frames_in_one_network_chunk_preserve_all_text() {
    let mut wire = sse(json!({"choices":[{"delta":{"content":"word "}}]})).repeat(10_000);
    wire.push_str(&sse(
        json!({"choices":[{"delta":{},"finish_reason":"stop"}]}),
    ));
    let mut decoder = StreamDecoder::new(false);
    let deltas = decoder.push(wire.as_bytes()).unwrap();
    assert_eq!(deltas.len(), 10_000);
    assert_eq!(decoder.finish().unwrap().text, "word ".repeat(10_000));
}
