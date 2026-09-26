//! Optional cloud transcription through OpenRouter. Used only when the user
//! picked "OpenRouter" as the voice engine in Settings › Voice (off by
//! default, never a fallback): the recording is sent as a WAV
//! `input_audio` part to an audio-capable chat model with the user's key.
use super::audio;
use anyhow::{bail, ensure, Context, Result};
use base64::Engine as _;
use serde_json::{json, Value};
use std::time::Duration;

/// Gemini Flash-Lite: reads audio at 32 tokens per second, so a minute of
/// speech is about 1,900 input tokens (a fraction of a US cent).
pub const DEFAULT_MODEL: &str = "google/gemini-3.1-flash-lite";
const MAX_RESPONSE_BYTES: usize = 2_000_000;

/// The request body for one clip. Pure, for tests.
pub fn request_body(model: &str, wav: &[u8], language: &str) -> Value {
    let language = match language {
        "" | "auto" => "the language spoken".to_owned(),
        code => format!("the language with code \"{code}\""),
    };
    json!({
        "model": model,
        "temperature": 0,
        "max_tokens": 2_000,
        "messages": [{
            "role": "user",
            "content": [
                {
                    "type": "text",
                    "text": format!(
                        "Transcribe this recording word for word in {language}. It is dictation for a coding assistant, so keep technical terms, file names and code identifiers as spoken. Reply with the transcript only: no quotes, labels or commentary. If nothing is said, reply with nothing."
                    ),
                },
                {
                    "type": "input_audio",
                    "input_audio": {
                        "data": base64::engine::general_purpose::STANDARD.encode(wav),
                        "format": "wav",
                    },
                },
            ],
        }],
    })
}

/// The transcript in a chat completion (string or text parts).
pub fn response_text(body: &Value) -> Result<String> {
    if let Some(message) = body["error"]["message"].as_str() {
        bail!("OpenRouter: {message}");
    }
    let content = &body["choices"][0]["message"]["content"];
    if let Some(text) = content.as_str() {
        return Ok(text.trim().to_owned());
    }
    if let Some(parts) = content.as_array() {
        return Ok(parts
            .iter()
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join("")
            .trim()
            .to_owned());
    }
    bail!("OpenRouter returned no transcript")
}

/// Send mono samples to OpenRouter at `base` (the API root) and return the
/// transcript.
pub async fn transcribe(
    base: &str,
    key: &str,
    model: &str,
    mono: &[f32],
    rate: u32,
    language: &str,
) -> Result<String> {
    // 16 kHz 16-bit mono keeps a minute of speech under 2 MB.
    let samples = audio::resample(mono, rate, audio::WHISPER_RATE);
    let wav = audio::encode_wav(&samples, audio::WHISPER_RATE);
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(90))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("ShadowCode/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let response = client
        .post(format!("{base}/chat/completions"))
        .bearer_auth(key)
        .header("X-Title", "ShadowCode voice input")
        .json(&request_body(model, &wav, language))
        .send()
        .await
        .context("Could not reach OpenRouter")?;
    let status = response.status();
    if matches!(status.as_u16(), 401 | 403) {
        bail!("OpenRouter rejected the API key ({})", status.as_u16());
    }
    let bytes = crate::openrouter::read_capped(response, MAX_RESPONSE_BYTES).await?;
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    ensure!(
        status.is_success(),
        "OpenRouter returned HTTP {}{}",
        status.as_u16(),
        body["error"]["message"]
            .as_str()
            .map(|m| format!(": {m}"))
            .unwrap_or_default()
    );
    response_text(&body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn request_uses_input_audio_wav() {
        let body = request_body("google/test", b"RIFFdata", "de");
        assert_eq!(body["model"], "google/test");
        let parts = body["messages"][0]["content"].as_array().unwrap();
        assert_eq!(parts[1]["type"], "input_audio");
        assert_eq!(parts[1]["input_audio"]["format"], "wav");
        assert_eq!(parts[1]["input_audio"]["data"], "UklGRmRhdGE=");
        assert!(parts[0]["text"].as_str().unwrap().contains("\"de\""));
        let auto = request_body("m", b"", "auto");
        assert!(auto["messages"][0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("the language spoken"));
    }

    #[test]
    fn response_text_reads_strings_parts_and_errors() {
        let text = json!({"choices": [{"message": {"content": " Fix the bug. \n"}}]});
        assert_eq!(response_text(&text).unwrap(), "Fix the bug.");
        let parts = json!({"choices": [{"message": {"content": [{"type": "text", "text": "a"}, {"type": "text", "text": "b"}]}}]});
        assert_eq!(response_text(&parts).unwrap(), "ab");
        let error = json!({"error": {"message": "No credits"}});
        assert!(response_text(&error)
            .unwrap_err()
            .to_string()
            .contains("No credits"));
        assert!(response_text(&json!({})).is_err());
    }

    /// Against a one-shot loopback server standing in for OpenRouter.
    #[tokio::test]
    async fn transcribe_posts_audio_to_chat_completions() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/api/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buf = [0u8; 65_536];
            loop {
                let n = socket.read(&mut buf).await.unwrap();
                request.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&request);
                if let Some(head_end) = text.find("\r\n\r\n") {
                    let length: usize = text[..head_end]
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .map(|v| v.trim().parse().unwrap())
                        })
                        .unwrap_or(0);
                    if request.len() >= head_end + 4 + length {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            let reply = r#"{"choices":[{"message":{"content":"hello world"}}]}"#;
            socket
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}",
                        reply.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            String::from_utf8_lossy(&request).into_owned()
        });
        let text = transcribe(
            &base,
            "sk-test",
            "google/test",
            &vec![0.1; 48_000],
            48_000,
            "en",
        )
        .await
        .unwrap();
        assert_eq!(text, "hello world");
        let request = server.await.unwrap();
        assert!(request.starts_with("POST /api/v1/chat/completions"));
        assert!(request
            .to_ascii_lowercase()
            .contains("authorization: bearer sk-test"));
        assert!(request.contains("\"input_audio\""));
    }
}
