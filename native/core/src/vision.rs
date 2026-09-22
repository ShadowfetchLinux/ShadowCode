//! Composer image attachments for vision-capable models.
//!
//! Bytes live under `.shadow/attachments/`. Transcript messages keep a bounded
//! `_shadow_images` reference (path + mime + size), never multi-megabyte base64.
use crate::workspace::{Workspace, MAX_FILE_BYTES};
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use serde_json::{json, Value};
use std::path::Path;

/// Maximum images on a single user turn.
pub const MAX_IMAGES_PER_TURN: usize = 4;
/// Per-image size cap (matches workspace file limit).
pub const MAX_IMAGE_BYTES: usize = MAX_FILE_BYTES;
/// Fixed token estimate per attached image for context budgeting.
pub const IMAGE_TOKEN_ESTIMATE: usize = 1_024;

const ALLOWED_EXT: &[&str] = &["png", "jpg", "jpeg", "webp"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageRef {
    pub path: String,
    pub mime: String,
    pub bytes: usize,
}

pub fn is_image_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| ALLOWED_EXT.iter().any(|ok| e.eq_ignore_ascii_case(ok)))
        .unwrap_or(false)
}

pub fn mime_for_filename(name: &str) -> Option<&'static str> {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())?
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

pub fn detect_mime(bytes: &[u8], filename: &str) -> Result<&'static str> {
    let sniffed = match bytes {
        [0x89, b'P', b'N', b'G', ..] => Some("image/png"),
        [0xFF, 0xD8, 0xFF, ..] => Some("image/jpeg"),
        [b'R', b'I', b'F', b'F', ..] if bytes.len() >= 12 && &bytes[8..12] == b"WEBP" => {
            Some("image/webp")
        }
        _ => None,
    }
    .context("Unsupported or corrupt image; use PNG, JPEG, or WebP")?;
    if let Some(named) = mime_for_filename(filename) {
        ensure!(
            named == sniffed,
            "Image type mismatch: file looks like {sniffed} but name suggests {named}"
        );
    }
    Ok(sniffed)
}

pub fn validate_image_bytes(bytes: &[u8], filename: &str) -> Result<&'static str> {
    ensure!(!bytes.is_empty(), "Image is empty");
    ensure!(
        bytes.len() <= MAX_IMAGE_BYTES,
        "Image exceeds the {} MB limit",
        MAX_IMAGE_BYTES / 1_000_000
    );
    detect_mime(bytes, filename)
}

/// Whether this provider/model combination can accept image inputs.
pub fn model_supports_vision(provider: &str, model: &str) -> bool {
    let name = model.to_ascii_lowercase();
    let hint = [
        "gemma4",
        "gemma-4",
        "llava",
        "moondream",
        "minicpm-v",
        "qwen2-vl",
        "qwen2.5-vl",
        "qwen3-vl",
        "pixtral",
        "vision",
        "gpt-4o",
        "gpt-4.1",
        "gpt-5",
        "claude-3",
        "claude-4",
        "claude-sonnet",
        "claude-opus",
        "claude-haiku",
        "gemini",
        "llama3.2-vision",
        "llama4",
    ]
    .iter()
    .any(|needle| name.contains(needle));
    if hint {
        return true;
    }
    // Cloud chat providers are usually multimodal for current flagship models;
    // still require an explicit vision hint for local/ollama text-only models.
    matches!(provider, "openai" | "openrouter" | "anthropic")
        && !["gpt-oss", "o1-mini", "codex", "davinci", "babbage"]
            .iter()
            .any(|needle| name.contains(needle))
}

pub fn ensure_vision_or_bail(provider: &str, model: &str, image_count: usize) -> Result<()> {
    if image_count == 0 {
        return Ok(());
    }
    ensure!(
        model_supports_vision(provider, model),
        "Model '{model}' ({provider}) does not support vision. Attach images only with a vision-capable model (for example gemma4), or remove the image attachment."
    );
    Ok(())
}

pub fn decode_data_base64(data: &str) -> Result<Vec<u8>> {
    let trimmed = data
        .trim()
        .strip_prefix("data:")
        .and_then(|rest| rest.split_once(',').map(|(_, b)| b))
        .unwrap_or(data.trim());
    B64.decode(trimmed.trim())
        .context("Image data is not valid base64")
}

pub fn store_attachment(workspace: &Workspace, filename: &str, bytes: &[u8]) -> Result<ImageRef> {
    let mime = validate_image_bytes(bytes, filename)?;
    let safe = Path::new(filename)
        .file_name()
        .and_then(|v| v.to_str())
        .context("Invalid image filename")?;
    ensure!(
        !safe.is_empty() && safe.len() <= 200,
        "Invalid image filename"
    );
    ensure!(
        mime_for_filename(safe).is_some(),
        "Image filename must end in .png, .jpg, .jpeg, or .webp"
    );
    let path = format!(".shadow/attachments/{}-{safe}", crate::id());
    workspace.write(&path, bytes, Some("missing"))?;
    Ok(ImageRef {
        path,
        mime: mime.into(),
        bytes: bytes.len(),
    })
}

pub fn refs_from_paths(workspace: &Workspace, paths: &[String]) -> Result<Vec<ImageRef>> {
    ensure!(
        paths.len() <= MAX_IMAGES_PER_TURN,
        "At most {MAX_IMAGES_PER_TURN} images may be attached per message"
    );
    let mut refs = Vec::with_capacity(paths.len());
    for path in paths {
        ensure!(
            is_image_path(path),
            "Attachment '{path}' is not a PNG, JPEG, or WebP image"
        );
        let snap = workspace
            .snapshot(path)
            .with_context(|| format!("Cannot read image attachment {path}"))?;
        let bytes = snap
            .bytes
            .context(format!("Image attachment missing: {path}"))?;
        let mime = validate_image_bytes(&bytes, path)?;
        refs.push(ImageRef {
            path: path.clone(),
            mime: mime.into(),
            bytes: bytes.len(),
        });
    }
    Ok(refs)
}

pub fn user_message(text: &str, images: &[ImageRef]) -> Value {
    let content = if text.trim().is_empty() && !images.is_empty() {
        "Describe the attached image(s).".to_owned()
    } else {
        text.to_owned()
    };
    let mut message = json!({"role":"user","content":content});
    if !images.is_empty() {
        message["_shadow_images"] = json!(images
            .iter()
            .map(|img| json!({"path":img.path,"mime":img.mime,"bytes":img.bytes}))
            .collect::<Vec<_>>());
    }
    message
}

pub fn image_refs(message: &Value) -> Vec<ImageRef> {
    message["_shadow_images"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| {
            Some(ImageRef {
                path: v["path"].as_str()?.to_owned(),
                mime: v["mime"].as_str().unwrap_or("image/png").to_owned(),
                bytes: v["bytes"].as_u64().unwrap_or(0) as usize,
            })
        })
        .collect()
}

pub fn count_images(messages: &[Value]) -> usize {
    messages.iter().map(|m| image_refs(m).len()).sum()
}

/// Load attachment bytes and shape messages for the wire protocol.
/// Stored history keeps only `_shadow_images` path refs.
pub fn hydrate_for_provider(
    messages: &[Value],
    workspace: &Workspace,
    provider: &str,
) -> Result<Vec<Value>> {
    let mut out = Vec::with_capacity(messages.len());
    for message in messages {
        let refs = image_refs(message);
        if refs.is_empty() {
            out.push(message.clone());
            continue;
        }
        ensure!(
            refs.len() <= MAX_IMAGES_PER_TURN,
            "Message exceeds the {MAX_IMAGES_PER_TURN} image limit"
        );
        let mut payloads = Vec::with_capacity(refs.len());
        for img in &refs {
            let snap = workspace.snapshot(&img.path)?;
            let bytes = snap
                .bytes
                .with_context(|| format!("Image attachment missing: {}", img.path))?;
            let mime = validate_image_bytes(&bytes, &img.path)?;
            payloads.push((mime.to_owned(), B64.encode(&bytes)));
        }
        let text = message["content"].as_str().unwrap_or("").to_owned();
        let mut wire = message.clone();
        if let Some(obj) = wire.as_object_mut() {
            obj.remove("_shadow_images");
        }
        if provider == "ollama" {
            wire["content"] = json!(text);
            wire["images"] = json!(payloads
                .iter()
                .map(|(_, b64)| b64.clone())
                .collect::<Vec<_>>());
        } else {
            let mut parts = vec![json!({"type":"text","text":text})];
            for (mime, b64) in payloads {
                parts.push(json!({
                    "type":"image_url",
                    "image_url":{"url":format!("data:{mime};base64,{b64}")}
                }));
            }
            wire["content"] = json!(parts);
        }
        out.push(wire);
    }
    Ok(out)
}

/// Token estimate that charges a fixed cost for image refs instead of base64.
pub fn estimate_message_tokens(message: &Value) -> usize {
    let shadow_images = image_refs(message).len();
    let wire_images = message
        .get("images")
        .and_then(Value::as_array)
        .map(|a| a.len())
        .unwrap_or(0);
    let part_images = message
        .get("content")
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter(|p| p["type"] == "image_url" || p.get("image_url").is_some())
                .count()
        })
        .unwrap_or(0);
    let images = shadow_images.max(wire_images).max(part_images);
    let text_tokens = match message.get("content") {
        Some(Value::String(s)) => s.len().div_ceil(3),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| {
                if p["type"] == "image_url" || p.get("image_url").is_some() {
                    None
                } else {
                    p["text"].as_str().or_else(|| p.as_str())
                }
            })
            .map(|s| s.len().div_ceil(3))
            .sum(),
        _ => {
            let mut clone = message.clone();
            if let Some(obj) = clone.as_object_mut() {
                obj.remove("_shadow_images");
                obj.remove("images");
                obj.remove("content");
            }
            clone.to_string().len().div_ceil(3)
        }
    };
    text_tokens + images * IMAGE_TOKEN_ESTIMATE
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn png_bytes() -> Vec<u8> {
        // Minimal valid 1x1 PNG
        vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00,
            0x00, 0x90, 0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08,
            0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01, 0x00, 0x05, 0xFE,
            0x02, 0xFE, 0xDC, 0xCC, 0x59, 0xE7, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44,
            0xAE, 0x42, 0x60, 0x82,
        ]
    }

    #[test]
    fn rejects_oversized_and_non_image() {
        assert!(validate_image_bytes(&[], "a.png").is_err());
        assert!(validate_image_bytes(&[1, 2, 3], "a.png").is_err());
        let big = vec![0u8; MAX_IMAGE_BYTES + 1];
        assert!(validate_image_bytes(&big, "a.png").is_err());
        assert_eq!(
            validate_image_bytes(&png_bytes(), "shot.png").unwrap(),
            "image/png"
        );
    }

    #[test]
    fn vision_capability_heuristics() {
        assert!(model_supports_vision(
            "ollama",
            "huihui_ai/gemma-4-abliterated:12b"
        ));
        assert!(model_supports_vision("ollama", "gemma4:12b"));
        assert!(!model_supports_vision("ollama", "gpt-oss:20b"));
        assert!(!model_supports_vision("ollama", "qwen3:14b"));
        assert!(model_supports_vision("openai", "gpt-4o-mini"));
    }

    #[test]
    fn ollama_and_openai_payloads_include_images() {
        let root = tempdir().unwrap();
        let ws_path = root.path().join("project");
        std::fs::create_dir_all(&ws_path).unwrap();
        let ws = Workspace::open(&ws_path).unwrap();
        let stored = store_attachment(&ws, "ok.png", &png_bytes()).unwrap();
        let message = user_message("What color?", std::slice::from_ref(&stored));
        assert!(message["content"].as_str().unwrap().contains("What color"));
        assert_eq!(message["_shadow_images"][0]["path"], stored.path);
        assert!(!message.to_string().contains("iVBORw0KGgo")); // no raw b64 dump in stored form

        let ollama = hydrate_for_provider(std::slice::from_ref(&message), &ws, "ollama").unwrap();
        assert!(ollama[0]["images"].as_array().unwrap().len() == 1);
        assert!(ollama[0]["images"][0]
            .as_str()
            .unwrap()
            .starts_with("iVBOR"));
        assert!(ollama[0].get("_shadow_images").is_none());

        let openai = hydrate_for_provider(&[message], &ws, "openai").unwrap();
        let parts = openai[0]["content"].as_array().unwrap();
        assert_eq!(parts[0]["type"], "text");
        assert_eq!(parts[1]["type"], "image_url");
        assert!(parts[1]["image_url"]["url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,"));
    }

    #[test]
    fn caps_image_count() {
        let paths: Vec<_> = (0..5).map(|i| format!("shot{i}.png")).collect();
        let root = tempdir().unwrap();
        let ws_path = root.path().join("project");
        std::fs::create_dir_all(&ws_path).unwrap();
        let ws = Workspace::open(&ws_path).unwrap();
        assert!(refs_from_paths(&ws, &paths).is_err());
    }
}
