//! Streaming transports for local Ollama and compatible Chat Completions APIs.
use crate::{
    config::{secret, ModelConfig},
    paths::AppPaths,
};
use anyhow::{bail, ensure, Context, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeMap, time::Duration};
use tokio_util::sync::CancellationToken;

const MAX_WIRE_BYTES: usize = 16_000_000;
const MAX_LINE_BYTES: usize = 1_000_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}
impl Usage {
    pub fn add(&mut self, other: &Self) {
        self.prompt_tokens = self.prompt_tokens.saturating_add(other.prompt_tokens);
        self.completion_tokens = self
            .completion_tokens
            .saturating_add(other.completion_tokens);
        self.total_tokens = self.total_tokens.saturating_add(other.total_tokens);
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChatResponse {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub usage: Usage,
    pub finish_reason: String,
}

#[derive(Clone)]
pub struct ModelClient {
    client: reqwest::Client,
    pub config: ModelConfig,
    key: Option<String>,
}
impl ModelClient {
    pub fn new(config: ModelConfig, paths: &AppPaths) -> Result<Self> {
        let validation = crate::config::Config {
            model: config.clone(),
            ..Default::default()
        };
        validation.validate()?;
        let key = secret(paths, &config.api_key_env)?;
        Ok(Self {
            client: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(600))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            config,
            key,
        })
    }
    pub fn endpoint(&self) -> String {
        let endpoint = if self.config.endpoint.is_empty() {
            preset(&self.config.provider)["endpoint"]
                .as_str()
                .unwrap_or("")
                .to_owned()
        } else {
            self.config.endpoint.clone()
        };
        endpoint.trim_end_matches('/').to_owned()
    }
    pub fn request_body(&self, messages: &[Value], tools: &[Value], max_tokens: usize) -> Value {
        let messages: Vec<Value> = messages
            .iter()
            .map(|message| {
                let mut message = message.clone();
                if let Some(object) = message.as_object_mut() {
                    object.retain(|key, _| !key.starts_with("_shadow_"));
                }
                message
            })
            .collect();
        if self.config.provider == "ollama" {
            // Common installed Ollama templates (including Qwen3) render only
            // the leading system block and skip later system-role messages.
            // Preserve runtime repair/compaction notes there, with their original
            // position labelled so historical failures do not look newly issued.
            let guidance: Vec<_> = messages.iter().enumerate().filter(|(_, m)| m["role"] == "system").map(|(index, m)| {
                let text = m["content"].as_str().unwrap_or("");
                if index == 0 { text.to_owned() } else {
                    format!("[Runtime note at conversation position {index}; later messages may resolve it.]\n{text}")
                }
            }).collect();
            let mut converted = Vec::new();
            if !guidance.is_empty() {
                converted.push(json!({"role":"system", "content":guidance.join("\n\n")}));
            }
            converted.extend(messages.iter().filter(|m| m["role"] != "system").map(|m| {
                let mut m = m.clone();
                if m["role"] == "tool" {
                    if let Some(name) = m.get("name").cloned() {
                        m["tool_name"] = name;
                    }
                    if let Some(obj) = m.as_object_mut() {
                        obj.remove("tool_call_id");
                    }
                }
                if let Some(calls) = m.get_mut("tool_calls").and_then(Value::as_array_mut) {
                    for call in calls {
                        if let Some(args) =
                            call.pointer("/function/arguments").and_then(Value::as_str)
                        {
                            if let Ok(args) = serde_json::from_str::<Value>(args) {
                                call["function"]["arguments"] = args;
                            }
                        }
                    }
                }
                m
            }));
            // keep_alive: keep the loaded model resident between turns (Ollama).
            // Prefix reuse: Ollama HTTP does not expose a safe prompt-prefix cache
            // API; we still avoid inventing a speedup claim. Unchanged system+tool
            // text is hashed for diagnostics only.
            let prefix_hash = {
                use sha2::{Digest, Sha256};
                let mut h = Sha256::new();
                if let Some(sys) = converted.iter().find(|m| m["role"] == "system") {
                    h.update(sys["content"].as_str().unwrap_or("").as_bytes());
                }
                h.update(serde_json::to_vec(tools).unwrap_or_default());
                format!("{:x}", h.finalize())[..16].to_owned()
            };
            let mut body = json!({
                "model": self.config.name,
                "messages": converted,
                "stream": true,
                "think": false,
                "keep_alive": self.config.keep_alive.clone(),
                "options": {
                    "num_predict": max_tokens,
                    "num_ctx": self.config.context_limit
                },
                "_shadow_prefix_hash": prefix_hash,
            });
            if !tools.is_empty() {
                body["tools"] = json!(tools);
            }
            // Strip internal diagnostic fields before send.
            if let Some(obj) = body.as_object_mut() {
                obj.remove("_shadow_prefix_hash");
            }
            body
        } else {
            let mut body = json!({"model":self.config.name,"messages":messages,"stream":true,"stream_options":{"include_usage":true},"max_tokens":max_tokens});
            if !tools.is_empty() {
                body["tools"] = json!(tools);
                body["tool_choice"] = json!("auto");
            }
            body
        }
    }
    /// Cancellation drops the HTTP body immediately. Once any response bytes
    /// arrive, the caller must not retry invisibly: partial tool arguments must
    /// never execute, and duplicate assistant text must not be appended.
    pub async fn chat<F>(
        &self,
        messages: &[Value],
        tools: &[Value],
        cancel: CancellationToken,
        mut text: F,
    ) -> Result<ChatResponse>
    where
        F: FnMut(&str) + Send,
    {
        ensure!(
            self.config.provider != "mock",
            "Offline demonstrations are handled by the agent fixture"
        );
        let ollama = self.config.provider == "ollama";
        let endpoint = self.endpoint();
        let url = if ollama {
            format!("{}/api/chat", endpoint.trim_end_matches("/v1"))
        } else {
            format!(
                "{}/chat/completions",
                if endpoint.ends_with("/v1") {
                    endpoint
                } else {
                    format!("{endpoint}/v1")
                }
            )
        };
        let response_tokens =
            crate::context::response_budget(messages, tools, self.config.context_limit)?;
        let body = self.request_body(messages, tools, response_tokens);
        let mut request = self.client.post(&url).json(&body);
        if let Some(key) = &self.key {
            request = request.bearer_auth(key);
        }
        let response = tokio::select! {_=cancel.cancelled()=>bail!("Model request cancelled"),response=request.send()=>response.context("Could not connect to the model provider")?};
        let status = response.status();
        if !status.is_success() {
            let code = status.as_u16();
            bail!(
                "Model provider returned HTTP {code}{}",
                match code {
                    401 | 403 => "; check the API key",
                    404 => "; check the endpoint and model name",
                    429 => "; provider rate limit reached",
                    _ => "",
                }
            );
        }
        let json_response = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|h| h.to_str().ok())
            .is_some_and(|h| h.starts_with("application/json"));
        let mut stream = response.bytes_stream();
        let mut decoder = StreamDecoder::new(ollama);
        let mut raw = Vec::new();
        let mut bytes = 0;
        loop {
            let next = tokio::select! {_=cancel.cancelled()=>bail!("Model request cancelled"),next=tokio::time::timeout(Duration::from_secs(120),stream.next())=>next.context("Model response stalled for 120 seconds")?};
            let Some(chunk) = next else { break };
            let chunk = chunk.context("Model stream disconnected before completion")?;
            bytes += chunk.len();
            ensure!(
                bytes <= MAX_WIRE_BYTES,
                "Model response exceeded the 16 MB limit"
            );
            if json_response {
                raw.extend_from_slice(&chunk);
            } else {
                for delta in decoder.push(&chunk)? {
                    text(&delta);
                }
                if decoder.done {
                    break;
                }
            }
        }
        if json_response {
            let value: Value =
                serde_json::from_slice(&raw).context("Provider returned invalid JSON")?;
            decoder.full_response(value)?;
            text(&decoder.response.text);
        } else {
            for delta in decoder.flush()? {
                text(&delta);
            }
        }
        decoder.finish()
    }
    pub async fn test(&self, cancel: CancellationToken) -> Result<Value> {
        let start = std::time::Instant::now();
        let response = self
            .chat(
                &[json!({"role":"user","content":"Reply with exactly: ShadowCode connected"})],
                &[],
                cancel,
                |_| {},
            )
            .await?;
        ensure!(
            !response.text.trim().is_empty(),
            "Model returned an empty reply"
        );
        Ok(
            json!({"ok":true,"latency_ms":start.elapsed().as_millis(),"reply":response.text,"model":self.config.name,"usage":response.usage}),
        )
    }
}

#[derive(Default)]
struct PartialCall {
    id: String,
    name: String,
    args: String,
}

/// Incremental wire parser. Bytes may split anywhere, including inside UTF-8,
/// JSON escapes, SSE frames, or a function's arguments.
pub struct StreamDecoder {
    ollama: bool,
    pending: Vec<u8>,
    sse_data: Vec<String>,
    response: ChatResponse,
    calls: BTreeMap<usize, PartialCall>,
    pub done: bool,
    seen_finish: bool,
}
impl StreamDecoder {
    pub fn new(ollama: bool) -> Self {
        Self {
            ollama,
            pending: Vec::new(),
            sse_data: Vec::new(),
            response: ChatResponse::default(),
            calls: BTreeMap::new(),
            done: false,
            seen_finish: false,
        }
    }
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<String>> {
        ensure!(
            bytes.len() <= MAX_WIRE_BYTES,
            "Model stream chunk exceeded 16 MB"
        );
        let mut pending = std::mem::take(&mut self.pending);
        pending.extend_from_slice(bytes);
        let mut text = Vec::new();
        let mut start = 0;
        while let Some(length) = pending[start..].iter().position(|b| *b == b'\n') {
            ensure!(length <= MAX_LINE_BYTES, "Model stream frame exceeded 1 MB");
            let end = start + length;
            let line = std::str::from_utf8(&pending[start..=end])
                .context("Model stream is not UTF-8")?
                .trim_end_matches(['\n', '\r']);
            self.line(line, &mut text)?;
            start = end + 1;
        }
        self.pending.extend_from_slice(&pending[start..]);
        ensure!(
            self.pending.len() <= MAX_LINE_BYTES,
            "Model stream frame exceeded 1 MB"
        );
        Ok(text)
    }
    fn line(&mut self, line: &str, out: &mut Vec<String>) -> Result<()> {
        if self.done {
            return Ok(());
        }
        if self.ollama {
            if !line.trim().is_empty() {
                self.chunk(
                    serde_json::from_str(line).context("Invalid Ollama stream frame")?,
                    out,
                )?;
            }
        } else if line.is_empty() {
            self.frame(out)?;
        } else if let Some(data) = line.strip_prefix("data:") {
            self.sse_data
                .push(data.strip_prefix(' ').unwrap_or(data).into());
            ensure!(
                self.sse_data.len() <= 10_000
                    && self.sse_data.iter().map(String::len).sum::<usize>() <= MAX_LINE_BYTES,
                "SSE event exceeded 1 MB"
            );
        }
        Ok(())
    }
    fn frame(&mut self, out: &mut Vec<String>) -> Result<()> {
        if self.sse_data.is_empty() {
            return Ok(());
        }
        let data = std::mem::take(&mut self.sse_data).join("\n");
        if data.trim() == "[DONE]" {
            self.done = true;
            return Ok(());
        }
        self.chunk(
            serde_json::from_str(&data).context("Invalid compatible stream frame")?,
            out,
        )
    }
    pub fn flush(&mut self) -> Result<Vec<String>> {
        let mut out = Vec::new();
        if !self.pending.is_empty() {
            let bytes = std::mem::take(&mut self.pending);
            let line = std::str::from_utf8(&bytes).context("Truncated UTF-8 stream")?;
            self.line(line.trim_end_matches('\r'), &mut out)?;
        }
        if !self.ollama {
            self.frame(&mut out)?;
        }
        Ok(out)
    }
    fn chunk(&mut self, value: Value, out: &mut Vec<String>) -> Result<()> {
        ensure!(
            value.get("error").is_none(),
            "Provider reported an error while generating"
        );
        let message = if self.ollama {
            &value["message"]
        } else {
            &value["choices"][0]["delta"]
        };
        if let Some(text) = message["content"].as_str() {
            self.response.text.push_str(text);
            if !text.is_empty() {
                out.push(text.into());
            }
        }
        ensure!(
            self.response.text.len() <= 4_000_000,
            "Model output exceeded the text limit"
        );
        if let Some(calls) = message["tool_calls"].as_array() {
            for (position, call) in calls.iter().enumerate() {
                let index = self.resolve_call_index(call, position, calls.len());
                ensure!(index < 128, "Too many tool calls in one response");
                let part = self.calls.entry(index).or_default();
                if let Some(id) = call["id"].as_str() {
                    if part.id.is_empty() {
                        part.id.push_str(id);
                    } else {
                        ensure!(part.id == id, "Tool call id changed during streaming");
                    }
                }
                if let Some(name) = call["function"]["name"].as_str() {
                    if part.name.is_empty() {
                        part.name.push_str(name);
                    } else if part.name != name && !name.is_empty() {
                        if name.starts_with(&part.name) {
                            part.name = name.to_owned();
                        } else if !part.name.ends_with(name) {
                            part.name.push_str(name);
                        }
                    }
                }
                if let Some(args) = call["function"].get("arguments") {
                    if let Some(args) = args.as_str() {
                        part.args.push_str(args);
                    } else {
                        part.args = args.to_string();
                    }
                }
                ensure!(
                    part.args.len() <= MAX_LINE_BYTES
                        && part.name.len() <= 256
                        && part.id.len() <= 512,
                    "Tool call exceeded limits"
                );
            }
        }
        if self.ollama {
            if value["done"].as_bool() == Some(true) {
                self.done = true;
                self.seen_finish = true;
                self.response.finish_reason =
                    value["done_reason"].as_str().unwrap_or("stop").into();
            }
            self.response.usage.prompt_tokens = value["prompt_eval_count"]
                .as_u64()
                .unwrap_or(self.response.usage.prompt_tokens);
            self.response.usage.completion_tokens = value["eval_count"]
                .as_u64()
                .unwrap_or(self.response.usage.completion_tokens);
        } else {
            if let Some(reason) = value["choices"][0]["finish_reason"].as_str() {
                self.response.finish_reason = reason.into();
                self.seen_finish = true;
            }
            if let Some(usage) = value.get("usage").filter(|v| !v.is_null()) {
                self.response.usage.prompt_tokens = usage["prompt_tokens"].as_u64().unwrap_or(0);
                self.response.usage.completion_tokens =
                    usage["completion_tokens"].as_u64().unwrap_or(0);
            }
        }
        self.response.usage.total_tokens = self
            .response
            .usage
            .prompt_tokens
            .saturating_add(self.response.usage.completion_tokens);
        Ok(())
    }
    /// Compatible local servers often omit `index` after the first delta.
    /// Use an explicit index, then a matching call id, then continue one
    /// incomplete same-name call. A new name or a completed same-name call
    /// starts another slot. Ollama still treats each unindexed frame as a
    /// new call unless an id matches an earlier one.
    fn resolve_call_index(&self, call: &Value, position: usize, chunk_len: usize) -> usize {
        if let Some(index) = call["index"].as_u64() {
            return index as usize;
        }
        if let Some(id) = call["id"].as_str().filter(|id| !id.is_empty()) {
            if let Some((&index, _)) = self.calls.iter().find(|(_, part)| part.id == id) {
                return index;
            }
        }
        if self.ollama {
            return self.calls.len();
        }
        let name = call["function"]["name"].as_str().unwrap_or("");
        if name.is_empty() {
            return self.calls.keys().next_back().copied().unwrap_or(position);
        }
        let named: Vec<usize> = self
            .calls
            .iter()
            .filter(|(_, part)| part.name == name)
            .map(|(&index, _)| index)
            .collect();
        if chunk_len == 1 && named.len() == 1 {
            let args = &self.calls[&named[0]].args;
            if args.is_empty() || serde_json::from_str::<Value>(args).is_err() {
                return named[0];
            }
        }
        self.calls
            .keys()
            .next_back()
            .map(|index| index + 1)
            .unwrap_or(position)
    }
    fn full_response(&mut self, value: Value) -> Result<()> {
        if self.ollama {
            self.chunk(value, &mut Vec::new())?;
        } else {
            let choice = value["choices"]
                .as_array()
                .and_then(|v| v.first())
                .context("Provider returned no completion choices")?;
            ensure!(
                choice["message"].is_object(),
                "Provider returned no assistant message"
            );
            let chunk = json!({"choices":[{"delta":choice["message"],"finish_reason":choice["finish_reason"]}],"usage":value["usage"]});
            self.chunk(chunk, &mut Vec::new())?;
        }
        Ok(())
    }
    pub fn finish(mut self) -> Result<ChatResponse> {
        ensure!(
            self.seen_finish,
            "Model stream ended without a finish marker; no tools were executed"
        );
        ensure!(
            !matches!(
                self.response.finish_reason.as_str(),
                "length" | "max_tokens" | "content_filter"
            ),
            "Model response was cut short ({}); no partial tools were executed",
            self.response.finish_reason
        );
        for (_, part) in self.calls {
            ensure!(!part.name.is_empty(), "Tool call has no name");
            let arguments: Value = serde_json::from_str(if part.args.is_empty() {
                "{}"
            } else {
                &part.args
            })
            .context("Model returned incomplete or invalid tool arguments")?;
            ensure!(arguments.is_object(), "Tool arguments must be an object");
            self.response.tool_calls.push(ToolCall {
                id: if part.id.is_empty() {
                    format!("call_{}", crate::id())
                } else {
                    part.id
                },
                name: part.name,
                arguments,
            });
        }
        Ok(self.response)
    }
}

pub fn preset(provider: &str) -> Value {
    let (label, endpoint, key, local) = match provider {
        "mock" => ("Offline demo", "", "OPENAI_API_KEY", true),
        "ollama" => (
            "Ollama",
            "http://127.0.0.1:11434/v1",
            "OLLAMA_API_KEY",
            true,
        ),
        "local" => (
            "LM Studio / local",
            "http://127.0.0.1:1234/v1",
            "OPENAI_API_KEY",
            true,
        ),
        "llamacpp" => (
            "llama.cpp",
            "http://127.0.0.1:8080/v1",
            "OPENAI_API_KEY",
            true,
        ),
        "vllm" => ("vLLM", "http://127.0.0.1:8000/v1", "OPENAI_API_KEY", true),
        "grok" => ("Grok / xAI", "https://api.x.ai/v1", "XAI_API_KEY", false),
        _ => (
            "OpenAI-compatible",
            "https://api.openai.com/v1",
            "OPENAI_API_KEY",
            false,
        ),
    };
    json!({"id":provider,"provider":provider,"label":label,"name":label,"endpoint":endpoint,"api_key_env":key,"local":local,"needs_key":!local,"running":provider=="mock"})
}

pub fn presets() -> Vec<Value> {
    [
        "ollama",
        "local",
        "llamacpp",
        "vllm",
        "openai_compatible",
        "grok",
        "mock",
    ]
    .iter()
    .map(|p| preset(p))
    .collect()
}

pub async fn detect() -> Vec<Value> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(1200))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("TLS client");
    futures_util::future::join_all(["ollama","local","llamacpp","vllm"].into_iter().map(|provider|{
        let client=client.clone();async move {
            let preset=preset(provider);let base=preset["endpoint"].as_str().unwrap_or("");
            let url=if provider=="ollama"{format!("{}/api/tags",base.trim_end_matches("/v1"))}else{format!("{base}/models")};
            let start=std::time::Instant::now();
            let result: Result<Value> = async {
                let response = client.get(url).send().await?.error_for_status()?;
                let mut stream = response.bytes_stream();
                let mut bytes = Vec::new();
                while let Some(chunk) = stream.next().await {
                    let chunk = chunk?;
                    ensure!(bytes.len() + chunk.len() <= 2_000_000, "Model list exceeded 2 MB");
                    bytes.extend_from_slice(&chunk);
                }
                Ok(serde_json::from_slice(&bytes)?)
            }.await;
            match result {
                Ok(value)=>{
                    let empty=Vec::new();let rows=value[if provider=="ollama"{"models"}else{"data"}].as_array().unwrap_or(&empty);
                    let models: Vec<_> = rows
                        .iter()
                        .filter_map(|m| {
                            let name = if provider == "ollama" {
                                m["name"].as_str()
                            } else {
                                m["id"].as_str()
                            }?;
                            let mut caps = serde_json::Map::new();
                            caps.insert("completion".into(), Value::Bool(true));
                            if let Some(list) = m.get("capabilities").and_then(Value::as_array) {
                                for item in list {
                                    if let Some(s) = item.as_str() {
                                        caps.insert(s.to_string(), Value::Bool(true));
                                    }
                                }
                            }
                            Some(json!({
                                "id": name,
                                "name": name,
                                "size_bytes": m["size"].as_u64().unwrap_or(0),
                                "context_limit": m.pointer("/details/context_length").and_then(Value::as_u64).unwrap_or(0),
                                "capabilities": Value::Object(caps),
                                "detail": m.pointer("/details/parameter_size").and_then(Value::as_str).unwrap_or("")
                            }))
                        })
                        .collect();
                    json!({"provider":provider,"label":preset["label"],"endpoint":base,"running":true,"latency_ms":start.elapsed().as_millis(),"detail":format!("{} models available",models.len()),"models":models})
                }
                Err(_)=>json!({"provider":provider,"label":preset["label"],"endpoint":base,"running":false,"latency_ms":start.elapsed().as_millis(),"models":[],"detail":"Not reachable"}),
            }
        }
    })).await
}
