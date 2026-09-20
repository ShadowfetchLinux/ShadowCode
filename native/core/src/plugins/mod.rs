//! Declarative project bundles. Installation never executes a package or grants
//! its hooks/MCP access. A private journal owns only exact generated file bytes.
use crate::{
    config::Config,
    paths::AppPaths,
    workspace::{hash, Workspace},
};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Component, Path},
};
mod builtin;

pub const MAX_BYTES: usize = 256_000;
const MAX_INSTALLED: usize = 32;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bundle {
    pub format: String,
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub skills: BTreeMap<String, Workflow>,
    #[serde(default)]
    pub commands: BTreeMap<String, Workflow>,
    #[serde(default)]
    pub hooks: Vec<crate::hooks::Definition>,
    #[serde(default)]
    pub mcp_servers: Vec<crate::mcp::registry::Definition>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workflow {
    pub description: String,
    pub content: String,
    #[serde(default)]
    pub mode: String,
    /// Supporting text relative to this skill's directory; never executable.
    #[serde(default)]
    pub files: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct File {
    pub path: String,
    pub kind: String,
    pub content: String,
    pub hash: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Install {
    workspace: String,
    state: String,
    bundle: Bundle,
}
fn name_valid(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}
fn joined(plugin: &str, name: &str) -> Result<String> {
    ensure!(
        name_valid(name),
        "Component names use 1–32 lowercase letters, numbers or hyphens"
    );
    Ok(format!("{plugin}--{name}"))
}
impl Bundle {
    pub fn parse(value: Value) -> Result<Self> {
        ensure!(
            value.to_string().len() <= MAX_BYTES,
            "Plugin bundle exceeds 256 KB"
        );
        let bundle: Self = serde_json::from_value(value).context("Invalid native plugin bundle")?;
        bundle.files()?;
        Ok(bundle)
    }
    pub fn hash(&self) -> Result<String> {
        Ok(hash(&serde_json::to_vec(self)?))
    }
    pub fn files(&self) -> Result<Vec<File>> {
        ensure!(
            self.format == "shadowcode-plugin-v1",
            "Expected shadowcode-plugin-v1 bundle format"
        );
        ensure!(
            name_valid(&self.name),
            "Plugin names use 1–32 lowercase letters, numbers or hyphens"
        );
        ensure!(
            !self.version.is_empty()
                && self.version.len() <= 64
                && self
                    .version
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".-+_".contains(&b)),
            "Invalid plugin version"
        );
        ensure!(
            !self.description.trim().is_empty() && self.description.len() <= 1000,
            "Plugin description must contain 1–1000 bytes"
        );
        ensure!(
            serde_json::to_vec(self)?.len() <= MAX_BYTES,
            "Plugin bundle exceeds 256 KB"
        );
        let mut files = BTreeMap::new();
        ensure!(
            !self
                .skills
                .keys()
                .any(|name| self.commands.contains_key(name)),
            "Skills and commands in a plugin must have distinct names"
        );
        let mut add = |path: String, kind: &str, content: String| -> Result<()> {
            ensure!(
                !content.contains('\0'),
                "Plugin files must be text without NUL bytes"
            );
            let file = File {
                hash: hash(content.as_bytes()),
                path: path.clone(),
                kind: kind.into(),
                content,
            };
            ensure!(files.insert(path, file).is_none(), "Duplicate plugin file");
            Ok(())
        };
        for (kind, definitions) in [("skill", &self.skills), ("command", &self.commands)] {
            for (name, workflow) in definitions {
                let name = joined(&self.name, name)?;
                let path = if kind == "skill" {
                    format!(".shadowcode/skills/{name}/SKILL.md")
                } else {
                    format!(".shadow/commands/{name}.md")
                };
                let header =
                    json!({"name":name,"description":workflow.description,"mode":workflow.mode});
                // JSON is YAML; quoting keeps user text from creating metadata.
                let content = format!("---\n{}\n---\n{}\n", header, workflow.content);
                crate::workflows::Definition::parse(&path, kind, &content, "")?;
                add(path, kind, content)?;
                ensure!(
                    kind == "skill" || workflow.files.is_empty(),
                    "Only skills can contain supporting files"
                );
                for (relative, content) in &workflow.files {
                    ensure!(
                        !relative.is_empty()
                            && relative.len() <= 180
                            && !relative.contains(['\\', '\0'])
                            && Path::new(relative)
                                .components()
                                .all(|c| matches!(c, Component::Normal(_)))
                            && !relative.split('/').any(|p| p.is_empty()
                                || matches!(p, ".git" | "SKILL.md")
                                || p.starts_with('.')),
                        "Invalid skill supporting-file path: {relative}"
                    );
                    ensure!(content.len() <= 64_000, "Supporting file exceeds 64 KB");
                    add(
                        format!(".shadowcode/skills/{name}/{relative}"),
                        "support",
                        content.clone(),
                    )?;
                }
            }
        }
        for hook in &self.hooks {
            let mut hook = hook.clone();
            hook.name = joined(&self.name, &hook.name)?;
            let content = serde_json::to_string_pretty(&hook)?;
            crate::hooks::Definition::parse(&content)?;
            add(
                format!(".shadowcode/hooks/{}.json", hook.name),
                "hook",
                content,
            )?;
        }
        for server in &self.mcp_servers {
            let mut server = server.clone();
            server.name = joined(&self.name, &server.name)?;
            ensure!(
                server.env.is_empty(),
                "Plugin MCP servers use env_refs, not literal environment values"
            );
            server.validate()?;
            add(
                format!(".shadowcode/mcp/{}.json", server.name),
                "mcp",
                serde_json::to_string_pretty(&server)?,
            )?;
        }
        ensure!(
            !files.is_empty() && files.len() <= 64,
            "A plugin must contain 1–64 files"
        );
        ensure!(
            files.values().map(|f| f.content.len()).sum::<usize>() <= MAX_BYTES,
            "Expanded plugin exceeds 256 KB"
        );
        for path in files.keys() {
            ensure!(
                !files
                    .keys()
                    .any(|other| other.starts_with(&format!("{path}/"))),
                "Plugin file conflicts with a directory: {path}"
            );
        }
        Ok(files.into_values().collect())
    }
}
pub fn resolve(body: &Value) -> Result<Bundle> {
    if let Some(value) = body.get("bundle").filter(|v| !v.is_null()) {
        ensure!(
            body["name"].as_str().is_none_or(str::is_empty),
            "Choose a built-in name or a custom bundle"
        );
        Bundle::parse(value.clone())
    } else {
        builtin::all()
            .into_iter()
            .find(|b| Some(b.name.as_str()) == body["name"].as_str())
            .context("Unknown built-in plugin")
    }
}
pub fn preview(bundle: &Bundle) -> Result<Value> {
    Ok(json!({"bundle":bundle,"hash":bundle.hash()?,"files":bundle.files()?}))
}
fn directory(workspace: &Workspace) -> String {
    format!(
        "native-plugins/{}",
        hash(workspace.path.as_os_str().as_encoded_bytes())
    )
}
fn record_path(workspace: &Workspace, name: &str) -> Result<String> {
    ensure!(name_valid(name), "Invalid plugin name");
    Ok(format!("{}/{name}.json", directory(workspace)))
}
fn records(private: &Workspace, workspace: &Workspace) -> Result<Vec<crate::workspace::FileEntry>> {
    let entries = match private.list(&directory(workspace)) {
        Ok(entries) => entries,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(vec![])
        }
        Err(error) => return Err(error),
    };
    let entries: Vec<_> = entries
        .into_iter()
        .filter(|e| e.name.ends_with(".json"))
        .collect();
    ensure!(
        entries.len() <= MAX_INSTALLED,
        "Plugin inventory exceeds 32 entries"
    );
    Ok(entries)
}
fn read_install(
    private: &Workspace,
    workspace: &Workspace,
    name: &str,
) -> Result<(Install, String)> {
    let file = private.read(&record_path(workspace, name)?)?;
    ensure!(
        file.bytes <= MAX_BYTES + 8192,
        "Plugin inventory record too large"
    );
    let install: Install =
        serde_json::from_str(&file.content).context("Invalid plugin inventory")?;
    ensure!(
        install.workspace == workspace.path.to_string_lossy()
            && install.bundle.name == name
            && matches!(
                install.state.as_str(),
                "prepared" | "installed" | "removing"
            ),
        "Plugin inventory does not match this project"
    );
    install.bundle.files()?;
    Ok((install, file.hash))
}
fn brief(bundle: &Bundle) -> Result<Value> {
    Ok(
        json!({"name":bundle.name,"version":bundle.version,"description":bundle.description,"hash":bundle.hash()?,"files":bundle.files()?.iter().map(|f| json!({"path":f.path,"kind":f.kind,"hash":f.hash})).collect::<Vec<_>>()}),
    )
}
pub fn catalog(paths: &AppPaths, workspace: &Workspace) -> Result<Value> {
    let private = Workspace::open(&paths.state)?;
    let mut installed = Vec::new();
    let mut issues = Vec::new();
    for file in records(&private, workspace)? {
        let name = file.name.trim_end_matches(".json");
        match read_install(&private, workspace, name) {
            Ok((record, record_hash)) => {
                let mut row = brief(&record.bundle)?;
                row["hash"] = json!(record_hash);
                row["state"] = json!(record.state);
                row["files"] = json!(record.bundle.files()?.into_iter().map(|file| {
                    let (status,error) = match workspace.snapshot(&file.path) {
                        Ok(snapshot) => (if snapshot.hash.as_deref() == Some(&file.hash) {"unchanged"} else if snapshot.bytes.is_none() {"missing"} else {"modified"},None),
                        Err(error) => ("unreadable",Some(error.to_string())),
                    };
                    json!({"path":file.path,"kind":file.kind,"hash":file.hash,"status":status,"error":error})
                }).collect::<Vec<_>>());
                installed.push(row);
            }
            Err(error) => issues.push(format!("{name}: {error:#}")),
        }
    }
    // Legacy bundles are retained verbatim. They are not imported as executable
    // Python callbacks or treated as a successful native installation.
    let legacy = Workspace::open(&paths.config)?
        .list("shadowcode/plugins")
        .unwrap_or_default()
        .into_iter()
        .filter(|f| f.kind == "dir")
        .take(33)
        .map(|f| f.name)
        .collect::<Vec<_>>();
    let config = Config::load(paths, Some(&workspace.path))?;
    Ok(
        json!({"format":"native-plugins-v1","workspace":workspace.path,"trusted":config.is_trusted(&workspace.path),"read_only":config.permissions.level == crate::config::PermissionLevel::ReadOnly,"installed":installed,"available":builtin::all().iter().map(brief).collect::<Result<Vec<_>>>()?,"issues":issues,"legacy":legacy}),
    )
}
fn revoke(paths: &AppPaths, workspace: &Workspace, files: &[File]) -> Result<()> {
    Config::update(paths, |config| {
        for file in files {
            if file.kind == "hook" {
                crate::hooks::activate(workspace, config, &file.path, "", false)?;
            }
            if file.kind == "mcp" {
                crate::mcp::registry::activate(
                    workspace,
                    config,
                    &format!("project:{}", file.path),
                    "",
                    false,
                )?;
            }
        }
        Ok(())
    })?;
    Ok(())
}
/// The service holds a trusted mutable-workspace reservation throughout.
pub fn install(
    paths: &AppPaths,
    workspace: &Workspace,
    bundle: Bundle,
    expected: &str,
) -> Result<Value> {
    ensure!(
        bundle.hash()? == expected,
        "Plugin changed; preview its contents before installing"
    );
    let files = bundle.files()?;
    let private = Workspace::open(&paths.state)?;
    let record_path = record_path(workspace, &bundle.name)?;
    ensure!(
        private.snapshot(&record_path)?.bytes.is_none(),
        "Plugin is already installed or has an interrupted installation; remove it first"
    );
    ensure!(
        records(&private, workspace)?.len() < MAX_INSTALLED,
        "At most 32 plugins may be installed per project"
    );
    for file in &files {
        workspace.writable(&file.path)?;
        ensure!(
            workspace.snapshot(&file.path)?.bytes.is_none(),
            "Plugin file already exists: {}. Move or preserve it before installation",
            file.path
        );
    }
    let workflows = crate::workflows::discover(workspace);
    let new_workflows: Vec<_> = files
        .iter()
        .filter(|file| matches!(file.kind.as_str(), "skill" | "command"))
        .collect();
    ensure!(
        workflows.definitions.len() + new_workflows.len() <= 256
            && !workflows
                .issues
                .iter()
                .any(|issue| issue.contains("256 project") || issue.contains("catalog exceeds")),
        "Project workflow catalog is full"
    );
    ensure!(
        workflows
            .definitions
            .iter()
            .map(|d| d.raw_content.len())
            .sum::<usize>()
            + new_workflows.iter().map(|f| f.content.len()).sum::<usize>()
            <= 2_000_000,
        "Project workflows would exceed 2 MB"
    );
    for file in new_workflows {
        let definition =
            crate::workflows::Definition::parse(&file.path, &file.kind, &file.content, &file.hash)?;
        ensure!(
            !workflows
                .definitions
                .iter()
                .any(|d| d.info.name == definition.info.name || d.alias == definition.info.name),
            "Plugin workflow name already exists: {}",
            definition.info.name
        );
    }
    for (kind, dirs) in [
        ("hook", [".shadowcode/hooks", ".shadow/hooks"]),
        ("mcp", [".shadowcode/mcp", ".shadow/mcp"]),
    ] {
        let mut count = 0;
        for dir in dirs {
            match workspace.list(dir) {
                Ok(entries) => count += entries.iter().filter(|entry| entry.kind == "file").count(),
                Err(error)
                    if error
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) => {}
                Err(error) => return Err(error),
            }
        }
        if kind == "mcp" {
            count += Config::load(paths, None)?.mcp["servers"]
                .as_array()
                .map_or(0, Vec::len);
        }
        let added = files.iter().filter(|file| file.kind == kind).count();
        ensure!(
            added == 0 || count + added <= 64,
            "Project {kind} catalog would exceed 64 definitions"
        );
    }
    // Persist recovery information before the first project write. Revoke stale
    // grants before creating byte-identical definitions at previously used paths.
    revoke(paths, workspace, &files)?;
    let mut record = Install {
        workspace: workspace.path.to_string_lossy().into(),
        state: "prepared".into(),
        bundle,
    };
    let journal_hash =
        private.write(&record_path, &serde_json::to_vec(&record)?, Some("missing"))?;
    for file in &files {
        // A failure leaves the journal visible for explicit recovery/removal.
        workspace.write(&file.path,file.content.as_bytes(),Some("missing"))
            .with_context(||format!("Plugin installation interrupted at {}; remove the incomplete installation before retrying",file.path))?;
    }
    record.state = "installed".into();
    private.write(
        &record_path,
        &serde_json::to_vec(&record)?,
        Some(&journal_hash),
    )?;
    Ok(
        json!({"name":record.bundle.name,"installed":files.iter().map(|f|&f.path).collect::<Vec<_>>()}),
    )
}
pub fn remove(
    paths: &AppPaths,
    workspace: &Workspace,
    name: &str,
    expected: &str,
) -> Result<Value> {
    let private = Workspace::open(&paths.state)?;
    let (mut record, record_hash) = read_install(&private, workspace, name)?;
    ensure!(
        record_hash == expected,
        "Plugin installation changed; refresh before removing"
    );
    let files = record.bundle.files()?;
    revoke(paths, workspace, &files)?;
    record.state = "removing".into();
    let record_path = record_path(workspace, name)?;
    let journal_hash = private.write(
        &record_path,
        &serde_json::to_vec(&record)?,
        Some(&record_hash),
    )?;
    let mut directories = std::collections::BTreeSet::new();
    for file in &files {
        if matches!(file.kind.as_str(), "skill" | "support") {
            let mut dir = Path::new(&file.path).parent();
            while let Some(path) = dir {
                if path == Path::new(".shadowcode/skills") {
                    break;
                }
                directories.insert(path.to_string_lossy().into_owned());
                dir = path.parent();
            }
        }
    }
    let mut removed = Vec::new();
    let mut retained = Vec::new();
    for file in files {
        match workspace.snapshot(&file.path) {
            Ok(snapshot) if snapshot.hash.as_deref() == Some(&file.hash) => {
                match workspace.delete(&file.path, Some(&file.hash)) {
                    Ok(()) => removed.push(file.path),
                    Err(error) => {
                        retained.push(json!({"path":file.path,"reason":error.to_string()}))
                    }
                }
            }
            Ok(snapshot) if snapshot.bytes.is_none() => {}
            Ok(_) => {
                retained.push(json!({"path":file.path,"reason":"Modified since installation"}))
            }
            Err(error) => retained.push(json!({"path":file.path,"reason":error.to_string()})),
        }
    }
    // Only empty component directories are eligible; user files and symlinks
    // remain untouched. Descendants sort after their parents.
    for directory in directories.into_iter().rev() {
        let _ = workspace.remove_empty_dir(&directory);
    }
    private.delete(&record_path, Some(&journal_hash))?;
    Ok(json!({"name":name,"removed":removed,"retained":retained}))
}
