//! Antigravity CLI adapter (`agy`), documented headless protocol
//! (https://antigravity.google/docs/cli/headless/).
//!
//! Command: `agy --output-format stream-json --input-format stream-json
//! --print=` (the empty `--print=` form is required: `--print` followed by
//! another flag would swallow that flag as the prompt). Each stdin line is
//! `{"event":"user","message":{"content":[{"type":"text","text":…}]}}` and
//! only text blocks are accepted, so images are refused up front. stdout is
//! NDJSON: `init` (with `conversation_id`), `step_update` (`user_input`,
//! `agent_response` with `text_delta`, `tool` with `tool_name`/`tool_info`,
//! `checkpoint`), and one `result` per turn (`status` SUCCESS | ERROR |
//! CANCELED | INTERRUPTED, `response`, `usage`). Frames were recorded from
//! agy 1.2.9 on 2026-09-23.
//!
//! Antigravity applies its own permission settings
//! (`~/.gemini/antigravity-cli/settings.json`): in print mode a tool it may
//! not run is soft-denied and reported, not asked. ShadowCode therefore never
//! receives approval prompts from this runtime and says so in the UI.
//! Follow-ups resume the same conversation with `--conversation <id>`.
use super::{
    clip, redact, redact_value, CliAdapter, LaunchOptions, PromptImage, Step, Update, Vendor,
};
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::collections::HashMap;

const OUTPUT_PREVIEW: usize = 8000;

#[derive(Default)]
pub struct AntigravityAdapter {
    started: bool,
    pending_prompt: Option<String>,
    conversation_id: Option<String>,
    /// step_index -> tool name for steps already reported as started.
    tool_steps: HashMap<u64, String>,
    streamed_text: bool,
    turn_active: bool,
}

impl AntigravityAdapter {
    fn user_message(text: &str) -> String {
        json!({"event":"user","message":{"content":[{"type":"text","text":text}]}}).to_string()
    }
    fn step_update(&mut self, update: &Value) -> Step {
        let step_type = update["step_type"].as_str().unwrap_or("");
        let state = update["state"].as_str().unwrap_or("");
        let index = update["step_index"].as_u64().unwrap_or(0);
        match step_type {
            "agent_response" => {
                let mut step = Step::default();
                if let Some(delta) = update["text_delta"].as_str() {
                    if !delta.is_empty() {
                        self.streamed_text = true;
                        step.updates.push(Update::Text(redact(delta)));
                    }
                }
                step
            }
            "tool" => {
                let name = format!(
                    "antigravity.{}",
                    update["tool_name"]
                        .as_str()
                        .or_else(|| update["tool_info"]["name"].as_str())
                        .unwrap_or("tool")
                );
                let info = &update["tool_info"];
                let id = format!("agy-step-{index}");
                let mut step = Step::default();
                if let std::collections::hash_map::Entry::Vacant(slot) =
                    self.tool_steps.entry(index)
                {
                    slot.insert(name.clone());
                    step.updates.push(Update::ToolStarted {
                        id: id.clone(),
                        name: name.clone(),
                        detail: redact_value(json!({"parameters": info["parameters"]})),
                    });
                }
                if state == "DONE" {
                    let success = info["error"].is_null();
                    let output = info["output"]
                        .as_str()
                        .map(|o| clip(o, OUTPUT_PREVIEW))
                        .unwrap_or_default();
                    step.updates.push(Update::ToolCompleted {
                        id,
                        name: name.clone(),
                        success,
                        output: redact_value(json!({
                            "output": output,
                            "error": info["error"],
                            "duration_seconds": update["duration_seconds"],
                        })),
                    });
                    if success {
                        if let Some(paths) = edited_paths(&name, &info["parameters"]) {
                            step.updates.push(Update::FilesChanged {
                                paths,
                                detail: json!({"tool": name}),
                            });
                        }
                    }
                }
                step
            }
            _ => Step::default(),
        }
    }
    fn result(&mut self, result: &Value) -> Step {
        self.turn_active = false;
        let mut step = Step::default();
        if let Some(id) = result["conversation_id"].as_str() {
            if self.conversation_id.as_deref() != Some(id) {
                self.conversation_id = Some(id.to_owned());
                step.updates
                    .push(Update::NativeSession { id: id.to_owned() });
            }
        }
        let usage = &result["usage"];
        if let (Some(input), Some(output)) = (
            usage["input_tokens"].as_u64(),
            usage["output_tokens"].as_u64(),
        ) {
            step.updates.push(Update::Usage { input, output });
        }
        match result["status"].as_str().unwrap_or("SUCCESS") {
            "ERROR" => step.updates.push(Update::TurnFailed(format!(
                "Antigravity error: {}",
                redact(result["error"].as_str().unwrap_or("unknown error"))
            ))),
            "CANCELED" | "INTERRUPTED" => step.updates.push(Update::TurnCompleted {
                text: None,
                interrupted: true,
            }),
            _ => {
                let text = result["response"]
                    .as_str()
                    .filter(|t| !t.is_empty() && !self.streamed_text)
                    .map(redact);
                step.updates.push(Update::TurnCompleted {
                    text,
                    interrupted: false,
                });
            }
        }
        self.streamed_text = false;
        self.tool_steps.clear();
        step
    }
}

/// Paths a file-mutating Antigravity tool touched, taken from its own
/// parameters. Unknown tool shapes yield nothing rather than a guess.
fn edited_paths(tool_name: &str, parameters: &Value) -> Option<Vec<String>> {
    let lower = tool_name.to_ascii_lowercase();
    let mutating = [
        "write", "edit", "replace", "create", "delete", "move", "rename", "patch",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    if !mutating {
        return None;
    }
    let mut paths = Vec::new();
    for key in [
        "file_path",
        "path",
        "TargetFile",
        "AbsolutePath",
        "target_file",
        "filePath",
    ] {
        if let Some(path) = parameters[key].as_str() {
            if !path.is_empty() {
                paths.push(path.to_owned());
            }
        }
    }
    (!paths.is_empty()).then_some(paths)
}

impl CliAdapter for AntigravityAdapter {
    fn vendor(&self) -> Vendor {
        Vendor::Antigravity
    }
    fn command(&self, options: &LaunchOptions) -> (String, Vec<String>) {
        let mut args: Vec<String> = vec![
            "--output-format".into(),
            "stream-json".into(),
            "--input-format".into(),
            "stream-json".into(),
        ];
        if options.read_only {
            args.push("--mode".into());
            args.push("plan".into());
        }
        if !options.model.is_empty() && options.model != "default" && options.model != "auto" {
            args.push("--model".into());
            args.push(options.model.clone());
        }
        if let Some(conversation) = options.resume.as_deref().filter(|c| !c.is_empty()) {
            args.push("--conversation".into());
            args.push(conversation.to_owned());
        }
        // Must be last and in `--print=` form so no later flag is taken as
        // the prompt text.
        args.push("--print=".into());
        (options.binary.clone(), args)
    }
    fn on_start(&mut self, options: &LaunchOptions) -> Vec<String> {
        self.conversation_id = options.resume.clone().filter(|c| !c.is_empty());
        self.started = true;
        match self.pending_prompt.take() {
            Some(prompt) => {
                self.turn_active = true;
                vec![Self::user_message(&prompt)]
            }
            None => Vec::new(),
        }
    }
    fn ready(&self) -> bool {
        self.started
    }
    fn prompt(&mut self, text: &str, images: &[PromptImage]) -> Result<Vec<String>> {
        if !images.is_empty() {
            bail!("Antigravity's stream-json input accepts text blocks only; images are not forwarded");
        }
        if self.started {
            self.turn_active = true;
            Ok(vec![Self::user_message(text)])
        } else {
            self.pending_prompt = Some(text.to_owned());
            Ok(Vec::new())
        }
    }
    fn on_line(&mut self, line: &str) -> Result<Step> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Ok(Step::default());
        }
        let message: Value = match serde_json::from_str(trimmed) {
            Ok(value) => value,
            Err(_) => {
                return Ok(Step::update(Update::Warning(format!(
                    "Ignored a non-JSON line from agy: {}",
                    clip(&redact(trimmed), 200)
                ))))
            }
        };
        if !message.is_object() {
            return Ok(Step::update(Update::Warning(
                "Ignored a non-object frame from agy".into(),
            )));
        }
        Ok(match message["event"].as_str().unwrap_or("") {
            "init" => {
                let mut step = Step::default();
                if let Some(id) = message["conversation_id"].as_str() {
                    if self.conversation_id.as_deref() != Some(id) {
                        self.conversation_id = Some(id.to_owned());
                        step.updates
                            .push(Update::NativeSession { id: id.to_owned() });
                    }
                }
                step
            }
            "step_update" => {
                let update = message["step_update"].clone();
                self.step_update(&update)
            }
            "result" => {
                let result = message["result"].clone();
                self.result(&result)
            }
            "" => Step::update(Update::Warning(
                "Ignored a frame without event from agy".into(),
            )),
            _ => Step::default(),
        })
    }
    fn approve(&mut self, request_id: &str, _approve: bool) -> Result<Vec<String>> {
        bail!(
            "Antigravity print mode applies its own permission settings and does not ask ShadowCode (request {request_id})"
        )
    }
    fn interrupt(&mut self) -> Vec<String> {
        // No documented interrupt frame; the runner stops the process on
        // cancel. Pause is unsupported for this runtime.
        Vec::new()
    }
    fn native_session(&self) -> Option<String> {
        self.conversation_id.clone()
    }
}
