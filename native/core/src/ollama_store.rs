//! Read-only access to an existing Ollama model store.
//!
//! Ollama keeps GGUF weights as content-addressed blobs
//! (`blobs/sha256-<hex>`) described by manifests
//! (`manifests/<host>/<namespace>/<repo>/<tag>`). ShadowCode reads the
//! manifests, resolves the model and projector blobs, and registers those
//! paths in its own catalog. Nothing is copied, and nothing is ever written
//! into the store. The Ollama daemon is not needed.
use anyhow::{bail, ensure, Context, Result};
use serde_json::Value;
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

const MAX_MANIFESTS: usize = 512;
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MODEL_MEDIA: &str = "application/vnd.ollama.image.model";
const PROJECTOR_MEDIA: &str = "application/vnd.ollama.image.projector";
const DEFAULT_HOST: &str = "registry.ollama.ai";

#[derive(Clone, Debug, PartialEq)]
pub struct StoreRoot {
    pub path: PathBuf,
    /// `"env"`, `"systemd"`, or `"default"`.
    pub origin: &'static str,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OllamaModel {
    /// `qwen3:14b`, `huihui_ai/gemma-4-abliterated:12b`, or `host/ns/repo:tag`.
    pub tag: String,
    pub model: PathBuf,
    pub projector: Option<PathBuf>,
    pub bytes: u64,
    /// Why the manifest cannot be used as-is (missing blob, bad digest).
    pub problem: Option<String>,
}

/// `OLLAMA_MODELS`, then `Environment=OLLAMA_MODELS=` in the user's systemd
/// Ollama units, then `~/.ollama/models`. The first root that has a
/// `manifests` directory wins; otherwise the first candidate is reported.
pub fn discover() -> Option<StoreRoot> {
    discover_with(
        std::env::var_os("OLLAMA_MODELS"),
        std::env::var_os("HOME").map(PathBuf::from),
    )
}

pub fn discover_with(env: Option<OsString>, home: Option<PathBuf>) -> Option<StoreRoot> {
    let mut candidates: Vec<StoreRoot> = Vec::new();
    if let Some(env) = env.filter(|v| !v.is_empty()) {
        let path = PathBuf::from(env);
        if path.is_absolute() {
            candidates.push(StoreRoot {
                path,
                origin: "env",
            });
        }
    }
    if let Some(home) = &home {
        for path in systemd_model_dirs(home) {
            candidates.push(StoreRoot {
                path,
                origin: "systemd",
            });
        }
        candidates.push(StoreRoot {
            path: home.join(".ollama/models"),
            origin: "default",
        });
    }
    let with_models = candidates.iter().find(|c| has_manifests(&c.path)).cloned();
    with_models.or_else(|| candidates.into_iter().next())
}

fn has_manifests(root: &Path) -> bool {
    !manifest_files(root).is_empty()
}

/// `Environment=OLLAMA_MODELS=...` lines in `~/.config/systemd/user/*ollama*`
/// units and their drop-ins. `%h` expands to the home directory.
fn systemd_model_dirs(home: &Path) -> Vec<PathBuf> {
    let unit_dir = home.join(".config/systemd/user");
    let Ok(read) = fs::read_dir(&unit_dir) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    let mut entries: Vec<PathBuf> = read.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_owned();
        if !name.contains("ollama") {
            continue;
        }
        if path.is_file() && name.ends_with(".service") {
            files.push(path);
        } else if path.is_dir() && name.ends_with(".service.d") {
            if let Ok(inner) = fs::read_dir(&path) {
                let mut dropins: Vec<PathBuf> = inner
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("conf"))
                    .collect();
                dropins.sort();
                files.extend(dropins);
            }
        }
    }
    let mut out = Vec::new();
    for file in files {
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        for value in environment_values(&text, "OLLAMA_MODELS") {
            let expanded = value.replace("%h", &home.display().to_string());
            let path = PathBuf::from(expanded);
            if path.is_absolute() && !out.contains(&path) {
                out.push(path);
            }
        }
    }
    out
}

/// Values of `name` in `Environment=` lines (quoted or bare assignments).
pub fn environment_values(unit: &str, name: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in unit.lines() {
        let Some(rest) = line.trim().strip_prefix("Environment=") else {
            continue;
        };
        for token in split_assignments(rest) {
            if let Some(value) = token.strip_prefix(&format!("{name}=")) {
                out.push(value.to_owned());
            }
        }
    }
    out
}

fn split_assignments(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for c in text.chars() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, c) if c.is_whitespace() => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn manifest_files(root: &Path) -> Vec<(PathBuf, [String; 4])> {
    let base = root.join("manifests");
    let mut out = Vec::new();
    let Ok(hosts) = fs::read_dir(&base) else {
        return out;
    };
    let dirs = |p: &Path| -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = fs::read_dir(p)
            .map(|r| r.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        v.sort();
        v
    };
    let mut hosts: Vec<PathBuf> = hosts.flatten().map(|e| e.path()).collect();
    hosts.sort();
    for host in hosts.iter().filter(|p| p.is_dir()) {
        for ns in dirs(host).iter().filter(|p| p.is_dir()) {
            for repo in dirs(ns).iter().filter(|p| p.is_dir()) {
                for tag in dirs(repo).iter().filter(|p| p.is_file()) {
                    if out.len() >= MAX_MANIFESTS {
                        return out;
                    }
                    let name = |p: &Path| {
                        p.file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("")
                            .to_owned()
                    };
                    out.push((tag.clone(), [name(host), name(ns), name(repo), name(tag)]));
                }
            }
        }
    }
    out
}

pub fn tag_name(parts: &[String; 4]) -> String {
    let [host, ns, repo, tag] = parts;
    if host == DEFAULT_HOST && ns == "library" {
        format!("{repo}:{tag}")
    } else if host == DEFAULT_HOST {
        format!("{ns}/{repo}:{tag}")
    } else {
        format!("{host}/{ns}/{repo}:{tag}")
    }
}

/// `sha256:<64 hex>` → `blobs/sha256-<hex>`. Anything else is rejected so a
/// manifest cannot point outside the store.
pub fn blob_path(root: &Path, digest: &str) -> Result<PathBuf> {
    let hex = digest
        .strip_prefix("sha256:")
        .context("Unsupported blob digest")?;
    ensure!(
        hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid blob digest"
    );
    Ok(root.join("blobs").join(format!("sha256-{hex}")))
}

fn parse_manifest(root: &Path, path: &Path, parts: &[String; 4]) -> Result<OllamaModel> {
    let meta = fs::metadata(path)?;
    ensure!(meta.len() <= MAX_MANIFEST_BYTES, "Manifest is too large");
    let manifest: Value = serde_json::from_slice(&fs::read(path)?)
        .with_context(|| format!("Invalid manifest {}", path.display()))?;
    let layers = manifest["layers"]
        .as_array()
        .context("Manifest has no layers")?;
    let find = |media: &str| {
        layers
            .iter()
            .find(|l| l["mediaType"] == media)
            .and_then(|l| l["digest"].as_str())
            .map(str::to_owned)
    };
    let model_digest = find(MODEL_MEDIA).context("Manifest has no model layer")?;
    let model = blob_path(root, &model_digest)?;
    let projector = find(PROJECTOR_MEDIA)
        .map(|d| blob_path(root, &d))
        .transpose()?;
    let mut problem = None;
    if !model.is_file() {
        problem = Some(format!("Model blob is missing: {}", model.display()));
    } else if projector.as_ref().is_some_and(|p| !p.is_file()) {
        problem = Some("Vision projector blob is missing; text only".into());
    }
    let bytes = fs::metadata(&model).map(|m| m.len()).unwrap_or(0);
    Ok(OllamaModel {
        tag: tag_name(parts),
        model,
        projector,
        bytes,
        problem,
    })
}

/// Every manifest with a model layer. Unreadable manifests are skipped.
pub fn list(root: &Path) -> Vec<OllamaModel> {
    manifest_files(root)
        .into_iter()
        .filter_map(|(path, parts)| parse_manifest(root, &path, &parts).ok())
        .collect()
}

/// Resolve one tag (`qwen3:14b`, `library/qwen3:14b`, or the full form).
pub fn find(root: &Path, tag: &str) -> Result<OllamaModel> {
    let tag = tag.trim();
    ensure!(
        !tag.is_empty() && tag.len() <= 512 && !tag.contains(['\n', '\0']),
        "Missing Ollama tag"
    );
    let normalized = tag.strip_prefix("library/").unwrap_or(tag);
    for (path, parts) in manifest_files(root) {
        if tag_name(&parts) == normalized {
            return parse_manifest(root, &path, &parts);
        }
    }
    bail!("{tag} is not in the Ollama store at {}", root.display())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn systemd_units_and_tags() {
        let unit = "[Service]\nEnvironment=OLLAMA_HOST=0.0.0.0:11434\nEnvironment=OLLAMA_MODELS=%h/models/ollama\nEnvironment=\"OLLAMA_MODELS=/srv/o m\" OTHER=1\n";
        assert_eq!(
            environment_values(unit, "OLLAMA_MODELS"),
            vec!["%h/models/ollama".to_owned(), "/srv/o m".to_owned()]
        );
        let home = tempfile::tempdir().unwrap();
        let units = home.path().join(".config/systemd/user");
        fs::create_dir_all(units.join("ollama.service.d")).unwrap();
        fs::write(units.join("ollama.service"), unit).unwrap();
        fs::write(
            units.join("unrelated.service"),
            "Environment=OLLAMA_MODELS=/x\n",
        )
        .unwrap();
        let store = home.path().join("models/ollama");
        fs::create_dir_all(store.join("manifests/registry.ollama.ai/library/qwen3")).unwrap();
        fs::write(
            store.join("manifests/registry.ollama.ai/library/qwen3/14b"),
            "{}",
        )
        .unwrap();
        let found = discover_with(None, Some(home.path().to_path_buf())).unwrap();
        assert_eq!(found.path, store);
        assert_eq!(found.origin, "systemd");
        let parts = [
            DEFAULT_HOST.to_owned(),
            "huihui_ai".to_owned(),
            "gemma-4-abliterated".to_owned(),
            "12b".to_owned(),
        ];
        assert_eq!(tag_name(&parts), "huihui_ai/gemma-4-abliterated:12b");
        assert!(blob_path(&store, "sha256:../../etc").is_err());
    }
}
