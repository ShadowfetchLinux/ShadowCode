//! Built-in local GGUF catalog and optional llama.cpp spawn.
//!
//! Users add files or directories they already have. Removing a catalog entry
//! never deletes weights. Ollama tags are not GGUF and are not imported here.
use anyhow::{bail, ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs, io::Read,
    path::{Path, PathBuf},
};

const GGUF_MAGIC: &[u8; 4] = b"GGUF";
const MAX_SCAN_FILES: usize = 256;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct LocalEngineConfig {
    pub directories: Vec<String>,
    pub files: Vec<String>,
    pub llama_binary: String,
}

impl LocalEngineConfig {
    pub fn from_value(value: &Value) -> Result<Self> {
        let config: Self = serde_json::from_value(value.clone())
            .map_err(|e| anyhow::anyhow!("Invalid local_engine configuration: {e}"))?;
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(self.directories.len() <= 32, "Too many local model directories");
        ensure!(self.files.len() <= 64, "Too many local model files");
        if self.llama_binary.len() > 1024 || self.llama_binary.contains(['\n', '\0']) {
            bail!("local_engine.llama_binary is not a usable path");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct GgufEntry {
    pub id: String,
    pub name: String,
    pub path: String,
    pub bytes: u64,
    pub compatible: bool,
    pub vision: bool,
    pub tools: bool,
    pub approx_memory_bytes: u64,
    pub detail: String,
}

pub fn is_gguf(path: &Path) -> bool {
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return false,
    };
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic).is_ok() && magic == *GGUF_MAGIC
}

fn entry_id(path: &Path) -> String {
    format!(
        "local:gguf:{}",
        crate::workspace::hash(path.to_string_lossy().as_bytes())
    )
}

pub fn inspect_gguf(path: &Path, ram_bytes: u64) -> Result<GgufEntry> {
    ensure!(path.is_file(), "Not a file");
    ensure!(is_gguf(path), "Not a GGUF file (missing GGUF magic)");
    let bytes = fs::metadata(path)?.len();
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("local-model");
    let lower = stem.to_ascii_lowercase();
    let vision = ["llava", "vision", "vl", "moondream", "pixtral"]
        .iter()
        .any(|n| lower.contains(n));
    let tools = !["chat-only", "instruct-only"].iter().any(|n| lower.contains(n));
    let overhead = 512 * 1024 * 1024;
    let context_cache = 512 * 1024 * 1024;
    let approx = bytes.saturating_add(overhead).saturating_add(context_cache);
    let compatible = ram_bytes == 0 || approx < ram_bytes.saturating_mul(90) / 100;
    Ok(GgufEntry {
        id: entry_id(path),
        name: stem.to_owned(),
        path: path.display().to_string(),
        bytes,
        compatible,
        vision,
        tools,
        approx_memory_bytes: approx,
        detail: if compatible {
            format!(
                "GGUF · ≈{:.1} GB loaded (weights + context cache + overhead)",
                approx as f64 / 1_000_000_000.0
            )
        } else {
            "File is GGUF but likely exceeds available RAM".into()
        },
    })
}

fn add_file(path: &Path, ram_bytes: u64, out: &mut Vec<GgufEntry>) {
    if out.len() >= MAX_SCAN_FILES {
        return;
    }
    if let Ok(entry) = inspect_gguf(path, ram_bytes) {
        if !out.iter().any(|e| e.path == entry.path) {
            out.push(entry);
        }
    }
}

pub fn scan(config: &LocalEngineConfig, ram_bytes: u64) -> Vec<GgufEntry> {
    let mut out = Vec::new();
    for file in &config.files {
        add_file(Path::new(file), ram_bytes, &mut out);
    }
    for dir in &config.directories {
        let path = Path::new(dir);
        if !path.is_dir() {
            continue;
        }
        let Ok(read) = fs::read_dir(path) else {
            continue;
        };
        for child in read.flatten() {
            let child = child.path();
            if child.extension().and_then(|e| e.to_str()) == Some("gguf") {
                add_file(&child, ram_bytes, &mut out);
            }
        }
    }
    out
}

pub fn hardware() -> Value {
    let ram = read_meminfo();
    let (gpu, vram) = read_nvidia();
    json!({
        "cpu_cores": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
        "ram_bytes": ram,
        "gpu": gpu,
        "vram_bytes": vram,
        "acceleration": if gpu.is_some() { "gpu_preferred_cpu_fallback" } else { "cpu" },
    })
}

fn read_meminfo() -> u64 {
    let Ok(text) = fs::read_to_string("/proc/meminfo") else {
        return 0;
    };
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            let kb = rest.split_whitespace().next().and_then(|n| n.parse::<u64>().ok());
            return kb.unwrap_or(0).saturating_mul(1024);
        }
    }
    0
}

fn read_nvidia() -> (Option<String>, Option<u64>) {
    let Some(output) = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"])
        .output()
        .ok()
    else {
        return (None, None);
    };
    if !output.status.success() {
        return (None, None);
    }
    let line = String::from_utf8_lossy(&output.stdout);
    let line = line.lines().next().unwrap_or("").trim();
    let mut parts = line.split(',');
    let Some(name) = parts.next().map(str::trim).filter(|s| !s.is_empty()) else {
        return (None, None);
    };
    let vram_mb = parts
        .next()
        .map(str::trim)
        .and_then(|s| s.parse::<u64>().ok());
    (
        Some(name.to_owned()),
        vram_mb.map(|mb| mb.saturating_mul(1024 * 1024)),
    )
}

pub fn llama_binary_status(configured: &str) -> Value {
    let candidate = if configured.trim().is_empty() {
        super::cli_agent::resolve_binary("llama-cli")
            .or_else(|| super::cli_agent::resolve_binary("llama-server"))
    } else {
        let path = PathBuf::from(configured.trim());
        path.is_file().then_some(path)
    };
    match candidate {
        Some(path) => json!({
            "state": "ready",
            "path": path,
            "detail": "llama.cpp binary is present; local GGUF rows can load after you pick a file",
        }),
        None => json!({
            "state": "setup_required",
            "path": null,
            "detail": "No llama.cpp binary yet. Point ShadowCode at an official llama-cli or llama-server you already have. Models are never auto-downloaded.",
        }),
    }
}

pub fn catalog(config: &LocalEngineConfig) -> Value {
    let hw = hardware();
    let ram = hw["ram_bytes"].as_u64().unwrap_or(0);
    let models = scan(config, ram);
    let binary = llama_binary_status(&config.llama_binary);
    json!({
        "hardware": hw,
        "llama": binary,
        "models": models,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn rejects_non_gguf_and_does_not_delete() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("model.bin");
        fs::write(&fake, b"NOTG").unwrap();
        assert!(!is_gguf(&fake));
        assert!(inspect_gguf(&fake, 8_000_000_000).is_err());
        let mut gguf = fs::File::create(dir.path().join("ok.gguf")).unwrap();
        gguf.write_all(b"GGUF").unwrap();
        gguf.write_all(&[0u8; 32]).unwrap();
        drop(gguf);
        let entry = inspect_gguf(&dir.path().join("ok.gguf"), 32_000_000_000).unwrap();
        assert!(entry.compatible);
        assert!(entry.id.starts_with("local:gguf:"));
        let scanned = scan(
            &LocalEngineConfig {
                directories: vec![dir.path().display().to_string()],
                ..Default::default()
            },
            32_000_000_000,
        );
        assert_eq!(scanned.len(), 1);
        assert!(dir.path().join("ok.gguf").exists());
    }
}
