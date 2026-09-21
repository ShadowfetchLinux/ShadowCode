use serde_json::Value;
use shadowcode_core::mcp::{http::HttpSpec, Client};
use std::{fs, path::PathBuf, process::Stdio, time::Duration};
use tokio::process::{Child, Command};
use tokio_util::sync::CancellationToken;
pub struct Fixture {
    root: tempfile::TempDir,
    _child: Child,
    pub spec: HttpSpec,
}
impl Fixture {
    pub async fn new(mode: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let child = Command::new("node")
            .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mcp-http.mjs"))
            .arg(mode)
            .arg(root.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let ready = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Ok(bytes) = fs::read(root.path().join("ready.json")) {
                    break serde_json::from_slice::<Value>(&bytes).unwrap();
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        Self {
            root,
            _child: child,
            spec: HttpSpec {
                url: ready["url"].as_str().unwrap().into(),
                bearer_token: None,
                timeout: Duration::from_secs(2),
            },
        }
    }
    pub async fn connect(&self, cancel: CancellationToken) -> anyhow::Result<Client> {
        Client::connect_http(&self.spec, cancel).await
    }
    pub fn requests(&self) -> Vec<Value> {
        fs::read_to_string(self.root.path().join("requests.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect()
    }
    pub async fn closed_streams(&self) {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let response = reqwest::Client::builder()
                    .no_proxy()
                    .build()
                    .unwrap()
                    .get(self.spec.url.replace("/mcp", "/status"))
                    .send()
                    .await
                    .unwrap()
                    .json::<Value>()
                    .await
                    .unwrap();
                if response["streams"] == 0 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("HTTP streams survived connection cleanup");
    }
}
