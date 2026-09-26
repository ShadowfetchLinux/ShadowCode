//! The web interface for remote access is the same bundle the window runs:
//! the UI files Tauri embeds in this binary are shared with
//! `shadowcode_core::remote` (for the in-app toggle and `shadowcode serve`)
//! instead of being shipped twice.
use shadowcode_core::remote::assets::{set_bundled, UiAssets};
use std::{borrow::Cow, sync::Arc};
use tauri::{
    utils::assets::{AssetKey, AssetsIter, CspHash},
    App, Assets, Context, Wry,
};

/// One copy of the embedded assets, used by the window and remote access.
struct Shared(Arc<dyn Assets<Wry>>);

impl Assets<Wry> for Shared {
    fn setup(&self, app: &App<Wry>) {
        self.0.setup(app)
    }
    fn get(&self, key: &AssetKey) -> Option<Cow<'_, [u8]>> {
        self.0.get(key)
    }
    fn iter(&self) -> Box<AssetsIter<'_>> {
        self.0.iter()
    }
    fn csp_hashes(&self, html_path: &AssetKey) -> Box<dyn Iterator<Item = CspHash<'_>> + '_> {
        self.0.csp_hashes(html_path)
    }
}

impl UiAssets for Shared {
    fn get(&self, path: &str) -> Option<Cow<'_, [u8]>> {
        Assets::get(self, &AssetKey::from(path))
    }
}

struct Empty;
impl Assets<Wry> for Empty {
    fn get(&self, _: &AssetKey) -> Option<Cow<'_, [u8]>> {
        None
    }
    fn iter(&self) -> Box<AssetsIter<'_>> {
        Box::new(std::iter::empty())
    }
    fn csp_hashes(&self, _: &AssetKey) -> Box<dyn Iterator<Item = CspHash<'_>> + '_> {
        Box::new(std::iter::empty())
    }
}

/// Hand the embedded UI to remote access and keep serving it to the window.
pub fn share(context: &mut Context<Wry>) {
    let embedded: Arc<dyn Assets<Wry>> = Arc::from(context.set_assets(Box::new(Empty)));
    context.set_assets(Box::new(Shared(embedded.clone())));
    set_bundled(Arc::new(Shared(embedded)));
}
