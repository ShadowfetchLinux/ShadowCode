//! whisper.cpp models for local dictation. Nothing downloads on its own: a
//! model is fetched only when the user clicks Install in Settings › Voice,
//! from a Hugging Face URL pinned to one commit, and the file must match its
//! recorded size and SHA-256 before it appears under its final name (the
//! same checked download the embedding models use).
use anyhow::Result;
use serde::Serialize;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

#[derive(Clone, Copy, Debug, Serialize)]
pub struct VoiceModel {
    pub id: &'static str,
    pub name: &'static str,
    pub file: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub bytes: u64,
    pub license: &'static str,
    /// Transcribes English only (and is better at it than the same size
    /// multilingual model).
    pub english_only: bool,
    pub summary: &'static str,
}

/// A file in ggerganov/whisper.cpp on Hugging Face at the commit of 2024-10-29.
macro_rules! url {
    ($file:literal) => {
        concat!(
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/",
            $file
        )
    };
}

pub const CATALOG: &[VoiceModel] = &[
    VoiceModel {
        id: "base.en",
        name: "Whisper base (English)",
        file: "ggml-base.en.bin",
        url: url!("ggml-base.en.bin"),
        sha256: "a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002",
        bytes: 147_964_211,
        license: "MIT",
        english_only: true,
        summary: "Recommended. Accurate for English, about a second per sentence on a laptop CPU.",
    },
    VoiceModel {
        id: "tiny.en",
        name: "Whisper tiny (English)",
        file: "ggml-tiny.en.bin",
        url: url!("ggml-tiny.en.bin"),
        sha256: "921e4cf8686fdd993dcd081a5da5b6c365bfde1162e72b08d75ac75289920b1f",
        bytes: 77_704_715,
        license: "MIT",
        english_only: true,
        summary: "Smallest and fastest; makes more mistakes.",
    },
    VoiceModel {
        id: "base",
        name: "Whisper base (multilingual)",
        file: "ggml-base.bin",
        url: url!("ggml-base.bin"),
        sha256: "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
        bytes: 147_951_465,
        license: "MIT",
        english_only: false,
        summary: "About 100 languages. Pick the language below or let it detect one.",
    },
];

pub fn entry(id: &str) -> Option<&'static VoiceModel> {
    CATALOG.iter().find(|m| m.id == id)
}

pub fn model_path(data_dir: &Path, model: &VoiceModel) -> PathBuf {
    data_dir.join("models").join(model.file)
}

pub fn installed(data_dir: &Path, model: &VoiceModel) -> bool {
    std::fs::metadata(model_path(data_dir, model)).is_ok_and(|m| m.len() == model.bytes)
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Progress {
    pub state: String,
    pub done: u64,
    pub total: u64,
    pub error: Option<String>,
}

static PROGRESS: Mutex<Option<HashMap<&'static str, Progress>>> = Mutex::new(None);

fn set_progress(id: &'static str, progress: Progress) {
    if let Ok(mut map) = PROGRESS.lock() {
        map.get_or_insert_with(HashMap::new).insert(id, progress);
    }
}

pub fn progress(id: &str) -> Option<Progress> {
    PROGRESS.lock().ok()?.as_ref()?.get(id).cloned()
}

fn downloading(id: &str) -> bool {
    progress(id).is_some_and(|p| matches!(p.state.as_str(), "downloading" | "verifying"))
}

/// Start a background download. Returns false when one is already running.
/// The caller checks offline mode first.
pub fn start_install(data_dir: PathBuf, model: &'static VoiceModel) -> bool {
    if downloading(model.id) {
        return false;
    }
    let progress = |state: &str, done| Progress {
        state: state.into(),
        done,
        total: model.bytes,
        error: None,
    };
    set_progress(model.id, progress("downloading", 0));
    tokio::spawn(async move {
        let result = crate::code_intel::embeddings::download(
            model.url,
            model.sha256,
            model.bytes,
            &model_path(&data_dir, model),
            |done| set_progress(model.id, progress("downloading", done)),
        )
        .await;
        set_progress(
            model.id,
            match result {
                Ok(()) => progress("installed", model.bytes),
                Err(error) => Progress {
                    error: Some(format!("{error:#}")),
                    ..progress("error", 0)
                },
            },
        );
    });
    true
}

/// Delete an installed model (and unload it if whisper holds it).
pub fn remove(data_dir: &Path, model: &VoiceModel) -> Result<bool> {
    anyhow::ensure!(
        !downloading(model.id),
        "{} is still downloading",
        model.name
    );
    let path = model_path(data_dir, model);
    super::whisper::unload(&path);
    let existed = path.exists();
    if existed {
        std::fs::remove_file(&path)?;
    }
    if let Ok(mut map) = PROGRESS.lock() {
        if let Some(map) = map.as_mut() {
            map.remove(model.id);
        }
    }
    Ok(existed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPO: &str =
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/";

    #[test]
    fn catalog_is_pinned_and_checked() {
        let mut ids = std::collections::HashSet::new();
        for model in CATALOG {
            assert!(ids.insert(model.id), "duplicate id {}", model.id);
            assert!(model.url.starts_with(REPO), "{} is not pinned", model.id);
            assert!(model.url.ends_with(model.file));
            assert_eq!(model.sha256.len(), 64);
            assert!(model
                .sha256
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
            assert!(model.bytes > 50_000_000 && model.bytes < 200_000_000);
            assert_eq!(model.license, "MIT");
            assert_eq!(model.english_only, model.id.ends_with(".en"));
        }
        for id in ["base.en", "tiny.en", "base"] {
            assert!(entry(id).is_some(), "{id} missing");
        }
        assert!(entry("large-v3").is_none());
    }

    #[test]
    fn installed_requires_the_exact_size_and_remove_deletes() {
        let dir = tempfile::tempdir().unwrap();
        let model = entry("tiny.en").unwrap();
        assert!(!installed(dir.path(), model));
        let path = model_path(dir.path(), model);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"partial").unwrap();
        assert!(
            !installed(dir.path(), model),
            "a short file is not installed"
        );
        assert!(remove(dir.path(), model).unwrap());
        assert!(!path.exists());
        assert!(!remove(dir.path(), model).unwrap());
    }
}
