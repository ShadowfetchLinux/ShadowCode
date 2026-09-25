//! `GET /api/sandbox/status`: what shell isolation this computer and the
//! saved configuration provide (bubblewrap, Landlock, network namespaces).
use super::*;

impl Service {
    pub(super) async fn sandbox_status(&self) -> Result<Value> {
        let config = self.config()?;
        Ok(tokio::task::spawn_blocking(move || crate::sandbox::status(&config)).await?)
    }
}
