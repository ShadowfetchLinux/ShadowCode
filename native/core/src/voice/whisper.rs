//! Local transcription with whisper.cpp (through whisper-rs), CPU only.
//!
//! One model stays loaded after first use so the next dictation starts at
//! once; removing the model or choosing another one drops it. Runs are
//! serialised by the same lock, and a run can be cut short through its abort
//! flag (the live preview uses this when the user stops talking).
use anyhow::{ensure, Context, Result};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, Once,
    },
};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

struct Loaded {
    path: PathBuf,
    context: WhisperContext,
}

static LOADED: Mutex<Option<Loaded>> = Mutex::new(None);
static QUIET: Once = Once::new();

/// whisper.cpp is built for x86-64 with AVX2, FMA and F16C (see
/// `.cargo/config.toml`); older CPUs would crash inside it, so they get a
/// clear message instead.
pub fn cpu_supported() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        std::arch::is_x86_feature_detected!("avx2")
            && std::arch::is_x86_feature_detected!("fma")
            && std::arch::is_x86_feature_detected!("f16c")
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        true
    }
}

pub const CPU_MESSAGE: &str = "Local dictation needs a processor with AVX2 (most made since 2013). Choose OpenRouter as the voice engine instead.";

/// The whisper.cpp revision compiled in.
pub fn version() -> &'static str {
    whisper_rs::WHISPER_CPP_VERSION
}

/// Drop the loaded model if it is `path`.
pub fn unload(path: &Path) {
    if let Ok(mut loaded) = LOADED.lock() {
        if loaded.as_ref().is_some_and(|l| l.path == path) {
            *loaded = None;
        }
    }
}

/// Segments more likely than this to be silence are dropped.
const NO_SPEECH: f32 = 0.6;

pub fn threads() -> i32 {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 8) as i32
}

/// Transcribe 16 kHz mono samples. `language` is a code such as "en", or
/// "auto" to detect. Blocking: call from a worker thread.
pub fn transcribe(
    model: &Path,
    samples: &[f32],
    language: &str,
    abort: Option<Arc<AtomicBool>>,
) -> Result<String> {
    ensure!(cpu_supported(), CPU_MESSAGE);
    QUIET.call_once(whisper_rs::install_logging_hooks);
    let mut loaded = LOADED
        .lock()
        .map_err(|_| anyhow::anyhow!("The voice engine stopped; restart ShadowCode"))?;
    if loaded.as_ref().is_none_or(|l| l.path != model) {
        *loaded = None;
        let path = model
            .to_str()
            .context("The model path is not valid UTF-8")?;
        let mut params = WhisperContextParameters::default();
        params.use_gpu(false);
        let context = WhisperContext::new_with_params(path, params)
            .map_err(|e| anyhow::anyhow!("Could not load the voice model: {e}"))?;
        *loaded = Some(Loaded {
            path: model.to_owned(),
            context,
        });
    }
    let context = &loaded.as_ref().context("No voice model is loaded")?.context;
    let mut state = context
        .create_state()
        .map_err(|e| anyhow::anyhow!("Could not start the voice model: {e}"))?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    let language = if context.is_multilingual() {
        language
    } else {
        "en"
    };
    params.set_language(Some(language));
    params.set_n_threads(threads());
    params.set_translate(false);
    params.set_no_context(true);
    params.set_no_timestamps(true);
    params.set_suppress_blank(true);
    params.set_no_speech_thold(NO_SPEECH);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    if let Some(flag) = abort.clone() {
        params.set_abort_callback_safe(move || flag.load(Ordering::Relaxed));
    }
    let result = state.full(params, samples);
    if abort.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
        anyhow::bail!("Stopped");
    }
    result.map_err(|e| anyhow::anyhow!("Transcription failed: {e}"))?;
    let mut text = String::new();
    for segment in state.as_iter() {
        // Whisper invents short words ("you", "Thank you.") for silence and
        // noise; it also says how likely a segment is to be no speech.
        if segment.no_speech_probability() > NO_SPEECH {
            continue;
        }
        if let Ok(part) = segment.to_str_lossy() {
            text.push_str(&part);
        }
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs the real model when `SHADOWCODE_WHISPER_TEST_MODEL` points at a
    /// ggml tiny/base model (kept in a scratch directory, never in the repo),
    /// optionally with `SHADOWCODE_WHISPER_TEST_WAV` for a speech clip:
    /// `cargo test -p shadowcode-core --offline voice::whisper -- --ignored`
    #[test]
    #[ignore = "needs a whisper model file"]
    fn transcribes_with_a_local_model() {
        let model = PathBuf::from(
            std::env::var("SHADOWCODE_WHISPER_TEST_MODEL")
                .expect("set SHADOWCODE_WHISPER_TEST_MODEL"),
        );
        // Silence never reaches whisper (see `audio::has_speech`); this only
        // checks that a run on it completes.
        let silence = vec![0.0f32; 16_000 * 2];
        let text = transcribe(&model, &silence, "en", None).unwrap();
        eprintln!("silence: {text:?}");
        if let Ok(wav) = std::env::var("SHADOWCODE_WHISPER_TEST_WAV") {
            let (mono, rate) =
                super::super::audio::decode_wav(&std::fs::read(wav).unwrap()).unwrap();
            let samples = super::super::audio::for_whisper(&mono, rate);
            let started = std::time::Instant::now();
            let text = transcribe(&model, &samples, "en", None).unwrap();
            eprintln!("{:?} in {:?}", text, started.elapsed());
            assert!(super::super::audio::has_speech(&mono, rate));
            assert!(!super::super::text::clean(&text).is_empty());
        }
        // An abort flag that is already set stops the run.
        let stop = Arc::new(AtomicBool::new(true));
        assert!(transcribe(&model, &silence, "en", Some(stop)).is_err());
        unload(&model);
        assert!(LOADED.lock().unwrap().is_none());
    }
}
