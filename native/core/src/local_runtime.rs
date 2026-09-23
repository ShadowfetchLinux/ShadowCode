//! One managed llama-server process. Only one GGUF is loaded at a time.
//!
//! The process is bound to 127.0.0.1. It is not an Ollama or LM Studio daemon.
use anyhow::{bail, ensure, Context, Result};
use std::{
    net::TcpListener,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{process::{Child, Command}, time::sleep};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(90);

pub struct LoadedServer {
    child: Child,
    pub port: u16,
    pub model_path: PathBuf,
    pub binary: PathBuf,
    pub endpoint: String,
}

impl LoadedServer {
    pub fn matches(&self, binary: &Path, model: &Path) -> bool {
        self.binary == binary && self.model_path == model
    }
    pub async fn stop(&mut self) {
        let _ = self.child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(3), self.child.wait()).await;
    }
}

pub async fn ensure_loaded(
    slot: &mut Option<LoadedServer>,
    binary: &Path,
    model: &Path,
) -> Result<String> {
    ensure!(binary.is_file(), "llama.cpp binary is missing: {}", binary.display());
    ensure!(model.is_file(), "GGUF file is missing: {}", model.display());
    if let Some(current) = slot.as_ref() {
        if current.matches(binary, model) {
            return Ok(current.endpoint.clone());
        }
    }
    if let Some(mut previous) = slot.take() {
        previous.stop().await;
    }
    let loaded = spawn(binary, model).await?;
    let endpoint = loaded.endpoint.clone();
    *slot = Some(loaded);
    Ok(endpoint)
}

pub async fn stop(slot: &mut Option<LoadedServer>) {
    if let Some(mut previous) = slot.take() {
        previous.stop().await;
    }
}

fn free_loopback_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").context("Could not reserve a loopback port")?;
    Ok(listener.local_addr()?.port())
}

async fn spawn(binary: &Path, model: &Path) -> Result<LoadedServer> {
    let port = free_loopback_port()?;
    let mut child = Command::new(binary)
        .args([
            "-m",
            &model.display().to_string(),
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--ctx-size",
            "2048",
            "--parallel",
            "1",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("Failed to start {}", binary.display()))?;
    let endpoint = format!("http://127.0.0.1:{port}/v1");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .connect_timeout(Duration::from_secs(1))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let started = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            bail!(
                "llama-server exited before it became ready ({status}). The GGUF may be incomplete or not a loadable model."
            );
        }
        if ready(&client, port).await {
            return Ok(LoadedServer {
                child,
                port,
                model_path: model.to_path_buf(),
                binary: binary.to_path_buf(),
                endpoint,
            });
        }
        if started.elapsed() > STARTUP_TIMEOUT {
            let _ = child.start_kill();
            bail!("llama-server did not become ready on 127.0.0.1:{port} within 90s");
        }
        sleep(Duration::from_millis(200)).await;
    }
}

async fn ready(client: &reqwest::Client, port: u16) -> bool {
    for path in ["/health", "/v1/models"] {
        let url = format!("http://127.0.0.1:{port}{path}");
        if let Ok(response) = client.get(url).send().await {
            if response.status().is_success() {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserves_a_loopback_port() {
        let port = free_loopback_port().unwrap();
        assert!(port > 0);
    }
}
