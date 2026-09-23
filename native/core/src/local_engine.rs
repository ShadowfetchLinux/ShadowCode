//! Built-in local GGUF catalog and managed llama.cpp resolution.
//!
//! Users add files or directories they already have. Removing a catalog entry
//! never deletes weights. Ollama tags are not GGUF and are not imported here.
//! The engine prefers the ShadowCode-managed llama-server over PATH.
use anyhow::{bail, ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const GGUF_MAGIC: &[u8; 4] = b"GGUF";
const MAX_SCAN_FILES: usize = 256;
pub const MANAGED_RELATIVE: &str = ".local/lib/shadowcode";
pub const MANAGED_SERVER: &str = "llama-server";
pub const MANAGED_CLI: &str = "llama-cli";

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
        ensure!(
            self.directories.len() <= 32,
            "Too many local model directories"
        );
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
    let vision = has_mmproj(path);
    let tools = !["chat-only", "instruct-only"]
        .iter()
        .any(|n| lower.contains(n));
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

pub fn entry_for_id(config: &LocalEngineConfig, id: &str) -> Option<GgufEntry> {
    let id = id.trim();
    if id.is_empty() {
        return None;
    }
    scan(config, 0)
        .into_iter()
        .find(|entry| entry.id == id || entry.path == id || entry.name == id)
}

fn has_mmproj(path: &Path) -> bool {
    let Some(dir) = path.parent() else {
        return false;
    };
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    for name in [
        format!("{stem}.mmproj.gguf"),
        format!("{stem}-mmproj.gguf"),
        format!("mmproj-{stem}.gguf"),
        format!("{stem}.mmproj"),
    ] {
        if dir.join(name).is_file() {
            return true;
        }
    }
    let Ok(read) = fs::read_dir(dir) else {
        return false;
    };
    read.flatten().any(|child| {
        let name = child.file_name().to_string_lossy().to_ascii_lowercase();
        name.contains("mmproj") && name.ends_with(".gguf")
    })
}

pub fn managed_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(MANAGED_RELATIVE))
}

fn usable_file(path: PathBuf) -> Option<PathBuf> {
    path.is_file().then_some(path)
}

/// Prefer the managed ShadowCode runtime over a PATH-only llama.cpp.
///
/// Order: configured file, `SHADOWCODE_LLAMA_SERVER`,
/// `~/.local/lib/shadowcode/llama-server` (then llama-cli), next to the
/// current executable, then PATH.
pub fn resolve_llama_binary(configured: &str) -> Option<PathBuf> {
    resolve_llama_binary_with(
        configured,
        std::env::var_os("SHADOWCODE_LLAMA_SERVER"),
        managed_dir(),
        std::env::var_os("PATH"),
    )
}

fn resolve_from_path_env(name: &str, path_env: Option<std::ffi::OsString>) -> Option<PathBuf> {
    let path = path_env?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

pub(crate) fn resolve_llama_binary_with(
    configured: &str,
    env_server: Option<impl AsRef<std::ffi::OsStr>>,
    managed: Option<PathBuf>,
    path_env: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    let configured = configured.trim();
    if !configured.is_empty() {
        if let Some(path) = usable_file(PathBuf::from(configured)) {
            return Some(path);
        }
    }
    if let Some(env) = env_server {
        if let Some(path) = usable_file(PathBuf::from(env.as_ref())) {
            return Some(path);
        }
    }
    if let Some(dir) = managed {
        if let Some(path) = usable_file(dir.join(MANAGED_SERVER)) {
            return Some(path);
        }
        if let Some(path) = usable_file(dir.join(MANAGED_CLI)) {
            return Some(path);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if let Some(path) = usable_file(dir.join(MANAGED_SERVER)) {
                return Some(path);
            }
            if let Some(path) = usable_file(dir.join("../lib/shadowcode").join(MANAGED_SERVER)) {
                return Some(path);
            }
        }
    }
    resolve_from_path_env(MANAGED_SERVER, path_env.clone())
        .or_else(|| resolve_from_path_env(MANAGED_CLI, path_env))
}

fn binary_origin(path: &Path) -> &'static str {
    if let Some(dir) = managed_dir() {
        if path.starts_with(dir) {
            return "managed";
        }
    }
    "other"
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
            let kb = rest
                .split_whitespace()
                .next()
                .and_then(|n| n.parse::<u64>().ok());
            return kb.unwrap_or(0).saturating_mul(1024);
        }
    }
    0
}

fn read_nvidia() -> (Option<String>, Option<u64>) {
    let Some(output) = std::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,memory.total",
            "--format=csv,noheader,nounits",
        ])
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
    match resolve_llama_binary(configured) {
        Some(path) => {
            let origin = binary_origin(&path);
            json!({
                "state": "ready",
                "path": path,
                "origin": origin,
                "detail": if origin == "managed" {
                    "Managed llama.cpp is installed. Local GGUF rows load through ShadowCode, not an Ollama or LM Studio daemon."
                } else {
                    "llama.cpp binary is present; local GGUF rows can load after you pick a file"
                },
            })
        }
        None => json!({
            "state": "setup_required",
            "path": null,
            "origin": null,
            "detail": "Managed llama.cpp is not installed yet. Build it with scripts/build-llama.cpp.sh (no sudo). Models are never auto-downloaded.",
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
        assert!(!scanned[0].vision);
        assert!(dir.path().join("ok.gguf").exists());
        let vision_path = dir.path().join("llava.gguf");
        fs::copy(dir.path().join("ok.gguf"), &vision_path).unwrap();
        assert!(!inspect_gguf(&vision_path, 32_000_000_000).unwrap().vision);
        fs::write(dir.path().join("llava.mmproj.gguf"), b"GGUF").unwrap();
        assert!(inspect_gguf(&vision_path, 32_000_000_000).unwrap().vision);
    }

    #[test]
    fn resolves_managed_binary_instead_of_path() {
        let home = tempfile::tempdir().unwrap();
        let path_dir = tempfile::tempdir().unwrap();
        let managed = home.path().join(MANAGED_RELATIVE);
        fs::create_dir_all(&managed).unwrap();
        fs::write(managed.join(MANAGED_SERVER), b"managed").unwrap();
        fs::write(path_dir.path().join(MANAGED_SERVER), b"path").unwrap();
        let resolved = resolve_llama_binary_with(
            "",
            None::<&str>,
            Some(managed.clone()),
            Some(path_dir.path().as_os_str().to_os_string()),
        )
        .expect("managed binary");
        assert_eq!(resolved, managed.join(MANAGED_SERVER));
        assert!(resolved.starts_with(&managed));
        let configured = path_dir.path().join("explicit");
        fs::write(&configured, b"configured").unwrap();
        assert_eq!(
            resolve_llama_binary_with(
                &configured.display().to_string(),
                None::<&str>,
                Some(managed.clone()),
                Some(path_dir.path().as_os_str().to_os_string()),
            ),
            Some(configured)
        );
        let from_path = resolve_llama_binary_with(
            "",
            None::<&str>,
            None,
            Some(path_dir.path().as_os_str().to_os_string()),
        );
        assert_eq!(from_path, Some(path_dir.path().join(MANAGED_SERVER)));
    }
}
