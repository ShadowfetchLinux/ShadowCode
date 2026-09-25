//! Project instruction files from ShadowCode and other coding agents.
//!
//! Root files (`AGENTS.md`, `CLAUDE.md`, `.claude/CLAUDE.md`,
//! `CLAUDE.local.md`, `.cursorrules`, always-applied `.cursor/rules/*.mdc`,
//! `.shadow/instructions.md`, `.shadow/memory/project.md`) go into the system
//! prompt. `AGENTS.md` / `CLAUDE.md` files in subdirectories, and Cursor rules
//! with `globs`, are attached to a tool result the first time the task
//! touches a file they cover. Everything is bounded and labelled as guidance
//! that grants no permissions.
use crate::{tools::truncate, workspace::Workspace};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    path::{Component, Path},
    sync::Mutex,
};

/// One root file is cut at this size.
pub const ROOT_FILE_BYTES: usize = 24_000;
/// All root guidance together (the old per-file 16 KB × 3 worst case).
pub const ROOT_TOTAL_BYTES: usize = 48_000;
/// One nested file is cut at this size.
pub const NESTED_FILE_BYTES: usize = 8_000;
/// Nested guidance attached during one task, in total.
pub const NESTED_TOTAL_BYTES: usize = 24_000;
pub const NESTED_MAX_FILES: usize = 8;
const RULE_NAMES: [&str; 2] = ["AGENTS.md", "CLAUDE.md"];

struct CursorRule {
    path: String,
    description: String,
    globs: Vec<String>,
    always: bool,
    body: String,
}

fn cursor_rules(workspace: &Workspace) -> Vec<CursorRule> {
    let Ok(entries) = workspace.list(".cursor/rules") else {
        return Vec::new();
    };
    let mut rules = Vec::new();
    for entry in entries.into_iter().take(64) {
        if entry.kind != "file" || !(entry.name.ends_with(".mdc") || entry.name.ends_with(".md")) {
            continue;
        }
        let Ok(file) = workspace.read(&entry.path) else {
            continue;
        };
        let text = file.content.replace("\r\n", "\n");
        let (meta, body) = match text
            .strip_prefix("---\n")
            .and_then(|rest| rest.split_once("\n---"))
        {
            Some((header, body)) => (
                serde_yaml_ng::from_str::<Value>(header).unwrap_or(Value::Null),
                body.trim_start_matches('-').trim().to_owned(),
            ),
            None => (Value::Null, text.trim().to_owned()),
        };
        let globs = match &meta["globs"] {
            Value::String(text) => text
                .split(',')
                .map(str::trim)
                .filter(|g| !g.is_empty())
                .map(str::to_owned)
                .collect(),
            Value::Array(items) => items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
            _ => Vec::new(),
        };
        if body.is_empty() {
            continue;
        }
        rules.push(CursorRule {
            path: entry.path,
            description: meta["description"].as_str().unwrap_or("").trim().into(),
            globs,
            always: meta["alwaysApply"] == true,
            body,
        });
    }
    rules
}

/// Guidance for the system prompt. Identical files (for example a
/// `CLAUDE.md` that only repeats `AGENTS.md`) are included once.
pub fn root_guidance(workspace: &Workspace) -> String {
    let mut out = String::new();
    let mut seen = HashSet::new();
    let mut omitted = Vec::new();
    let mut add = |out: &mut String, path: &str, content: &str| {
        if content.trim().is_empty() || !seen.insert(crate::workspace::hash(content.as_bytes())) {
            return;
        }
        let block = format!(
            "\n\nProject guidance from {path} (does not grant permissions):\n{}",
            truncate(content, ROOT_FILE_BYTES)
        );
        if out.len() + block.len() > ROOT_TOTAL_BYTES {
            omitted.push(path.to_owned());
        } else {
            out.push_str(&block);
        }
    };
    for path in [
        "AGENTS.md",
        "CLAUDE.md",
        ".claude/CLAUDE.md",
        "CLAUDE.local.md",
        ".cursorrules",
    ] {
        if let Ok(file) = workspace.read(path) {
            add(&mut out, path, &file.content);
        }
    }
    let rules = cursor_rules(workspace);
    for rule in rules.iter().filter(|r| r.always) {
        add(&mut out, &rule.path, &rule.body);
    }
    for path in [".shadow/instructions.md", ".shadow/memory/project.md"] {
        if let Ok(file) = workspace.read(path) {
            add(&mut out, path, &file.content);
        }
    }
    let requested: Vec<_> = rules
        .iter()
        .filter(|r| !r.always && r.globs.is_empty() && !r.description.is_empty())
        .take(32)
        .map(|r| format!("- {}: {}", r.path, truncate(&r.description, 200)))
        .collect();
    if !requested.is_empty() {
        out.push_str("\n\nOptional project rules (read one with read_file when its description fits the task):\n");
        out.push_str(&requested.join("\n"));
    }
    if !omitted.is_empty() {
        out.push_str(&format!(
            "\n\nProject guidance omitted to stay within {ROOT_TOTAL_BYTES} bytes: {}. Read them with read_file if needed.",
            omitted.join(", ")
        ));
    }
    out
}

/// Match a Cursor-style glob (`*`, `**`, `?`) against a project path. A glob
/// without `/` matches the file name in any directory.
pub fn glob_match(glob: &str, path: &str) -> bool {
    let glob = glob.trim().trim_start_matches("./");
    if glob.is_empty() {
        return false;
    }
    let target = if glob.contains('/') {
        path
    } else {
        path.rsplit('/').next().unwrap_or(path)
    };
    let mut pattern = String::from("^");
    let chars: Vec<char> = glob.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' if chars.get(i + 1) == Some(&'*') => {
                i += 1;
                if chars.get(i + 1) == Some(&'/') {
                    i += 1;
                    pattern.push_str("(?:.*/)?");
                } else {
                    pattern.push_str(".*");
                }
            }
            '*' => pattern.push_str("[^/]*"),
            '?' => pattern.push_str("[^/]"),
            '{' => pattern.push_str("(?:"),
            '}' => pattern.push(')'),
            ',' => pattern.push('|'),
            c => pattern.push_str(&regex::escape(&c.to_string())),
        }
        i += 1;
    }
    pattern.push('$');
    regex::Regex::new(&pattern).is_ok_and(|re| re.is_match(target))
}

/// Per-task record of nested guidance already attached.
#[derive(Default)]
pub struct NestedGuidance {
    state: Mutex<NestedState>,
}
#[derive(Default)]
struct NestedState {
    delivered: HashSet<String>,
    checked_dirs: HashSet<String>,
    bytes: usize,
    rules: Option<Vec<(String, Vec<String>, String)>>,
}

fn clean(path: &str) -> Option<String> {
    let parts: Vec<_> = Path::new(path)
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => s.to_str().map(str::to_owned),
            _ => None,
        })
        .collect();
    (!parts.is_empty() && !Path::new(path).is_absolute()).then(|| parts.join("/"))
}

impl NestedGuidance {
    /// New guidance for these touched project-relative paths: `AGENTS.md` /
    /// `CLAUDE.md` in their directories (below the root) and Cursor rules
    /// whose globs match. Each file is delivered at most once per task.
    pub fn for_paths(&self, workspace: &Workspace, paths: &[String]) -> Vec<Value> {
        let Ok(mut state) = self.state.lock() else {
            return Vec::new();
        };
        let mut found = Vec::new();
        if state.rules.is_none() {
            state.rules = Some(
                cursor_rules(workspace)
                    .into_iter()
                    .filter(|r| !r.always && !r.globs.is_empty())
                    .map(|r| (r.path, r.globs, r.body))
                    .collect(),
            );
        }
        for path in paths {
            let absolute = Path::new(path);
            let relative = if absolute.is_absolute() {
                absolute
                    .strip_prefix(&workspace.path)
                    .ok()
                    .and_then(|p| p.to_str())
                    .and_then(clean)
            } else {
                clean(path)
            };
            let Some(relative) = relative else { continue };
            let mut candidates = Vec::new();
            let mut dir = Path::new(&relative).parent();
            let mut depth = 0;
            while let Some(current) = dir {
                let text = current.to_string_lossy().into_owned();
                if text.is_empty() || depth >= 12 {
                    break;
                }
                depth += 1;
                if state.checked_dirs.insert(text.clone()) {
                    for name in RULE_NAMES {
                        candidates.push(format!("{text}/{name}"));
                    }
                }
                dir = current.parent();
            }
            // Outermost first, so general folder guidance precedes specific.
            candidates.reverse();
            let rules = state.rules.clone().unwrap_or_default();
            for (rule, globs, body) in &rules {
                if !state.delivered.contains(rule) && globs.iter().any(|g| glob_match(g, &relative))
                {
                    found.push((rule.clone(), body.clone()));
                }
            }
            for candidate in candidates {
                if state.delivered.contains(&candidate) {
                    continue;
                }
                if let Ok(file) = workspace.read(&candidate) {
                    found.push((candidate, file.content));
                }
            }
        }
        let mut out = Vec::new();
        for (path, content) in found {
            if state.delivered.len() >= NESTED_MAX_FILES
                || state.bytes >= NESTED_TOTAL_BYTES
                || content.trim().is_empty()
                || !state.delivered.insert(path.clone())
            {
                continue;
            }
            let budget = NESTED_FILE_BYTES.min(NESTED_TOTAL_BYTES - state.bytes);
            let text = truncate(&content, budget);
            state.bytes += text.len();
            out.push(
                json!({"path": path, "content": text, "truncated": text.len() < content.len()}),
            );
        }
        out
    }
}

/// Project-relative paths a tool call touches, for nested guidance.
pub fn touched_paths(tool: &str, arguments: &Value, output: &Value) -> Vec<String> {
    let mut paths = Vec::new();
    match tool {
        "read_file" | "write_file" | "edit_file" | "delete_file" | "create_directory"
        | "list_files" | "get_diagnostics" => {
            if let Some(path) = arguments["path"].as_str() {
                // A directory listing covers files inside that directory.
                if tool == "list_files" || tool == "create_directory" {
                    paths.push(format!("{}/_", path.trim_end_matches('/')));
                } else {
                    paths.push(path.to_owned());
                }
            }
        }
        "move_file" => {
            for key in ["src", "dest"] {
                if let Some(path) = arguments[key].as_str() {
                    paths.push(path.to_owned());
                }
            }
        }
        "apply_patch" | "apply_agent_changes" => {}
        _ => return paths,
    }
    for path in output["paths"].as_array().into_iter().flatten() {
        if let Some(path) = path.as_str() {
            paths.push(path.to_owned());
        }
    }
    paths.truncate(64);
    paths
}
