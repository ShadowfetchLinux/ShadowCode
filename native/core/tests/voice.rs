//! Voice input routes: Settings › Voice status and settings, model installs
//! (refused offline), clear messages before recording when no model or key
//! is set, and `/api/voice/transcribe` through the OpenRouter engine against
//! a loopback stand-in (no real network, no microphone, no model download).
use base64::Engine as _;
use serde_json::{json, Value};
use shadowcode_core::{
    config::{self, Config},
    paths::AppPaths,
    service::{Request, Service},
    voice,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn dispatch(
    service: &Service,
    method: &str,
    path: &str,
    body: Value,
) -> anyhow::Result<Value> {
    service
        .dispatch(Request {
            method: method.into(),
            path: path.into(),
            body,
        })
        .await
}

fn open(offline: bool) -> (tempfile::TempDir, AppPaths, Service) {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let paths = AppPaths::isolated(&root.path().join("profile")).unwrap();
    if offline {
        Config::patch(&paths, json!({"network": {"mode": "offline"}})).unwrap();
    }
    let service = Service::open(paths.clone(), Some(project)).unwrap();
    (root, paths, service)
}

fn env_key() -> bool {
    std::env::var("OPENROUTER_API_KEY").is_ok_and(|k| !k.is_empty())
}

fn wav_base64(samples: &[f32], rate: u32) -> String {
    base64::engine::general_purpose::STANDARD.encode(voice::audio::encode_wav(samples, rate))
}

fn speech_like(rate: u32, seconds: f32) -> Vec<f32> {
    (0..(rate as f32 * seconds) as usize)
        .map(|i| (i as f32 * 220.0 * std::f32::consts::TAU / rate as f32).sin() * 0.3)
        .collect()
}

#[tokio::test]
async fn status_lists_models_and_installs_refuse_offline() {
    let (_root, paths, service) = open(true);
    let status = dispatch(&service, "GET", "/api/voice/status", json!({}))
        .await
        .unwrap();
    let ids: Vec<&str> = status["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["base.en", "tiny.en", "base"]);
    assert!(status["models"]
        .as_array()
        .unwrap()
        .iter()
        .all(|m| m["installed"] == false && m["bytes"].as_u64().unwrap() > 70_000_000));
    assert_eq!(status["config"]["engine"], "local");
    assert_eq!(status["offline"], true);
    assert_eq!(status["ready"], false);
    assert_eq!(status["recording"], false);
    assert!(status["languages"].as_array().unwrap().len() > 10);
    if status["cpu_supported"] == true {
        assert!(status["blocked"]
            .as_str()
            .unwrap()
            .contains("No voice model is installed"));
    }
    let refused = dispatch(
        &service,
        "POST",
        "/api/voice/models/install",
        json!({"model": "tiny.en"}),
    )
    .await
    .unwrap_err();
    assert!(format!("{refused:#}").contains("offline"), "{refused:#}");
    assert!(!voice::data_dir(&paths).join("models").exists());
    // Removing a model that is not there is not an error; an unknown id is.
    let removed = dispatch(
        &service,
        "POST",
        "/api/voice/models/remove",
        json!({"model": "tiny.en"}),
    )
    .await
    .unwrap();
    assert_eq!(removed["removed"], false);
    assert!(dispatch(
        &service,
        "POST",
        "/api/voice/models/remove",
        json!({"model": "large"})
    )
    .await
    .is_err());
}

#[tokio::test]
async fn settings_are_validated_and_saved() {
    let (_root, paths, service) = open(false);
    let saved = dispatch(
        &service,
        "POST",
        "/api/voice/config",
        json!({"model": "base", "language": "de", "voice_commands": false}),
    )
    .await
    .unwrap();
    assert_eq!(saved["config"]["language"], "de");
    let config = Config::load(&paths, None).unwrap();
    assert_eq!(config.extra["voice"]["model"], "base");
    assert_eq!(config.extra["voice"]["voice_commands"], false);
    assert_eq!(config.extra["voice"]["engine"], "local");
    for bad in [
        json!({"engine": "cloud"}),
        json!({"language": "German"}),
        json!({"max_seconds": 0}),
        json!({"modle": "base"}),
        json!({"live_preview": "yes"}),
    ] {
        assert!(
            dispatch(&service, "POST", "/api/voice/config", bad.clone())
                .await
                .is_err(),
            "{bad} accepted"
        );
    }
    // Unchanged by the rejected requests.
    let status = dispatch(&service, "GET", "/api/voice/status", json!({}))
        .await
        .unwrap();
    assert_eq!(status["config"]["model"], "base");
    assert_eq!(status["config"]["max_seconds"], 120);
}

#[tokio::test]
async fn start_explains_what_is_missing_before_opening_the_microphone() {
    let (_root, _paths, service) = open(false);
    if voice::whisper::cpu_supported() {
        let error = dispatch(&service, "POST", "/api/voice/start", json!({}))
            .await
            .unwrap_err();
        assert!(
            format!("{error:#}").contains("No voice model is installed"),
            "{error:#}"
        );
    }
    dispatch(
        &service,
        "POST",
        "/api/voice/config",
        json!({"engine": "openrouter"}),
    )
    .await
    .unwrap();
    if !env_key() {
        let error = dispatch(&service, "POST", "/api/voice/start", json!({}))
            .await
            .unwrap_err();
        assert!(
            format!("{error:#}").contains("no OpenRouter API key"),
            "{error:#}"
        );
        let status = dispatch(&service, "GET", "/api/voice/status", json!({}))
            .await
            .unwrap();
        assert_eq!(status["openrouter_key"], false);
        assert_eq!(status["ready"], false);
    }
    let recording = dispatch(&service, "GET", "/api/voice/recording", json!({}))
        .await
        .unwrap();
    assert_eq!(recording["active"], false);
    let error = dispatch(&service, "POST", "/api/voice/stop", json!({}))
        .await
        .unwrap_err();
    assert!(format!("{error:#}").contains("Not listening"));
    let cancelled = dispatch(&service, "POST", "/api/voice/cancel", json!({}))
        .await
        .unwrap();
    assert_eq!(cancelled["cancelled"], false);
}

#[tokio::test]
async fn openrouter_engine_is_refused_offline() {
    let (_root, paths, service) = open(true);
    config::set_secret(&paths, "OPENROUTER_API_KEY", "sk-or-test").unwrap();
    dispatch(
        &service,
        "POST",
        "/api/voice/config",
        json!({"engine": "openrouter"}),
    )
    .await
    .unwrap();
    for (path, body) in [
        ("/api/voice/start", json!({})),
        (
            "/api/voice/transcribe",
            json!({"audio": wav_base64(&speech_like(16_000, 1.0), 16_000)}),
        ),
    ] {
        let error = dispatch(&service, "POST", path, body).await.unwrap_err();
        assert!(format!("{error:#}").contains("offline mode"), "{error:#}");
    }
}

#[tokio::test]
async fn transcribe_route_checks_audio_and_skips_silence() {
    let (_root, paths, service) = open(false);
    let error = dispatch(
        &service,
        "POST",
        "/api/voice/transcribe",
        json!({"audio": "not base64!"}),
    )
    .await
    .unwrap_err();
    assert!(format!("{error:#}").contains("base64"));
    let error = dispatch(
        &service,
        "POST",
        "/api/voice/transcribe",
        json!({"audio": base64::engine::general_purpose::STANDARD.encode(b"hello")}),
    )
    .await
    .unwrap_err();
    assert!(format!("{error:#}").contains("WAV"));
    if !voice::whisper::cpu_supported() {
        return;
    }
    // A placeholder of the right size counts as installed; silence never
    // reaches whisper, so the placeholder is never loaded.
    let model = voice::models::entry("base.en").unwrap();
    let path = voice::models::model_path(&voice::data_dir(&paths), model);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::File::create(&path)
        .unwrap()
        .set_len(model.bytes)
        .unwrap();
    let result = dispatch(
        &service,
        "POST",
        "/api/voice/transcribe",
        json!({"audio": wav_base64(&vec![0.0; 32_000], 16_000)}),
    )
    .await
    .unwrap();
    assert_eq!(result["text"], "");
    assert_eq!(result["engine"], "local");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("Nothing was heard"));
    let status = dispatch(&service, "GET", "/api/voice/status", json!({}))
        .await
        .unwrap();
    assert_eq!(status["ready"], true);
    assert_eq!(status["models"][0]["installed"], true);
    assert_eq!(status["models"][0]["active"], true);
    // Remove deletes the file.
    dispatch(
        &service,
        "POST",
        "/api/voice/models/remove",
        json!({"model": "base.en"}),
    )
    .await
    .unwrap();
    assert!(!path.exists());
}

/// The OpenRouter engine posts the clip as `input_audio` and applies voice
/// commands to the reply.
#[tokio::test]
async fn transcribe_route_uses_openrouter_only_when_chosen() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/api/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buf = [0u8; 65_536];
        loop {
            let n = socket.read(&mut buf).await.unwrap();
            request.extend_from_slice(&buf[..n]);
            let text = String::from_utf8_lossy(&request).into_owned();
            if let Some(end) = text.find("\r\n\r\n") {
                let length: usize = text[..end]
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .map(|v| v.trim().parse().unwrap())
                    })
                    .unwrap_or(0);
                if request.len() >= end + 4 + length || n == 0 {
                    break;
                }
            }
        }
        let reply = r#"{"choices":[{"message":{"content":"Add a test. New line. Run it."}}]}"#;
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
    std::env::set_var("SHADOWCODE_OPENROUTER_BASE", &base);
    let (_root, paths, service) = open(false);
    config::set_secret(&paths, "OPENROUTER_API_KEY", "sk-or-test").unwrap();
    dispatch(
        &service,
        "POST",
        "/api/voice/config",
        json!({"engine": "openrouter", "openrouter_model": "google/test-audio"}),
    )
    .await
    .unwrap();
    let result = dispatch(
        &service,
        "POST",
        "/api/voice/transcribe",
        json!({"audio": wav_base64(&speech_like(48_000, 1.0), 48_000)}),
    )
    .await
    .unwrap();
    assert_eq!(result["engine"], "openrouter");
    assert_eq!(result["text"], "Add a test.\nRun it.");
    let request = server.await.unwrap();
    assert!(request.starts_with("POST /api/v1/chat/completions"));
    assert!(request.contains("\"google/test-audio\""));
    assert!(request.contains("\"input_audio\""));
}
