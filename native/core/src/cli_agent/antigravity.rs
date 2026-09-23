//! Antigravity CLI adapter (`agy`).
//!
//! Official print-mode stream-json
//! (https://antigravity.google/docs/cli/headless/):
//! `agy --print --output-format stream-json --input-format stream-json`
//! reads one NDJSON user message per stdin line and emits NDJSON events.
//! ShadowCode never treats the `antigravity` Electron app as this CLI, never
//! reads Google cookies, and never maps a Gemini API key to this adapter.
use super::{clip, redact, redact_value, ApprovalPrompt, CliAdapter, LaunchOptions, Step, Update, Vendor};
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

const OUTPUT_PREVIEW: usize = 8000;

#[derive(Default)]
pub struct AntigravityAdapter {
    started: bool,
    pending_prompt: Option<String>,
    pending_permissions: HashSet<String>,
    tool_names: HashMap<String, String>,
    tool_paths: HashMap<String, String>,
    streamed_text: bool,
    turn_active: bool,
}

impl AntigravityAdapter {
    fn user_message(text: &str) -> String {
        json!({"type":"user","message":{"role":"user","content":[{"type":"text","text":text}]}})
            .to_string()
    }
    fn content_blocks(message: &Value) -> Vec<Value> {
        match &message["content"] {
            Value::Array(blocks) => blocks.clone(),
            Value::String(text) => vec![json!({"type":"text","text":text})],
            _ => Vec::new(),
        }
    }
    fn assistant(&mut self, message: &Value) -> Step {
        let mut step = Step::default();
        for block in Self::content_blocks(message) {
            match block["type"].as_str().unwrap_or("") {
                "text" => {
                    if !self.streamed_text {
                        if let Some(text) = block["text"].as_str().filter(|t| !t.is_empty()) {
                            step.updates.push(Update::Text(redact(text)));
                        }
                    }
                }
                "tool_use" => {
                    let id = block["id"].as_str().unwrap_or("").to_owned();
                    let name = format!(
                        "antigravity.{}",
                        block["name"].as_str().unwrap_or("tool")
                    );
                    if let Some(path) = block["input"]["file_path"]
                        .as_str()
                        .or_else(|| block["input"]["path"].as_str())
                    {
                        self.tool_paths.insert(id.clone(), path.to_owned());
                    }
                    self.tool_names.insert(id.clone(), name.clone());
                    step.updates.push(Update::ToolStarted {
                        id,
                        name,
                        detail: redact_value(json!({"input":block["input"]})),
                    });
                }
                _ => {}
            }
        }
        self.streamed_text = false;
        step
    }
    fn tool_results(&mut self, message: &Value) -> Step {
        let mut step = Step::default();
        for block in Self::content_blocks(message) {
            if block["type"] != "tool_result" {
                continue;
            }
            let id = block["tool_use_id"].as_str().unwrap_or("").to_owned();
            let name = self
                .tool_names
                .get(&id)
                .cloned()
                .unwrap_or_else(|| "antigravity.tool".into());
            let success = block["is_error"].as_bool() != Some(true);
            let text = match &block["content"] {
                Value::String(text) => text.clone(),
                Value::Array(parts) => parts
                    .iter()
                    .filter_map(|p| p["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
                _ => String::new(),
            };
            step.updates.push(Update::ToolCompleted {
                id: id.clone(),
                name: name.clone(),
                success,
                output: redact_value(json!({"output":clip(&text, OUTPUT_PREVIEW)})),
            });
            if success {
                if let Some(path) = self.tool_paths.get(&id) {
                    step.updates.push(Update::FilesChanged {
                        paths: vec![path.clone()],
                        detail: json!({"tool":name}),
                    });
                }
            }
        }
        step
    }
    fn permission(&mut self, message: &Value) -> Step {
        let request_id = message["request_id"]
            .as_str()
            .or_else(|| message["id"].as_str())
            .unwrap_or("agy-perm")
            .to_owned();
        self.pending_permissions.insert(request_id.clone());
        let tool = message
            .pointer("/request/tool_name")
            .or_else(|| message.pointer("/tool_name"))
            .and_then(Value::as_str)
            .unwrap_or("tool");
        let command = message
            .pointer("/request/input/command")
            .or_else(|| message.pointer("/input/command"))
            .and_then(Value::as_str)
            .unwrap_or(tool);
        Step::update(Update::Approval(ApprovalPrompt {
            request_id,
            kind: if tool.eq_ignore_ascii_case("bash") || tool.contains("shell") {
                "command".into()
            } else {
                "tool".into()
            },
            tool: format!("antigravity.{tool}"),
            command: redact(command),
            reason: redact("Antigravity requests permission"),
            arguments: redact_value(message.clone()),
        }))
    }
}

impl CliAdapter for AntigravityAdapter {
    fn vendor(&self) -> Vendor {
        Vendor::Antigravity
    }
    fn command(&self, options: &LaunchOptions) -> (String, Vec<String>) {
        let mut args = vec![
            "--print".into(),
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
        (options.binary.clone(), args)
    }
    fn on_start(&mut self, options: &LaunchOptions) -> Vec<String> {
        let _ = options;
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
    fn prompt(&mut self, text: &str) -> Result<Vec<String>> {
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
                    "Ignored a non-JSON line from antigravity: {}",
                    clip(&redact(trimmed), 200)
                ))))
            }
        };
        if !message.is_object() {
            return Ok(Step::update(Update::Warning(
                "Ignored a non-object frame from antigravity".into(),
            )));
        }
        Ok(match message["type"].as_str().unwrap_or("") {
            "system" | "keep_alive" => Step::default(),
            "stream_event" => {
                let event = &message["event"];
                if event["type"] == "content_block_delta" && event["delta"]["type"] == "text_delta"
                {
                    match event["delta"]["text"].as_str() {
                        Some(text) if !text.is_empty() => {
                            self.streamed_text = true;
                            Step::update(Update::Text(redact(text)))
                        }
                        _ => Step::default(),
                    }
                } else {
                    Step::default()
                }
            }
            "assistant" => self.assistant(&message["message"].clone()),
            "user" => self.tool_results(&message["message"].clone()),
            "control_request" | "permission_request" => self.permission(&message),
            "result" => {
                self.turn_active = false;
                let mut step = Step::default();
                if let (Some(input), Some(output)) = (
                    message["usage"]["input_tokens"].as_u64(),
                    message["usage"]["output_tokens"].as_u64(),
                ) {
                    step.updates.push(Update::Usage { input, output });
                }
                if message["is_error"].as_bool() == Some(true) {
                    step.updates.push(Update::TurnFailed(format!(
                        "Antigravity error: {}",
                        redact(message["result"].as_str().unwrap_or("failed"))
                    )));
                } else {
                    if !self.streamed_text {
                        if let Some(text) = message["result"].as_str().filter(|t| !t.is_empty()) {
                            step.updates.push(Update::Text(redact(text)));
                        }
                    }
                    step.updates.push(Update::TurnCompleted {
                        text: None,
                        interrupted: message["subtype"].as_str() == Some("cancelled"),
                    });
                }
                step
            }
            _ => Step::update(Update::Warning(format!(
                "Ignored an unrecognized frame from antigravity: {}",
                clip(&redact(trimmed), 80)
            ))),
        })
    }
    fn approve(&mut self, request_id: &str, approve: bool) -> Result<Vec<String>> {
        if !self.pending_permissions.remove(request_id) {
            bail!("Unknown Antigravity permission request {request_id}");
        }
        Ok(vec![json!({
            "type":"control_response",
            "request_id":request_id,
            "response":{"behavior": if approve { "allow" } else { "deny" }}
        })
        .to_string()])
    }
    fn interrupt(&mut self) -> Vec<String> {
        if self.turn_active {
            vec![json!({"type":"control_request","request":{"subtype":"interrupt"}}).to_string()]
        } else {
            Vec::new()
        }
    }
    fn one_shot(&self) -> bool {
        true
    }
}
