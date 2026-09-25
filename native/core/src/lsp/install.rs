//! Optional managed installs of the npm-based language servers into a
//! ShadowCode-owned prefix (`<data>/code-intel/language-servers/<id>`).
//!
//! Runs only when the user clicks Install. Package versions are pinned,
//! install scripts are disabled (`--ignore-scripts`), and the new prefix is
//! built in a staging folder and swapped in only when npm succeeds.
use anyhow::{bail, ensure, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Package {
    pub id: &'static str,
    pub label: &'static str,
    /// npm packages with pinned versions.
    pub packages: &'static [(&'static str, &'static str)],
    /// Unpacked size reported by the npm registry, for the Install button.
    pub approx_bytes: u64,
    pub provides: &'static str,
}

pub const PACKAGES: &[Package] = &[
    Package {
        id: "typescript",
        label: "TypeScript / JavaScript",
        packages: &[
            ("typescript-language-server", "5.3.0"),
            ("typescript", "5.9.3"),
        ],
        approx_bytes: 2_335_451 + 23_625_066,
        provides: "typescript-language-server",
    },
    Package {
        id: "python",
        label: "Python (Pyright)",
        packages: &[("pyright", "1.1.414")],
        approx_bytes: 19_457_120,
        provides: "pyright-langserver",
    },
];

pub fn package(id: &str) -> Option<&'static Package> {
    PACKAGES.iter().find(|p| p.id == id)
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct InstallState {
    pub state: String,
    pub error: Option<String>,
    pub log: String,
}

static STATES: Mutex<Option<HashMap<String, InstallState>>> = Mutex::new(None);

fn set_state(id: &str, state: InstallState) {
    if let Ok(mut states) = STATES.lock() {
        states
            .get_or_insert_with(HashMap::new)
            .insert(id.to_owned(), state);
    }
}

pub fn state(id: &str) -> Option<InstallState> {
    STATES.lock().ok()?.as_ref()?.get(id).cloned()
}

pub fn npm() -> Option<PathBuf> {
    super::servers::find_program("npm")
}

fn installed_version(prefix: &Path, name: &str) -> Option<String> {
    let text = std::fs::read_to_string(prefix.join("node_modules").join(name).join("package.json"))
        .ok()?;
    serde_json::from_str::<Value>(&text).ok()?["version"]
        .as_str()
        .map(str::to_owned)
}

fn size_on_disk(path: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![path.to_owned()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(metadata) = entry.path().symlink_metadata() else {
                continue;
            };
            if metadata.is_dir() {
                stack.push(entry.path());
            } else {
                total += metadata.len();
            }
        }
    }
    total
}

/// Status of every managed package under `managed`.
pub fn status(managed: &Path) -> Value {
    json!(PACKAGES
        .iter()
        .map(|package| {
            let prefix = super::servers::managed_prefix(managed, package.id);
            let bin = prefix.join("node_modules/.bin").join(package.provides);
            let installed = bin.exists();
            let size_file = prefix.join(".shadowcode-size");
            let bytes = std::fs::read_to_string(&size_file)
                .ok()
                .and_then(|s| s.trim().parse::<u64>().ok());
            json!({
                "id": package.id,
                "label": package.label,
                "packages": package.packages.iter().map(|(n, v)| format!("{n}@{v}")).collect::<Vec<_>>(),
                "approx_bytes": package.approx_bytes,
                "installed": installed,
                "installed_bytes": bytes,
                "versions": package.packages.iter().map(|(n, _)| json!({"name": n, "version": installed_version(&prefix, n)})).collect::<Vec<_>>(),
                "path": prefix,
                "progress": state(package.id),
            })
        })
        .collect::<Vec<_>>())
}

/// Start an install in the background. Returns false if one is running.
pub fn start(managed: PathBuf, package: &'static Package) -> Result<bool> {
    let npm = npm().context(
        "npm was not found. Install Node.js (with npm) first; ShadowCode uses it to fetch the language server.",
    )?;
    if state(package.id).is_some_and(|s| s.state == "installing") {
        return Ok(false);
    }
    set_state(
        package.id,
        InstallState {
            state: "installing".into(),
            ..Default::default()
        },
    );
    tokio::spawn(async move {
        let outcome = run(&npm, &managed, package).await;
        set_state(
            package.id,
            match outcome {
                Ok(log) => InstallState {
                    state: "installed".into(),
                    error: None,
                    log,
                },
                Err(error) => InstallState {
                    state: "error".into(),
                    error: Some(format!("{error:#}")),
                    log: String::new(),
                },
            },
        );
    });
    Ok(true)
}

async fn run(npm: &Path, managed: &Path, package: &Package) -> Result<String> {
    crate::paths::private_directory(managed)?;
    let target = super::servers::managed_prefix(managed, package.id);
    let staging = managed.join(format!(".{}-staging-{}", package.id, crate::id()));
    std::fs::create_dir_all(&staging)?;
    let mut args: Vec<String> = vec![
        "install".into(),
        "--prefix".into(),
        staging.to_string_lossy().into_owned(),
        "--no-audit".into(),
        "--no-fund".into(),
        "--ignore-scripts".into(),
        "--omit=dev".into(),
        "--no-save".into(),
        "--loglevel=error".into(),
    ];
    args.extend(
        package
            .packages
            .iter()
            .map(|(name, version)| format!("{name}@{version}")),
    );
    let mut command = tokio::process::Command::new(npm);
    command
        .args(&args)
        .current_dir(&staging)
        .env_clear()
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    for (key, value) in std::env::vars_os() {
        let key_text = key.to_string_lossy();
        let keep = matches!(
            key_text.as_ref(),
            "HOME"
                | "USER"
                | "LANG"
                | "LC_ALL"
                | "TMPDIR"
                | "XDG_CONFIG_HOME"
                | "XDG_CACHE_HOME"
                | "HTTP_PROXY"
                | "HTTPS_PROXY"
                | "NO_PROXY"
                | "http_proxy"
                | "https_proxy"
                | "no_proxy"
        ) || key_text.starts_with("npm_config_")
            || key_text.starts_with("NPM_CONFIG_");
        if keep {
            command.env(&key, value);
        }
    }
    // npm is a node script: keep its folder (and node's) on PATH.
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    if let Some(dir) = npm.parent() {
        dirs.insert(0, dir.to_owned());
    }
    dirs.extend(super::servers::extra_dirs());
    command.env("PATH", std::env::join_paths(dirs)?);
    let output = tokio::time::timeout(Duration::from_secs(600), command.output())
        .await
        .context("npm did not finish within 10 minutes")?
        .context("Could not run npm")?;
    let log = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let tail = |text: &str| -> String {
        let start = text.len().saturating_sub(3000);
        let mut start = start;
        while !text.is_char_boundary(start) {
            start += 1;
        }
        text[start..].trim().to_owned()
    };
    if !output.status.success() {
        let _ = std::fs::remove_dir_all(&staging);
        bail!("npm install failed ({}):\n{}", output.status, tail(&log));
    }
    let bin = staging.join("node_modules/.bin").join(package.provides);
    if !bin.exists() {
        let _ = std::fs::remove_dir_all(&staging);
        bail!("npm finished but {} is missing", package.provides);
    }
    // Running servers from the old copy stop before it is replaced.
    super::pool().stop_all(Some(&target)).await;
    if target.exists() {
        std::fs::remove_dir_all(&target)?;
    }
    std::fs::rename(&staging, &target)?;
    // npm's .bin links are relative, so they survive the rename.
    let bytes = size_on_disk(&target);
    let _ = std::fs::write(target.join(".shadowcode-size"), bytes.to_string());
    Ok(tail(&log))
}

pub async fn remove(managed: &Path, package: &Package) -> Result<bool> {
    ensure!(
        state(package.id).is_none_or(|s| s.state != "installing"),
        "An install is still running"
    );
    let target = super::servers::managed_prefix(managed, package.id);
    super::pool().stop_all(Some(&target)).await;
    let existed = target.exists();
    if existed {
        std::fs::remove_dir_all(&target)?;
    }
    if let Ok(mut states) = STATES.lock() {
        if let Some(states) = states.as_mut() {
            states.remove(package.id);
        }
    }
    Ok(existed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packages_are_pinned() {
        for package in PACKAGES {
            for (name, version) in package.packages {
                assert!(!name.is_empty());
                assert!(
                    version.split('.').count() == 3
                        && version.chars().all(|c| c.is_ascii_digit() || c == '.'),
                    "{name}@{version} must be an exact version"
                );
            }
            assert!(package.approx_bytes > 1_000_000);
        }
        assert!(package("typescript").is_some() && package("python").is_some());
    }

    #[test]
    fn status_reports_missing_installs() {
        let dir = tempfile::tempdir().unwrap();
        let status = status(dir.path());
        assert_eq!(status[0]["installed"], false);
        assert_eq!(status[1]["packages"][0], "pyright@1.1.414");
    }
}
