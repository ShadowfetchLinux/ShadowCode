use crate::config::{PermissionLevel, PermissionsConfig};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny(String),
    Ask(String),
}

pub fn read_only(tool: &str) -> bool {
    matches!(
        tool,
        "list_files"
            | "read_file"
            | "search_files"
            | "search_text"
            | "search_symbol"
            | "workspace_symbols"
            | "goto_definition"
            | "find_references"
            | "get_diagnostics"
            | "mcp_sqlite_tables"
            | "mcp_sqlite_query"
            | "background_list"
            | "background_output"
            | "git_status"
            | "git_diff"
            | "git_log"
            | "update_plan"
            | "update_todos"
    )
}

pub fn parallel_safe(tool: &str, args: &Value) -> bool {
    (read_only(tool) && !matches!(tool, "update_plan" | "update_todos"))
        || (tool == "git_branch" && args["create"].as_bool() != Some(true))
}

pub fn check(config: &PermissionsConfig, tool: &str, args: &Value) -> Decision {
    use Decision::*;
    if read_only(tool) || (tool == "git_branch" && args["create"].as_bool() != Some(true)) {
        return Allow;
    }
    if config.level == PermissionLevel::ReadOnly {
        return Deny(format!("{tool} is unavailable in read-only mode"));
    }
    match tool {
        "write_file" | "edit_file" | "apply_patch" | "create_directory" | "move_file" => Allow,
        "delete_file" => Ask("Delete a workspace file".into()),
        "git_add" => Ask("Stage files; repository clean filters may execute commands".into()),
        "git_commit" | "git_checkout" | "git_branch" => {
            Ask("Change repository history or the active branch".into())
        }
        "git_reset" | "git_clean" => {
            if config.level != PermissionLevel::Elevated {
                Deny("Destructive Git operations require elevated permissions".into())
            } else {
                Ask("Destructive Git operation".into())
            }
        }
        "background_stop" => Ask("Stop a managed background process in this project".into()),
        "exec" | "background_start" => {
            let command = args["command"].as_str().unwrap_or("");
            let words: Vec<_> = command
                .split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
                .filter(|s| !s.is_empty())
                .collect();
            if !config.allow_root
                && words
                    .iter()
                    .any(|word| matches!(*word, "sudo" | "doas" | "su" | "--privileged"))
            {
                return Deny("Root and privileged commands are disabled".into());
            }
            if !config.network
                && words.iter().any(|word| {
                    matches!(
                        *word,
                        "curl"
                            | "wget"
                            | "ssh"
                            | "scp"
                            | "rsync"
                            | "nc"
                            | "ncat"
                            | "nmap"
                            | "pip"
                            | "pip3"
                            | "npm"
                            | "npx"
                            | "pnpm"
                            | "yarn"
                            | "uv"
                            | "uvx"
                            | "bunx"
                            | "pipx"
                    )
                })
            {
                return Deny(
                    "Enable network commands in permissions before running this command".into(),
                );
            }
            if config.approve_shell {
                if tool == "background_start" {
                    return Ask("Start a project background process. It continues independently after this coding task, including cancellation, until stopped or the application closes. Its shell effects are not undone by rewind.".into());
                }
                return Ask("Run a shell command as your user; it can affect files and services beyond this project".into());
            }
            if config.require_approval_for_dangerous
                && words.iter().any(|word| {
                    matches!(
                        *word,
                        "rm" | "mkfs"
                            | "dd"
                            | "chmod"
                            | "chown"
                            | "reboot"
                            | "shutdown"
                            | "--force"
                            | "--hard"
                    )
                })
            {
                Ask("Potentially destructive shell command".into())
            } else {
                Allow
            }
        }
        _ => Deny(format!("Unknown or unregistered tool: {tool}")),
    }
}
