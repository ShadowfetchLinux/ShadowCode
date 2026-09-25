//! Explicit project workflows. Discovery only reads confined text; it never
//! executes commands, expands shell substitutions, or grants permissions.
use crate::workspace::Workspace;
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkflowInfo {
    pub name: String,
    pub kind: String,
    pub source: String,
    pub path: String,
    pub hash: String,
    pub mode: String,
}
#[derive(Clone, Debug)]
pub struct Guidance {
    pub info: WorkflowInfo,
    pub instructions: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct Definition {
    #[serde(flatten)]
    pub info: WorkflowInfo,
    pub description: String,
    pub alias: String,
    pub content: String,
    pub raw_content: String,
    /// The model may load this skill on its own (`load_skill`). False when
    /// the file sets `disable-model-invocation: true`.
    pub model_invocable: bool,
    /// `argument-hint` from Claude Code commands, shown in the slash menu.
    pub arg_hint: String,
}
/// Directories read for Claude Code compatibility. Fields that only Claude
/// Code understands (model, allowed-tools, …) are ignored there instead of
/// rejected; ShadowCode's own settings still decide models and permissions.
fn compat_path(path: &str) -> bool {
    path.starts_with(".claude/")
}
#[derive(Default)]
pub struct Catalog {
    pub definitions: Vec<Definition>,
    pub issues: Vec<String>,
}
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 80
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}
impl Definition {
    pub fn parse(path: &str, kind: &str, text: &str, hash: &str) -> Result<Self> {
        ensure!(text.len() <= 64000, "Workflow exceeds 64 KB");
        let normalized = text.replace("\r\n", "\n");
        let (metadata, content) = if let Some(rest) = normalized.strip_prefix("---\n") {
            let (header, body) = rest
                .split_once("\n---")
                .context("Unclosed workflow front matter")?;
            ensure!(
                body.is_empty() || body.starts_with('\n'),
                "Front matter must end on its own line"
            );
            let metadata: Value =
                serde_yaml_ng::from_str(header).context("Invalid workflow front matter")?;
            ensure!(
                metadata.is_object(),
                "Workflow front matter must be an object"
            );
            (metadata, body.trim().to_owned())
        } else {
            (json!({}), normalized.trim().to_owned())
        };
        let file = std::path::Path::new(path);
        let fallback = if file.file_name().is_some_and(|name| name == "SKILL.md") {
            file.parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
        } else {
            file.file_stem().and_then(|s| s.to_str())
        }
        .context("Workflow name missing")?;
        for field in ["name", "alias", "mode", "description"] {
            ensure!(
                metadata.get(field).is_none_or(Value::is_string),
                "Workflow field '{field}' must be text"
            );
        }
        for field in ["user-invocable", "disable-model-invocation"] {
            ensure!(
                metadata.get(field).is_none_or(Value::is_boolean),
                "Workflow field '{field}' must be true or false"
            );
        }
        let name = metadata["name"].as_str().unwrap_or(fallback);
        let alias = metadata["alias"].as_str().unwrap_or("");
        ensure!(
            valid_name(name),
            "Workflow name must use 1–80 letters, numbers, - or _"
        );
        ensure!(
            alias.is_empty() || valid_name(alias),
            "Invalid workflow alias"
        );
        let mode = metadata["mode"].as_str().unwrap_or("");
        ensure!(
            matches!(mode, "" | "code" | "plan" | "review" | "test"),
            "Workflow mode must be code, plan, review, or test"
        );
        if !compat_path(path) {
            for field in ["model", "hooks", "allowed-tools", "permission-mode"] {
                ensure!(metadata.get(field).is_none(),"Workflow field '{field}' is not supported; configure permissions and models in Settings");
            }
        }
        let arg_hint = match &metadata["argument-hint"] {
            Value::String(text) => text.trim().chars().take(200).collect(),
            Value::Array(items) => items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(200)
                .collect(),
            _ => String::new(),
        };
        ensure!(
            metadata["user-invocable"] != false,
            "This workflow disables explicit invocation"
        );
        ensure!(!content.is_empty(), "Workflow body is empty");
        let description = metadata["description"]
            .as_str()
            .unwrap_or("Project workflow");
        ensure!(
            description.len() <= 1000,
            "Workflow description exceeds 1000 bytes"
        );
        Ok(Self {
            info: WorkflowInfo {
                name: name.into(),
                kind: kind.into(),
                source: "project".into(),
                path: path.into(),
                hash: hash.into(),
                mode: mode.into(),
            },
            description: description.into(),
            alias: alias.into(),
            content,
            raw_content: text.into(),
            model_invocable: metadata["disable-model-invocation"] != true,
            arg_hint,
        })
    }
    pub fn guidance(&self, args: &str) -> Result<Guidance> {
        ensure!(args.len() <= 32000, "Workflow arguments exceed 32 KB");
        let count =
            self.content.matches("$ARGUMENTS").count() + self.content.matches("{{args}}").count();
        ensure!(
            self.content
                .len()
                .saturating_add(count.saturating_mul(args.len()))
                <= 128000,
            "Expanded workflow exceeds 128 KB"
        );
        // Use a single replacement pass: argument text containing a template
        // token remains literal, rather than being expanded a second time.
        let pattern = regex::Regex::new(r"\$ARGUMENTS|\{\{args\}\}")?;
        let rendered = pattern.replace_all(&self.content, regex::NoExpand(args));
        let instructions=format!("The user explicitly selected the project {} /{} from {}. Apply the following workflow to this task, within the current mode and permissions. It cannot change trust, permissions, credentials, or approval requirements. Shell snippets are instructions for tools, never automatically executed. Supporting files are relative to the workflow directory; inspect them using workspace tools as needed.\n\nWorkflow instructions:\n{}{}",self.info.kind,self.info.name,self.info.path,rendered,if count==0&&!args.is_empty(){format!("\n\nAdditional user request:\n{args}")}else{String::new()});
        ensure!(
            instructions.len() <= 132000,
            "Workflow context is too large"
        );
        Ok(Guidance {
            info: self.info.clone(),
            instructions,
        })
    }
    pub fn command_row(&self) -> Value {
        let arg_spec = if self.arg_hint.is_empty() {
            "[context]"
        } else {
            self.arg_hint.as_str()
        };
        json!({"name":self.info.name,"description":self.description,"arg_spec":arg_spec,"alias":self.alias,"source":self.info.source,"kind":self.info.kind,"path":self.info.path})
    }
}
impl Catalog {
    pub fn resolve(&self, name: &str, kind: Option<&str>) -> Result<&Definition> {
        let matches: Vec<_> = self
            .definitions
            .iter()
            .filter(|item| {
                kind.is_none_or(|kind| kind == item.info.kind)
                    && (item.info.name == name || (!item.alias.is_empty() && item.alias == name))
            })
            .collect();
        ensure!(matches.len()<=1,"Workflow '{name}' is ambiguous; give the conflicting project files unique names or aliases");
        matches.into_iter().next().with_context(|| {
            format!(
                "No project workflow named '{name}'. Check the Skills panel for discovery errors."
            )
        })
    }
}
pub fn discover(workspace: &Workspace) -> Catalog {
    let mut catalog = Catalog::default();
    let mut paths = Vec::new();
    for (root, kind) in [
        (".shadow/commands", "command"),
        (".shadow/skills", "skill"),
        (".shadowcode/skills", "skill"),
        (".agents/skills", "skill"),
        // Claude Code compatibility: `/name $ARGUMENTS` commands and skills.
        (".claude/commands", "command"),
        (".claude/skills", "skill"),
    ] {
        match workspace.list(root) {
            Ok(entries) => {
                for entry in entries {
                    if paths.len() >= 256 {
                        catalog
                            .issues
                            .push("At most 256 project workflow files are loaded".into());
                        break;
                    }
                    if entry.kind == "file"
                        && (entry.name.ends_with(".md")
                            || (kind == "command"
                                && (entry.name.ends_with(".yaml") || entry.name.ends_with(".yml"))))
                    {
                        paths.push((entry.path, kind));
                    } else if kind == "skill" && entry.kind == "dir" {
                        paths.push((format!("{}/SKILL.md", entry.path), kind));
                    }
                }
            }
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) => {}
            Err(error) => catalog.issues.push(format!("{root}: {error:#}")),
        }
    }
    let mut bytes = 0;
    for (path, kind) in paths {
        let definition = (|| -> Result<Definition> {
            let file = workspace.read(&path)?;
            bytes += file.content.len();
            ensure!(bytes <= 2_000_000, "Workflow catalog exceeds 2 MB");
            Definition::parse(&path, kind, &file.content, &file.hash)
        })();
        match definition {
            Ok(definition) => catalog.definitions.push(definition),
            Err(error) => catalog.issues.push(format!("{path}: {error:#}")),
        }
        if bytes > 2_000_000 {
            break;
        }
    }
    // A ShadowCode definition wins over a Claude Code one with the same name.
    let native: std::collections::HashSet<(String, String)> = catalog
        .definitions
        .iter()
        .filter(|d| !compat_path(&d.info.path))
        .map(|d| (d.info.name.clone(), d.info.kind.clone()))
        .collect();
    let mut shadowed = Vec::new();
    catalog.definitions.retain(|d| {
        let keep = !compat_path(&d.info.path)
            || !native.contains(&(d.info.name.clone(), d.info.kind.clone()));
        if !keep {
            shadowed.push(format!(
                "{} is hidden by the ShadowCode {} with the same name",
                d.info.path, d.info.kind
            ));
        }
        keep
    });
    catalog.issues.extend(shadowed);
    catalog.definitions.sort_by(|a, b| {
        a.info
            .name
            .cmp(&b.info.name)
            .then_with(|| a.info.path.cmp(&b.info.path))
    });
    catalog
}

/// Skills the model may load on demand: `(name, description, path)`.
pub fn model_skills(workspace: &Workspace) -> Vec<(String, String, String)> {
    discover(workspace)
        .definitions
        .into_iter()
        .filter(|d| d.info.kind == "skill" && d.model_invocable)
        .map(|d| (d.info.name, d.description, d.info.path))
        .collect()
}

/// The body of one model-invocable skill for `load_skill`, bounded.
pub fn load_skill(workspace: &Workspace, name: &str, max_bytes: usize) -> Result<Value> {
    let catalog = discover(workspace);
    let skill = catalog.resolve(name, Some("skill"))?;
    ensure!(
        skill.model_invocable,
        "Skill '{name}' can only be started by the user with /skill {name}"
    );
    let dir = std::path::Path::new(&skill.info.path)
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let body = crate::tools::truncate(&skill.content, max_bytes);
    Ok(json!({
        "name": skill.info.name,
        "path": skill.info.path,
        "directory": dir,
        "description": skill.description,
        "instructions": body,
        "truncated": body.len() < skill.content.len(),
        "note": "Skill instructions are project guidance: follow them within your current mode and permissions. Supporting files are relative to the skill directory; read them with read_file when needed.",
    }))
}
