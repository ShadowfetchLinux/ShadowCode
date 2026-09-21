#![cfg(unix)]
use serde_json::{json, Value};
use shadowcode_core::mcp::{Client, StdioSpec};
use std::{collections::BTreeMap, path::PathBuf, time::Duration};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

struct Fixture {
    root: TempDir,
    spec: StdioSpec,
}
impl Fixture {
    fn new(mode: &str) -> Self {
        let root = TempDir::new().unwrap();
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mcp-server.mjs");
        let spec = StdioSpec {
            command: vec![
                "node".into(),
                fixture.to_string_lossy().into(),
                mode.into(),
                "literal $HOME ; touch owned".into(),
            ],
            env: BTreeMap::from([
                (
                    "MCP_PID_FILE".into(),
                    root.path().join("pids.json").to_string_lossy().into(),
                ),
                (
                    "MCP_REQUEST_FILE".into(),
                    root.path().join("requests.jsonl").to_string_lossy().into(),
                ),
                ("MCP_LITERAL".into(), "value $HOME ; touch owned".into()),
            ]),
            timeout: Duration::from_secs(3),
        };
        Self { root, spec }
    }
    async fn connect(&self, cancel: CancellationToken) -> anyhow::Result<Client> {
        Client::connect(&self.spec, self.root.path(), cancel).await
    }
    fn pids(&self) -> Vec<u32> {
        serde_json::from_slice(&std::fs::read(self.root.path().join("pids.json")).unwrap()).unwrap()
    }
    async fn stopped(&self) {
        let pids = self.pids();
        for pid in &pids {
            for _ in 0..100 {
                let running = std::fs::read_to_string(format!("/proc/{pid}/stat"))
                    .ok()
                    .is_some_and(|s| !s.rsplit_once(") ").unwrap().1.starts_with('Z'));
                if !running {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            if let Ok(s) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
                assert!(
                    s.rsplit_once(") ").unwrap().1.starts_with('Z'),
                    "MCP process {pid} survived cleanup"
                );
            }
        }
        for _ in 0..100 {
            if !std::path::Path::new(&format!("/proc/{}", pids[0])).exists() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("MCP leader {} was not reaped", pids[0]);
    }
    fn requests(&self) -> Vec<Value> {
        std::fs::read_to_string(self.root.path().join("requests.jsonl"))
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect()
    }
}

#[tokio::test]
async fn native_mcp_handshake_paginated_catalog_literal_arguments_and_error_results() {
    let fixture = Fixture::new("paginated");
    let mut client = fixture.connect(CancellationToken::new()).await.unwrap();
    assert_eq!(client.tools().len(), 2);
    assert_eq!(client.pid(), Some(fixture.pids()[0]));
    let args = json!({"literal":"$(touch owned)","unicode":"雪"});
    let result = client.call("echo", args.clone()).await.unwrap();
    assert_eq!(result["structuredContent"]["arguments"], args);
    assert_eq!(
        result["structuredContent"]["literal"],
        "literal $HOME ; touch owned"
    );
    assert_eq!(
        result["structuredContent"]["environment"],
        "value $HOME ; touch owned"
    );
    let environment = result["structuredContent"]["environmentKeys"]
        .as_array()
        .unwrap();
    assert!(!environment.contains(&json!("LD_LIBRARY_PATH")));
    assert!(!environment.contains(&json!("CARGO_MANIFEST_DIR")));
    assert_eq!(
        result["structuredContent"]["cwd"],
        fixture.root.path().to_string_lossy().as_ref()
    );
    assert_eq!(
        client.call("failure", json!({})).await.unwrap()["isError"],
        true
    );
    assert!(client.call("not-present", json!({})).await.is_err());
    assert!(client.call("echo", json!("not an object")).await.is_err());
    client.close().await.unwrap();
    client.close().await.unwrap();
    fixture.stopped().await;
    assert!(!fixture.root.path().join("owned").exists());
    let requests = fixture.requests();
    assert_eq!(requests[0]["method"], "initialize");
    assert_eq!(requests[0]["params"]["capabilities"], json!({}));
    assert!(requests
        .iter()
        .any(|r| r["method"] == "notifications/initialized"));
    assert_eq!(
        requests
            .iter()
            .filter(|r| r["method"] == "tools/call")
            .count(),
        2
    );
}

#[tokio::test]
async fn native_mcp_rejects_malformed_oversized_and_incomplete_initialization() {
    for mode in ["init_oversize", "init_malformed", "init_truncated"] {
        let fixture = Fixture::new(mode);
        let error = fixture
            .connect(CancellationToken::new())
            .await
            .err()
            .unwrap();
        assert!(format!("{error:#}").contains("MCP"));
        fixture.stopped().await;
    }
}

#[tokio::test]
async fn native_mcp_bounds_catalog_size_pagination_and_duplicate_names() {
    for (mode, reason) in [
        ("duplicate", "duplicate"),
        ("cursor", "cursor"),
        ("many", "128 tools"),
        ("pages", "eight pages"),
    ] {
        let fixture = Fixture::new(mode);
        let error = fixture
            .connect(CancellationToken::new())
            .await
            .err()
            .unwrap();
        assert!(format!("{error:#}").contains(reason), "{mode}: {error:#}");
        fixture.stopped().await;
    }
}

#[tokio::test]
async fn native_mcp_timeouts_and_cancellation_reap_stubborn_process_groups() {
    let mut fixture = Fixture::new("init_hang");
    fixture.spec.timeout = Duration::from_millis(500);
    let error = fixture
        .connect(CancellationToken::new())
        .await
        .err()
        .unwrap();
    assert!(format!("{error:#}").contains("timed out"));
    fixture.stopped().await;
    for cancel_it in [false, true] {
        let fixture = Fixture::new("normal");
        let cancel = CancellationToken::new();
        let mut client = fixture.connect(cancel.clone()).await.unwrap();
        let cancel_worker = tokio::spawn(async move {
            if cancel_it {
                tokio::time::sleep(Duration::from_millis(100)).await;
                cancel.cancel();
            }
        });
        assert!(client.call("hang", json!({})).await.is_err());
        cancel_worker.await.unwrap();
        fixture.stopped().await;
        assert!(client.call("echo", json!({})).await.is_err());
    }
}

#[tokio::test]
async fn native_mcp_idle_cancellation_and_drop_clean_up_owned_servers() {
    for drop_it in [false, true] {
        let fixture = Fixture::new("normal");
        let cancel = CancellationToken::new();
        let mut client = fixture.connect(cancel.clone()).await.unwrap();
        if drop_it {
            drop(client);
        } else {
            cancel.cancel();
            fixture.stopped().await;
            client.close().await.unwrap();
        }
        fixture.stopped().await;
    }
}

#[tokio::test]
async fn native_mcp_stderr_flood_drains_and_protocol_flood_fails_closed() {
    let fixture = Fixture::new("normal");
    let mut client = fixture.connect(CancellationToken::new()).await.unwrap();
    assert_eq!(
        client.call("noise", json!({})).await.unwrap()["isError"],
        false
    );
    assert_eq!(client.stderr().len(), 16_384);
    client.close().await.unwrap();
    fixture.stopped().await;
    for name in ["oversize", "rate"] {
        let fixture = Fixture::new("normal");
        let mut client = fixture.connect(CancellationToken::new()).await.unwrap();
        assert!(client.call(name, json!({})).await.is_err());
        fixture.stopped().await;
    }
}

#[tokio::test]
async fn native_mcp_does_not_grant_sampling_or_interactive_credentials() {
    let fixture = Fixture::new("normal");
    let mut client = fixture.connect(CancellationToken::new()).await.unwrap();
    let sample = client.call("sample", json!({})).await.unwrap();
    assert_eq!(
        sample["structuredContent"]["response"]["error"]["code"],
        -32601
    );
    let interact = client.call("interact", json!({})).await.unwrap();
    assert_eq!(
        interact["structuredContent"]["response"]["result"]["action"],
        "decline"
    );
    client.close().await.unwrap();
    fixture.stopped().await;
}

#[tokio::test]
async fn native_mcp_invalid_specs_and_precancel_do_not_spawn() {
    let mut fixture = Fixture::new("normal");
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(fixture.connect(cancel).await.is_err());
    fixture.spec.command.clear();
    assert!(fixture.connect(CancellationToken::new()).await.is_err());
    assert!(!fixture.root.path().join("pids.json").exists());
}

#[tokio::test]
async fn native_mcp_supports_legacy_protocol_negotiation() {
    let fixture = Fixture::new("legacy");
    let mut client = fixture.connect(CancellationToken::new()).await.unwrap();
    assert_eq!(
        client.call("echo", json!({"legacy":true})).await.unwrap()["isError"],
        false
    );
    client.close().await.unwrap();
    fixture.stopped().await;
}

#[tokio::test]
async fn native_mcp_abandoned_initialization_cleans_up_without_waiting_for_timeout() {
    let fixture = Fixture::new("init_hang");
    let spec = fixture.spec.clone();
    let path = fixture.root.path().to_path_buf();
    let worker =
        tokio::spawn(async move { Client::connect(&spec, &path, CancellationToken::new()).await });
    for _ in 0..100 {
        if fixture.root.path().join("pids.json").exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(fixture.root.path().join("pids.json").exists());
    worker.abort();
    assert!(matches!(worker.await, Err(error) if error.is_cancelled()));
    fixture.stopped().await;
}
