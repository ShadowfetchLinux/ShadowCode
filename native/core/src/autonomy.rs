//! 0.21 autonomy helpers. These classify existing tools, budgets, and
//! recovery — they do not replace context compaction, checkpoints, or
//! permissions.
use crate::{context, permissions, tools};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolClass {
    ReadOnly,
    WorkspaceMutation,
    Process,
    Network,
    External,
    Privileged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayClass {
    SafeToReplay,
    ReEvaluate,
    RequiresConfirmation,
    NeverAutoReplay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    Success,
    Partial,
    Failure,
    Cancelled,
    TimedOut,
    Denied,
    Truncated,
    NotFound,
    Conflict,
    Retryable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunawayAction {
    Continue,
    Warn,
    Replan,
    Pause,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimLevel {
    ModelClaim,
    Observed,
    Verified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutonomyCaps {
    pub max_steps: usize,
    pub max_tokens: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GitSafety {
    pub dirty: bool,
    pub staged: bool,
    pub untracked: bool,
    pub detached: bool,
    pub conflicted: bool,
    pub rebase_or_merge: bool,
    pub unusual_names: Vec<String>,
    pub binary_or_huge: bool,
    pub risk: &'static str,
    pub checkpoint_required: bool,
    pub note: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct CapabilityProfile {
    pub provider: String,
    pub context_window: usize,
    pub tools: bool,
    pub structured_output: bool,
    pub reasoning: bool,
    pub multimodal: bool,
    pub streaming: bool,
    pub parallel_tools: bool,
    pub quirks: Vec<&'static str>,
}

pub fn tool_class(name: &str) -> ToolClass {
    if permissions::read_only(name) {
        return ToolClass::ReadOnly;
    }
    match name {
        "write_file" | "edit_file" | "apply_patch" | "create_directory" | "move_file"
        | "delete_file" => ToolClass::WorkspaceMutation,
        "exec" | "background_start" | "background_stop" => ToolClass::Process,
        "mcp_call" | "mcp_tools" => ToolClass::External,
        "git_reset" | "git_clean" => ToolClass::Privileged,
        name if name.starts_with("git_") => ToolClass::WorkspaceMutation,
        _ => ToolClass::External,
    }
}

/// Crash recovery must never assume a mutation finished. Reads may be
/// repeated; shell and Git history changes need a human.
pub fn replay_class(name: &str) -> ReplayClass {
    match name {
        "list_files" | "read_file" | "search_files" | "search_text" | "search_symbol"
        | "mcp_sqlite_tables" | "mcp_sqlite_query" | "background_list" | "background_output"
        | "git_status" | "git_diff" | "git_log" | "update_plan" | "update_todos" => {
            ReplayClass::SafeToReplay
        }
        "git_branch" => ReplayClass::ReEvaluate,
        "exec" | "background_start" | "background_stop" | "mcp_call" | "git_commit"
        | "git_checkout" | "git_add" => ReplayClass::RequiresConfirmation,
        "write_file" | "edit_file" | "apply_patch" | "create_directory" | "move_file"
        | "delete_file" | "git_reset" | "git_clean" => ReplayClass::NeverAutoReplay,
        _ => ReplayClass::RequiresConfirmation,
    }
}

pub fn tool_status(success: bool, output: &Value, error: &str) -> ToolStatus {
    let timed_out = output["timed_out"] == true
        || output["timeout"] == true
        || error.contains("timed out")
        || error.contains("timed_out");
    let cancelled = output["cancelled"] == true || error.contains("cancelled");
    let denied = error.contains("denied")
        || error.contains("Permission")
        || error.contains("unavailable in read-only");
    let not_found = error.contains("not found")
        || error.contains("does not exist")
        || output["error"]
            .as_str()
            .is_some_and(|e| e.contains("not found"));
    let conflict = error.contains("changed after")
        || error.contains("stale")
        || error.contains("expected_hash")
        || error.contains("Checkpoint mismatch");
    let truncated = output["truncated"] == true;
    if cancelled {
        return ToolStatus::Cancelled;
    }
    if timed_out {
        return ToolStatus::TimedOut;
    }
    if denied {
        return ToolStatus::Denied;
    }
    if conflict {
        return ToolStatus::Conflict;
    }
    if not_found {
        return ToolStatus::NotFound;
    }
    if error.contains("connect") || error.contains("429") || error.contains("temporar") {
        return ToolStatus::Retryable;
    }
    if success && truncated {
        return ToolStatus::Truncated;
    }
    if success {
        return ToolStatus::Success;
    }
    if truncated || output.get("exit_code").is_some() {
        return ToolStatus::Partial;
    }
    ToolStatus::Failure
}

pub fn caps_for(profile: &str) -> Option<AutonomyCaps> {
    match profile {
        "conservative" => Some(AutonomyCaps {
            max_steps: 16,
            max_tokens: 64_000,
        }),
        "normal" => Some(AutonomyCaps {
            max_steps: 64,
            max_tokens: 1_000_000,
        }),
        "extended" => Some(AutonomyCaps {
            max_steps: 200,
            max_tokens: 4_000_000,
        }),
        "unlimited" | "custom" => None,
        _ => None,
    }
}

/// Named profiles never raise the configured caps. Unlimited/custom use
/// the user's configured limits as-is.
pub fn effective_caps(profile: &str, configured_steps: usize, configured_tokens: u64) -> AutonomyCaps {
    match caps_for(profile) {
        Some(named) => AutonomyCaps {
            max_steps: named.max_steps.min(configured_steps),
            max_tokens: named.max_tokens.min(configured_tokens),
        },
        None => AutonomyCaps {
            max_steps: configured_steps,
            max_tokens: configured_tokens,
        },
    }
}

pub fn budget_status(used_steps: usize, used_tokens: u64, caps: AutonomyCaps) -> Value {
    let step_ratio = used_steps as f64 / caps.max_steps.max(1) as f64;
    let token_ratio = used_tokens as f64 / caps.max_tokens.max(1) as f64;
    let ratio = step_ratio.max(token_ratio);
    let approaching = ratio >= 0.75;
    // Steps are enforced by the engine loop. Treat only the token cap as
    // exhausted here so the final allowed step can still run its tools.
    let exhausted = used_tokens >= caps.max_tokens;
    json!({
        "used_steps": used_steps,
        "used_tokens": used_tokens,
        "max_steps": caps.max_steps,
        "max_tokens": caps.max_tokens,
        "approaching": approaching,
        "exhausted": exhausted,
        "ratio": (ratio * 1000.0).round() / 1000.0,
        "silent_kill": false
    })
}

/// Progressive loop policy. Count 3 warns, 4 asks for a replan, 5 pauses.
/// Legitimate iteration with changing arguments is a different key.
pub fn runaway_action(repeats: usize) -> RunawayAction {
    match repeats {
        0..=2 => RunawayAction::Continue,
        3 => RunawayAction::Warn,
        4 => RunawayAction::Replan,
        _ => RunawayAction::Pause,
    }
}

pub fn capability_profile(provider: &str, context_limit: usize) -> CapabilityProfile {
    let local = matches!(
        provider,
        "ollama" | "local" | "llamacpp" | "lmstudio" | "vllm" | "mock"
    );
    CapabilityProfile {
        provider: provider.into(),
        context_window: context_limit,
        tools: provider != "mock",
        structured_output: !local,
        reasoning: matches!(provider, "openai" | "openrouter" | "anthropic"),
        multimodal: matches!(provider, "openai" | "openrouter" | "ollama"),
        streaming: true,
        parallel_tools: !matches!(provider, "ollama" | "llamacpp"),
        quirks: match provider {
            "ollama" => vec![
                "unindexed tool frames start a new call unless an id matches",
                "thinking is not forwarded as assistant text",
            ],
            "local" | "lmstudio" | "llamacpp" => vec![
                "later tool deltas often omit index and repeat id/name",
                "argument fragments may arrive as a one-element array",
            ],
            "openai" | "openrouter" => vec!["indexed tool streams are the default"],
            _ => vec!["treat missing usage as estimated"],
        },
    }
}

pub fn account(messages: &[Value], schemas: &[Value], context_limit: usize) -> Result<Value, anyhow::Error> {
    let reserved = context::response_budget(messages, schemas, context_limit).unwrap_or(256);
    let mut layers = BTreeMap::from([
        ("system", 0usize),
        ("live", 0),
        ("working", 0),
        ("project", 0),
        ("artifact", 0),
        ("historical", 0),
        ("tools", context::estimate_tokens(&json!(schemas))),
        ("reserved_output", reserved),
    ]);
    let last_user = messages.iter().rposition(|m| m["role"] == "user");
    for (index, message) in messages.iter().enumerate() {
        let tokens = context::estimate_tokens(message);
        let role = message["role"].as_str().unwrap_or("");
        let content = message["content"].as_str().unwrap_or("");
        if message["_shadow_compaction"] == true {
            *layers.get_mut("historical").unwrap() += tokens;
            continue;
        }
        if role == "system" {
            if content.contains("Project guidance from") {
                *layers.get_mut("project").unwrap() += tokens;
            } else {
                *layers.get_mut("system").unwrap() += tokens;
            }
            continue;
        }
        if last_user == Some(index) || last_user.is_some_and(|i| index > i) {
            *layers.get_mut("live").unwrap() += tokens;
            continue;
        }
        if role == "tool" && content.len() > 2000 {
            *layers.get_mut("artifact").unwrap() += tokens;
        } else if role == "tool" || message.get("tool_calls").is_some() {
            *layers.get_mut("working").unwrap() += tokens;
        } else {
            *layers.get_mut("historical").unwrap() += tokens;
        }
    }
    let used: usize = layers.values().sum();
    Ok(json!({
        "layers": layers,
        "used_estimated_tokens": used,
        "limit": context_limit,
        "remaining": context_limit.saturating_sub(used),
        "fits": used + 256 <= context_limit,
        "method": "deterministic_char_div3"
    }))
}

/// Extract what compaction must not silently forget. This is a keep-list
/// for the inspectable note, not a model-written summary.
pub fn preserve(messages: &[Value]) -> Value {
    let mut intent = Vec::new();
    let mut constraints = Vec::new();
    let mut unresolved = Vec::new();
    let mut completed = Vec::new();
    let mut failed = Vec::new();
    let mut files = BTreeSet::new();
    let mut verification = Vec::new();
    let mut plan = Value::Null;
    let mut security = Vec::new();
    for message in messages {
        let role = message["role"].as_str().unwrap_or("");
        let content = message["content"].as_str().unwrap_or("");
        if role == "user" && intent.len() < 4 {
            intent.push(tools::truncate(content, 280).to_owned());
        }
        if role == "user" && looks_like_constraint(content) && constraints.len() < 8 {
            constraints.push(tools::truncate(content, 240).to_owned());
        }
        if role == "assistant" {
            if let Some(calls) = message["tool_calls"].as_array() {
                for call in calls {
                    if let Ok(args) = serde_json::from_str::<Value>(
                        call["function"]["arguments"].as_str().unwrap_or("{}"),
                    ) {
                        collect_paths(&args, &mut files);
                    }
                }
            }
        }
        if role == "tool" {
            if let Ok(body) = serde_json::from_str::<Value>(content) {
                collect_paths(&body, &mut files);
                let name = message["name"].as_str().unwrap_or("");
                if body["success"] == false || body["ok"] == false {
                    if failed.len() < 8 {
                        failed.push(format!(
                            "{name}: {}",
                            tools::truncate(
                                body["error"].as_str().unwrap_or(content),
                                160
                            )
                        ));
                    }
                    if unresolved.len() < 8 {
                        unresolved.push(name.to_owned());
                    }
                } else if matches!(
                    name,
                    "write_file" | "edit_file" | "apply_patch" | "create_directory"
                ) && completed.len() < 8
                {
                    completed.push(name.to_owned());
                }
                if name == "exec" {
                    verification.push(json!({
                        "command": body["command"],
                        "success": body["success"],
                        "exit_code": body["exit_code"],
                        "timed_out": body["timed_out"]
                    }));
                }
            } else if content.contains("denied") && security.len() < 6 {
                security.push(tools::truncate(content, 160).to_owned());
            }
        }
        if message.get("tool_calls").is_none() && content.contains("\"steps\"") {
            if let Ok(body) = serde_json::from_str::<Value>(content) {
                if body.get("steps").is_some() {
                    plan = body;
                }
            }
        }
    }
    json!({
        "intent": intent,
        "constraints": constraints,
        "unresolved": unresolved,
        "completed": completed,
        "failed_approaches": failed,
        "file_locations": files.into_iter().take(24).collect::<Vec<_>>(),
        "verification": verification,
        "plan": plan,
        "security": security,
        "method": "deterministic_keep_list"
    })
}

fn looks_like_constraint(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("must not")
        || lower.contains("do not")
        || lower.contains("never ")
        || lower.contains("required:")
        || lower.contains("constraint")
}

fn collect_paths(value: &Value, files: &mut BTreeSet<String>) {
    for key in ["path", "src", "dest"] {
        if let Some(path) = value[key].as_str().filter(|p| !p.is_empty() && p.len() <= 512) {
            files.insert(path.to_owned());
        }
    }
    if let Some(paths) = value["paths"].as_array() {
        for path in paths.iter().filter_map(|p| p.as_str()).take(16) {
            if path.len() <= 512 {
                files.insert(path.to_owned());
            }
        }
    }
}

pub fn classify_verification(model_text: &str, commands: &[Value], inspected: bool) -> Value {
    let claims = looks_like_success_claim(model_text);
    let any_cmd = !commands.is_empty();
    let any_ok = commands.iter().any(|c| c["success"] == true);
    let any_fail = commands.iter().any(|c| c["success"] == false || c["timed_out"] == true);
    let evidence = commands.iter().any(looks_like_verification_command);
    let level = if evidence && any_ok && !any_fail {
        ClaimLevel::Verified
    } else if inspected || any_cmd {
        ClaimLevel::Observed
    } else {
        ClaimLevel::ModelClaim
    };
    json!({
        "claim": level,
        "verified": level == ClaimLevel::Verified,
        "model_claimed_success": claims,
        "inspected_workspace": inspected,
        "commands": commands,
        "unverified_claim": claims && level != ClaimLevel::Verified,
        "note": if claims && level != ClaimLevel::Verified {
            "Model text is not verification. Tests, compiler, lint, diff, or a recorded command result are required."
        } else {
            "Claim level is derived from recorded tool evidence, not prose."
        }
    })
}

fn looks_like_success_claim(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("tests passed")
        || lower.contains("all tests pass")
        || lower.contains("verified")
        || lower.contains("build succeeded")
        || lower.contains("correctly implemented")
}

fn looks_like_verification_command(command: &Value) -> bool {
    let text = command["command"].as_str().unwrap_or("").to_ascii_lowercase();
    ["test", "pytest", "cargo test", "npm test", "lint", "clippy", "tsc", "cargo check"]
        .iter()
        .any(|needle| text.contains(needle))
}

pub fn parse_git_status(branch_line: &str, entries: &str) -> GitSafety {
    let detached = branch_line.contains("detached") || branch_line.starts_with("## HEAD");
    let rebase_or_merge = branch_line.contains("rebasing")
        || branch_line.contains("merging")
        || entries.lines().any(|line| line.starts_with("u ") || line.contains("UU "));
    let mut dirty = false;
    let mut staged = false;
    let mut untracked = false;
    let mut conflicted = rebase_or_merge;
    let mut unusual_names = Vec::new();
    let mut binary_or_huge = false;
    for line in entries.lines() {
        if line.is_empty() {
            continue;
        }
        if line.starts_with("?? ") {
            untracked = true;
        } else if line.starts_with("u ") || line.contains("UU ") || line.contains("AA ") {
            conflicted = true;
        } else {
            dirty = true;
            if line.len() >= 2 && !line.starts_with(' ') && &line[..1] != "?" {
                staged = staged || !line.starts_with(' ');
            }
            if line.as_bytes().get(0).is_some_and(|c| *c != b'?' && *c != b' ') {
                staged = true;
            }
        }
        let name = line.splitn(2, ' ').nth(1).unwrap_or("");
        let file = name.rsplit_once(' ').map(|(_, n)| n).unwrap_or(name);
        if file.starts_with('-')
            || file.contains('\0')
            || file.contains('\n')
            || file.contains("..")
        {
            unusual_names.push(tools::truncate(file, 80).to_owned());
        }
        if file.ends_with(".bin") || file.ends_with(".wasm") || file.ends_with(".so") {
            binary_or_huge = true;
        }
    }
    let checkpoint_required = dirty || staged || conflicted || rebase_or_merge;
    let risk = if conflicted || rebase_or_merge {
        "conflict"
    } else if detached {
        "detached"
    } else if checkpoint_required {
        "dirty"
    } else {
        "clean"
    };
    GitSafety {
        dirty,
        staged,
        untracked,
        detached,
        conflicted,
        rebase_or_merge,
        unusual_names,
        binary_or_huge,
        risk,
        checkpoint_required,
        note: match risk {
            "conflict" => "Do not auto-commit, reset, or clean. Conflicts need a human.".into(),
            "detached" => "HEAD is detached. Do not reset or create commits without confirmation.".into(),
            "dirty" => "Take a safety checkpoint before destructive Git or shell work.".into(),
            _ => "Working tree is clean enough for ordinary read-only Git tools.".into(),
        },
    }
}

pub fn worktree_recovery_advice(reason: &str) -> Value {
    let (safe, action) = if reason.contains("moved") || reason.contains("path") {
        (false, "Refuse auto-recovery. Re-open the original real path; do not guess a relocated checkout.")
    } else if reason.contains("metadata") || reason.contains("lost") || reason.contains("index") {
        (false, "Refuse auto-recovery. Restore Git administrative files from the recorded worktree identity.")
    } else if reason.contains("locked") {
        (false, "Another Git process holds the worktree. Wait or inspect the lock; do not delete it blindly.")
    } else {
        (false, "Auto-recovery is unsafe. Surface the recorded paths and require review.")
    };
    json!({"auto_recover":safe,"action":action,"guess_paths":false})
}

pub fn catalog() -> Value {
    let names = [
        "list_files",
        "read_file",
        "search_files",
        "search_text",
        "search_symbol",
        "mcp_sqlite_tables",
        "mcp_sqlite_query",
        "background_start",
        "background_list",
        "background_output",
        "background_stop",
        "write_file",
        "edit_file",
        "apply_patch",
        "create_directory",
        "move_file",
        "delete_file",
        "exec",
        "git_status",
        "git_diff",
        "git_log",
        "git_branch",
        "git_checkout",
        "git_add",
        "git_commit",
        "git_reset",
        "git_clean",
        "update_plan",
        "mcp_tools",
        "mcp_call",
    ];
    json!(names
        .iter()
        .map(|name| json!({
            "name": name,
            "class": tool_class(name),
            "replay": replay_class(name),
            "read_only": permissions::read_only(name)
        }))
        .collect::<Vec<_>>())
}

/// Shell policy study helpers. Lexical word lists are not a sandbox.
pub fn shell_policy_limits() -> Value {
    json!({
        "sandbox": false,
        "method": "lexical_word_list",
        "false_negatives": [
            "quoted sudo via env/alias",
            "interpreter -c with destructive payload",
            "curl via python/node",
            "rm via find -exec or xargs"
        ],
        "false_positives": [
            "comments mentioning curl",
            "paths named rm-backup",
            "npm as a local project script name"
        ],
        "do_not_claim": "OS sandbox or complete command safety"
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_auto_replays_mutations_or_destructive_git() {
        assert_eq!(replay_class("read_file"), ReplayClass::SafeToReplay);
        assert_eq!(replay_class("exec"), ReplayClass::RequiresConfirmation);
        assert_eq!(replay_class("write_file"), ReplayClass::NeverAutoReplay);
        assert_eq!(replay_class("git_reset"), ReplayClass::NeverAutoReplay);
    }

    #[test]
    fn runaway_is_progressive() {
        assert_eq!(runaway_action(1), RunawayAction::Continue);
        assert_eq!(runaway_action(3), RunawayAction::Warn);
        assert_eq!(runaway_action(4), RunawayAction::Replan);
        assert_eq!(runaway_action(5), RunawayAction::Pause);
    }

    #[test]
    fn last_allowed_step_is_not_a_token_exhaustion() {
        let status = budget_status(
            1,
            30,
            AutonomyCaps {
                max_steps: 1,
                max_tokens: 1_000_000,
            },
        );
        assert_eq!(status["exhausted"], false);
        assert_eq!(status["approaching"], true);
    }

    #[test]
    fn profiles_never_raise_configured_caps() {
        let caps = effective_caps("extended", 64, 1_000_000);
        assert_eq!(caps.max_steps, 64);
        assert_eq!(caps.max_tokens, 1_000_000);
        let unlimited = effective_caps("unlimited", 64, 1_000_000);
        assert_eq!(unlimited.max_steps, 64);
    }

    #[test]
    fn model_prose_is_not_verification() {
        let report = classify_verification("All tests passed.", &[], false);
        assert_eq!(report["claim"], "model_claim");
        assert_eq!(report["verified"], false);
        assert_eq!(report["unverified_claim"], true);
        let observed = classify_verification(
            "Looks good",
            &[json!({"command":"ls","success":true})],
            true,
        );
        assert_eq!(observed["claim"], "observed");
        let verified = classify_verification(
            "Done",
            &[json!({"command":"cargo test --offline","success":true})],
            true,
        );
        assert_eq!(verified["claim"], "verified");
        assert_eq!(verified["verified"], true);
    }
}
