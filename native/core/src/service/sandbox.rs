//! `/api/sandbox…` routes: what shell isolation this computer and the saved
//! configuration provide, and discarding a command's scratch folder. The
//! engine side lives in `crate::sandbox`.
use super::*;

#[derive(Default, Deserialize)]
#[serde(default)]
struct ScratchBody {
    path: Text,
}

impl Service {
    pub(super) async fn sandbox_routes(&self, call: &Arc<Call>) -> Result<Value> {
        match (call.method.as_str(), call.path.as_str()) {
            // Probes bubblewrap and namespaces: blocking work.
            ("GET", "/api/sandbox/status") => self.blocking(call, Self::sandbox_status).await,
            ("POST", "/api/sandbox/discard-scratch") => {
                let body: ScratchBody = call.body()?;
                crate::sandbox::discard_scratch(&PathBuf::from(body.path.as_str()))
            }
            _ => Err(call.unavailable()),
        }
    }
    fn sandbox_status(&self, _: &Call) -> Result<Value> {
        Ok(crate::sandbox::status(&self.config()?))
    }
}
