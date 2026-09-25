//! `/api/voice/*`: dictation into the composer. Settings › Voice (engine,
//! model installs, language), and push-to-talk: start, a cheap level/preview
//! poll, stop-and-transcribe, cancel. `transcribe` takes a WAV the caller
//! already has (tests, and clients whose microphone is elsewhere).
use super::*;
use crate::voice::{self, models, VoiceConfig};
use base64::Engine as _;

#[derive(Default, Deserialize)]
#[serde(default)]
struct ModelBody {
    model: Text,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct AudioBody {
    /// Base64 WAV.
    audio: Text,
}

/// Settings to change; fields left out keep their value. Strict like the
/// code-intelligence settings: a mistyped setting is an error.
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct SettingsBody {
    engine: Option<String>,
    model: Option<String>,
    language: Option<String>,
    openrouter_model: Option<String>,
    voice_commands: Option<bool>,
    live_preview: Option<bool>,
    max_seconds: Option<u32>,
}

impl SettingsBody {
    fn apply(self, config: &mut VoiceConfig) {
        macro_rules! set {
            ($($field:ident),*) => {$(
                if let Some(value) = self.$field {
                    config.$field = value;
                }
            )*};
        }
        set!(
            engine,
            model,
            language,
            openrouter_model,
            voice_commands,
            live_preview,
            max_seconds
        );
    }
}

/// Largest WAV `/api/voice/transcribe` accepts (about three minutes of
/// 16 kHz mono).
const MAX_WAV_BYTES: usize = 6_000_000;

fn save_voice(paths: &AppPaths, body: SettingsBody) -> Result<VoiceConfig> {
    let mut saved = None;
    Config::update(paths, |config| {
        let mut voice = VoiceConfig::lenient(config);
        body.apply(&mut voice);
        voice.validate()?;
        config
            .extra
            .insert("voice".into(), serde_json::to_value(&voice)?);
        saved = Some(voice);
        Ok(())
    })?;
    saved.context("Settings were not saved")
}

impl Service {
    pub(super) async fn voice_routes(&self, call: &Arc<Call>) -> Result<Value> {
        match (call.method.as_str(), call.path.as_str()) {
            // Polled several times a second while listening: no config read.
            ("GET", "/api/voice/recording") => Ok(voice::recording()),
            ("GET", "/api/voice/status") => self.blocking(call, |s, _| s.voice_status()).await,
            ("POST", "/api/voice/config") => {
                let body: SettingsBody = serde_json::from_value(call.body.clone())
                    .map_err(|e| anyhow::anyhow!("Invalid voice settings: {e}"))?;
                let paths = self.engine.paths().clone();
                let next = tokio::task::spawn_blocking(move || save_voice(&paths, body))
                    .await
                    .context("Settings worker stopped")??;
                Ok(json!({"ok": true, "config": next}))
            }
            ("POST", "/api/voice/models/install") => {
                let config = self.config()?;
                ensure!(
                    !config.offline(),
                    "ShadowCode is in offline mode; switch the network mode to online to download a voice model"
                );
                let body: ModelBody = call.body()?;
                let model = models::entry(body.model.as_str()).context("Unknown voice model")?;
                let started = models::start_install(voice::data_dir(self.engine.paths()), model);
                Ok(json!({"ok": true, "started": started}))
            }
            ("POST", "/api/voice/models/remove") => {
                let body: ModelBody = call.body()?;
                let model = models::entry(body.model.as_str()).context("Unknown voice model")?;
                let dir = voice::data_dir(self.engine.paths());
                let removed = tokio::task::spawn_blocking(move || models::remove(&dir, model))
                    .await
                    .context("Voice worker stopped")??;
                Ok(json!({"ok": true, "removed": removed}))
            }
            ("POST", "/api/voice/start") => {
                let config = self.config()?;
                let paths = self.engine.paths().clone();
                tokio::task::spawn_blocking(move || voice::start(&paths, &config))
                    .await
                    .context("Voice worker stopped")?
            }
            ("POST", "/api/voice/stop") => voice::stop().await,
            ("POST", "/api/voice/cancel") => {
                Ok(json!({"ok": true, "cancelled": voice::cancel().await?}))
            }
            ("POST", "/api/voice/transcribe") => {
                let body: AudioBody = call.body()?;
                ensure!(
                    body.audio.as_str().len() <= MAX_WAV_BYTES / 3 * 4 + 4,
                    "The recording is too long to transcribe at once"
                );
                let wav = base64::engine::general_purpose::STANDARD
                    .decode(body.audio.as_str().trim())
                    .context("audio must be a base64 WAV file")?;
                let (mono, rate) = voice::audio::decode_wav(&wav)?;
                let config = self.config()?;
                let settings = VoiceConfig::lenient(&config);
                let engine = voice::engine(self.engine.paths(), &config, &settings)?;
                voice::transcribe(engine, mono, rate, settings.voice_commands).await
            }
            _ => Err(call.unavailable()),
        }
    }

    fn voice_status(&self) -> Result<Value> {
        let config = self.config()?;
        let settings = VoiceConfig::lenient(&config);
        let paths = self.engine.paths();
        let dir = voice::data_dir(paths);
        let models: Vec<Value> = models::CATALOG
            .iter()
            .map(|model| {
                json!({
                    "id": model.id,
                    "name": model.name,
                    "bytes": model.bytes,
                    "license": model.license,
                    "english_only": model.english_only,
                    "summary": model.summary,
                    "installed": models::installed(&dir, model),
                    "active": settings.model == model.id,
                    "progress": models::progress(model.id),
                })
            })
            .collect();
        // Whether the chosen engine can run now, and if not, why.
        let ready = voice::engine(paths, &config, &settings);
        Ok(json!({
            "config": settings,
            "models": models,
            "languages": voice::LANGUAGES
                .iter()
                .map(|(code, name)| json!({"code": code, "name": name}))
                .collect::<Vec<_>>(),
            "ready": ready.is_ok(),
            "blocked": ready.err().map(|e| e.to_string()),
            "cpu_supported": voice::whisper::cpu_supported(),
            "whisper_version": voice::whisper::version(),
            "openrouter_key": crate::openrouter::key(paths).is_some(),
            "offline": config.offline(),
            "recording": voice::is_recording(),
        }))
    }
}
