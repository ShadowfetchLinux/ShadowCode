//! Bounded, deterministic project inspection. Reads manifests and source
//! markers; never imports project code, runs build scripts, or calls a model.
use crate::workspace::Workspace;
use anyhow::{ensure, Result};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};
use tokio_util::sync::CancellationToken;
const SKIP: &[&str] = &[
    ".git",
    ".shadow",
    ".shadowcode",
    "node_modules",
    "target",
    "dist",
    "build",
    ".venv",
    "venv",
    "__pycache__",
    ".tox",
    ".mypy_cache",
    ".pytest_cache",
    ".next",
    "coverage",
];
const MANIFESTS: &[&str] = &[
    "package.json",
    "pyproject.toml",
    "setup.py",
    "setup.cfg",
    "pytest.ini",
    "requirements.txt",
    "Cargo.toml",
    "go.mod",
    "CMakeLists.txt",
    "meson.build",
    "Makefile",
    "Dockerfile",
    "compose.yaml",
    "docker-compose.yml",
];
pub async fn inspect(workspace: Arc<Workspace>) -> Result<Value> {
    let cancel = CancellationToken::new();
    let _guard = cancel.clone().drop_guard();
    tokio::task::spawn_blocking(move || scan(&workspace, &cancel)).await?
}
fn command_at(dir: &str, command: &str) -> String {
    if dir.is_empty() {
        command.into()
    } else {
        format!("cd '{}' && {command}", dir.replace('\'', "'\\''"))
    }
}
fn scan(workspace: &Workspace, cancel: &CancellationToken) -> Result<Value> {
    let mut builder = ignore::WalkBuilder::new(&workspace.path);
    builder
        .hidden(false)
        .follow_links(false)
        .max_depth(Some(16))
        .sort_by_file_path(|a, b| a.cmp(b));
    builder.filter_entry(|entry| {
        entry.depth() == 0 || !SKIP.contains(&entry.file_name().to_string_lossy().as_ref())
    });
    let (mut languages, mut frameworks, mut build, mut test, mut arch) = (
        BTreeSet::new(),
        BTreeSet::new(),
        BTreeSet::new(),
        BTreeSet::new(),
        BTreeSet::new(),
    );
    let mut modules: BTreeMap<String, usize> = BTreeMap::new();
    let mut manifests = Vec::new();
    let mut commands = Vec::new();
    let mut debt = Vec::new();
    let mut warnings = BTreeSet::new();
    let mut scanned = 0;
    let mut source_files = 0;
    let mut bytes_read = 0;
    let mut markers = 0;
    let mut truncated = false;
    let mut docs = false;
    for entry in builder.build() {
        ensure!(!cancel.is_cancelled(), "Project inspection cancelled");
        let entry = match entry {
            Ok(v) => v,
            Err(_) => {
                warnings.insert("Some entries could not be inspected");
                truncated = true;
                continue;
            }
        };
        scanned += 1;
        if scanned > 20_000 {
            truncated = true;
            break;
        }
        let rel = entry.path().strip_prefix(&workspace.path)?;
        let path = rel.to_string_lossy();
        if path.len() > 4096 {
            truncated = true;
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        if entry.file_type().is_some_and(|t| t.is_dir()) {
            if entry.depth() == 16 {
                truncated = true;
            }
            if path == ".github/workflows" {
                arch.insert("CI configuration");
            }
            if path == "src" {
                arch.insert("src layout");
            }
            if path == "docs" {
                docs = true;
                arch.insert("documentation directory");
            }
            continue;
        }
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        if name.to_ascii_lowercase().starts_with("readme") {
            docs = true;
        }
        if matches!(name.as_ref(), "TODO" | "TODO.md") && debt.len() < 32 {
            debt.push(format!("Open-task file: {path}"));
        }
        let extension = rel.extension().and_then(|v| v.to_str()).unwrap_or("");
        let language = match extension {
            "py" => Some("python"),
            "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => Some("javascript/typescript"),
            "rs" => Some("rust"),
            "go" => Some("go"),
            "c" | "h" | "cc" | "cpp" | "hpp" => Some("c/c++"),
            "java" => Some("java"),
            "rb" => Some("ruby"),
            "swift" => Some("swift"),
            _ => None,
        };
        if let Some(language) = language {
            languages.insert(language);
            source_files += 1;
            let module = rel
                .components()
                .next()
                .map(|v| v.as_os_str().to_string_lossy().into_owned())
                .unwrap_or_default();
            if rel.components().count() > 1 && (modules.contains_key(&module) || modules.len() < 64)
            {
                *modules.entry(module).or_default() += 1;
            } else if rel.components().count() > 1 {
                truncated = true;
            }
        }
        let manifest = MANIFESTS.contains(&name.as_ref());
        if !manifest && language.is_none() {
            continue;
        }
        if bytes_read >= 8_000_000 {
            truncated = true;
            continue;
        }
        let limit = if manifest { 128_000 } else { 64_000 };
        let (bytes, size) = match workspace.inspect(&path, limit.min(8_000_000 - bytes_read)) {
            Ok(v) => v,
            Err(_) => {
                warnings.insert("Some files were unreadable or changed during inspection");
                truncated = true;
                continue;
            }
        };
        bytes_read += bytes.len();
        if size > bytes.len() as u64 {
            truncated = true;
        }
        if language.is_some() {
            markers += String::from_utf8_lossy(&bytes)
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .filter(|word| matches!(*word, "TODO" | "FIXME" | "XXX" | "HACK"))
                .count();
            if size > 100_000 && debt.len() < 32 {
                debt.push(format!("Large source file: {path} ({size} bytes)"));
            }
        }
        if !manifest {
            continue;
        }
        if manifests.len() >= 64 {
            truncated = true;
            continue;
        }
        manifests.push(path.to_string());
        let parent = rel.parent().unwrap_or(Path::new("")).to_string_lossy();
        let mut add_command = |command: &str, runner: &str| {
            if commands.len() < 64 {
                commands.push(json!({"directory":parent,"command":command_at(&parent,command),"runner":runner,"manifest":path}));
            }
        };
        let text = String::from_utf8_lossy(&bytes).to_lowercase();
        match name.as_ref() {
            "package.json" => {
                languages.insert("javascript/typescript");
                build.insert("npm-compatible package scripts");
                match serde_json::from_slice::<Value>(&bytes) {
                    Ok(package) => {
                        for key in ["react", "vite", "next", "vue", "svelte", "vitest", "jest"] {
                            if package["dependencies"].get(key).is_some()
                                || package["devDependencies"].get(key).is_some()
                            {
                                frameworks.insert(key);
                            }
                        }
                        if package["scripts"]["test"].is_string() {
                            test.insert("package test script");
                            add_command("npm test", "npm");
                        }
                    }
                    Err(_) => {
                        warnings.insert(
                            "A package.json could not be parsed within the inspection limit",
                        );
                    }
                }
            }
            "pyproject.toml" | "setup.py" | "setup.cfg" | "requirements.txt" | "pytest.ini" => {
                languages.insert("python");
                if name != "requirements.txt" && name != "pytest.ini" {
                    build.insert("Python project metadata");
                }
                for key in ["fastapi", "django", "flask", "typer", "pytest"] {
                    if text.contains(key) {
                        frameworks.insert(key);
                    }
                }
                if name == "pyproject.toml" || name == "pytest.ini" {
                    test.insert("pytest candidate");
                    add_command("python3 -m pytest -q", "python3");
                }
            }
            "Cargo.toml" => {
                languages.insert("rust");
                build.insert("cargo");
                test.insert("cargo test");
                add_command("cargo test", "cargo");
            }
            "go.mod" => {
                languages.insert("go");
                build.insert("go");
                test.insert("go test");
                add_command("go test ./...", "go");
            }
            "CMakeLists.txt" => {
                languages.insert("c/c++");
                build.insert("cmake");
            }
            "meson.build" => {
                build.insert("meson");
            }
            "Makefile" => {
                build.insert("make");
            }
            "Dockerfile" | "compose.yaml" | "docker-compose.yml" => {
                arch.insert("container configuration");
            }
            _ => {}
        }
    }
    commands.sort_by_key(|v| v["command"].as_str().unwrap_or("").to_owned());
    commands.dedup_by(|a, b| a["command"] == b["command"]);
    if markers > 0 {
        debt.push(format!(
            "{markers} TODO/FIXME/XXX/HACK markers in inspected source prefixes"
        ));
    }
    Ok(
        json!({"workspace":workspace.path,"stack":{"languages":languages,"frameworks":frameworks,"build":build,"test":test,"arch":arch,"manifests":manifests},"modules":modules.into_iter().map(|(name,files)|json!({"name":name,"files":files})).collect::<Vec<_>>(),"debt":debt,"warnings":warnings,"test_commands":commands,"documentation_present":docs,"inspection":{"entries":scanned.min(20_000),"source_files":source_files,"bytes_read":bytes_read,"truncated":truncated,"max_depth":16,"note":"Heuristic manifest/source inspection; tests, dependencies and code correctness have not been verified."}}),
    )
}
pub fn render(map: &Value) -> String {
    let mut text = String::from("# Project map\n\n");
    for (key, label) in [
        ("languages", "Languages"),
        ("frameworks", "Frameworks"),
        ("build", "Build"),
        ("test", "Test candidates"),
        ("arch", "Layout"),
        ("manifests", "Manifests"),
    ] {
        let values = map["stack"][key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        text.push_str(&format!(
            "- {label}: {}\n",
            if values.is_empty() {
                "none detected"
            } else {
                &values
            }
        ));
    }
    text.push_str("\n## Modules\n");
    for module in map["modules"].as_array().into_iter().flatten() {
        text.push_str(&format!(
            "- {}: {} source files\n",
            module["name"].as_str().unwrap_or(""),
            module["files"]
        ));
    }
    text.push_str("\n## Inspection notes\n");
    for item in map["debt"].as_array().into_iter().flatten() {
        text.push_str(&format!("- {}\n", item.as_str().unwrap_or("")));
    }
    text.push_str(&format!(
        "\nPartial scan: {}. {}\n",
        map["inspection"]["truncated"],
        map["inspection"]["note"].as_str().unwrap_or("")
    ));
    text
}
pub fn test_command(map: &Value) -> Result<String> {
    let all = map["test_commands"].as_array().cloned().unwrap_or_default();
    let root = all
        .iter()
        .filter(|v| v["directory"] == "")
        .collect::<Vec<_>>();
    let candidates = if root.is_empty() {
        all.iter().collect::<Vec<_>>()
    } else {
        root
    };
    ensure!(candidates.len()==1,"Choose an explicit test command; {} candidates were detected. Inspect the project map's test_commands.",candidates.len());
    Ok(candidates[0]["command"].as_str().unwrap().into())
}
pub fn merge_memory(old: &str, rendered: &str) -> Result<String> {
    const START: &str = "<!-- shadowcode:project-map:start -->";
    const END: &str = "<!-- shadowcode:project-map:end -->";
    ensure!(
        old.matches(START).count() <= 1 && old.matches(END).count() <= 1,
        "Project notes contain duplicate generated-map markers; resolve them before saving"
    );
    let block = format!("{START}\n{rendered}\n{END}");
    let result = match (old.find(START), old.find(END)) {
        (None, None) => format!("{}\n\n{block}\n", old.trim_end()),
        (Some(start), Some(end)) if end > start => {
            format!("{}{block}{}", &old[..start], &old[end + END.len()..])
        }
        _ => anyhow::bail!(
            "Project notes contain an incomplete generated-map section; resolve it before saving"
        ),
    };
    ensure!(
        result.len() <= 16_000,
        "Project notes would exceed 16 KB; keep the map read-only or shorten existing notes"
    );
    Ok(result)
}
