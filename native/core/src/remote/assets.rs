//! The web interface's static files.
//!
//! The desktop binary embeds the built UI (`ui/dist`) and registers it here
//! at startup with [`set_bundled`], so `shadowcode serve` and the in-app
//! toggle serve exactly the interface the window runs. Tests and development
//! builds can serve a folder instead ([`DirAssets`]).
//!
//! Request paths are checked before any lookup: only plain relative names
//! made of safe characters, with no `.`/`..` segments, hidden files or
//! backslashes, reach a provider, and the folder provider also refuses
//! anything that resolves outside its root (symlinks included).
use std::{
    borrow::Cow,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

/// A source of built UI files, keyed by `/`-rooted paths (`/index.html`).
pub trait UiAssets: Send + Sync + 'static {
    fn get(&self, path: &str) -> Option<Cow<'_, [u8]>>;
}

static BUNDLED: OnceLock<Arc<dyn UiAssets>> = OnceLock::new();

/// Register the UI embedded in this binary. Later calls are ignored.
pub fn set_bundled(assets: Arc<dyn UiAssets>) {
    let _ = BUNDLED.set(assets);
}

pub fn bundled() -> Option<Arc<dyn UiAssets>> {
    BUNDLED.get().cloned()
}

/// Files under one folder (for tests and development).
pub struct DirAssets {
    root: PathBuf,
}
impl DirAssets {
    pub fn new(root: impl Into<PathBuf>) -> std::io::Result<Self> {
        Ok(Self {
            root: root.into().canonicalize()?,
        })
    }
}
impl UiAssets for DirAssets {
    fn get(&self, path: &str) -> Option<Cow<'_, [u8]>> {
        let relative = path.strip_prefix('/')?;
        let candidate = self.root.join(relative).canonicalize().ok()?;
        if !candidate.starts_with(&self.root) || !candidate.is_file() {
            return None;
        }
        let metadata = std::fs::metadata(&candidate).ok()?;
        if metadata.len() > 32 * 1024 * 1024 {
            return None;
        }
        std::fs::read(candidate).ok().map(Cow::Owned)
    }
}

/// The asset key for a request path, or `None` when the path is not a safe,
/// plain file name. `/` maps to `/index.html`.
pub fn asset_key(path: &str) -> Option<String> {
    if path == "/" || path.is_empty() {
        return Some("/index.html".into());
    }
    let relative = path.strip_prefix('/')?;
    if relative.len() > 256 {
        return None;
    }
    for segment in relative.split('/') {
        if segment.is_empty()
            || segment.starts_with('.')
            || !segment
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        {
            return None;
        }
    }
    Some(format!("/{relative}"))
}

pub fn content_type(key: &str) -> &'static str {
    let extension = Path::new(key)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match extension {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "webmanifest" => "application/manifest+json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Hashed build output can be cached for good; everything else is re-read.
pub fn immutable(key: &str) -> bool {
    key.starts_with("/assets/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traversal_and_hidden_paths_are_refused() {
        assert_eq!(asset_key("/").as_deref(), Some("/index.html"));
        assert_eq!(
            asset_key("/assets/index-abc.js").as_deref(),
            Some("/assets/index-abc.js")
        );
        for bad in [
            "/../secrets.env",
            "/assets/../../etc/passwd",
            "/./index.html",
            "/.env",
            "/assets//x.js",
            "/assets\\..\\x",
            "/%2e%2e/secrets.env",
            "/a b.js",
            "relative.js",
            "/assets/",
        ] {
            assert!(asset_key(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn folder_provider_stays_inside_its_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("dist");
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(root.join("index.html"), "<html></html>").unwrap();
        std::fs::write(dir.path().join("secret.txt"), "private").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.path().join("secret.txt"), root.join("assets/link.txt"))
            .unwrap();
        let assets = DirAssets::new(&root).unwrap();
        assert_eq!(
            assets.get("/index.html").unwrap().as_ref(),
            b"<html></html>"
        );
        assert!(assets.get("/../secret.txt").is_none());
        assert!(assets.get("/assets/link.txt").is_none());
        assert!(assets.get("/assets").is_none());
        assert_eq!(
            content_type("/manifest.webmanifest"),
            "application/manifest+json"
        );
        assert!(immutable("/assets/index-abc.js") && !immutable("/index.html"));
    }
}
