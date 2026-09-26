//! `/api/remote…`: Settings › Remote access. Remote clients can never reach
//! this family (`remote::policy`); it is answered only for the desktop
//! window and local command-line clients.
use super::*;

impl Service {
    /// The `remote` family.
    pub(super) async fn remote_routes(&self, call: &Arc<Call>) -> Result<Value> {
        let manager = self.remote().clone();
        match (call.method.as_str(), call.path.as_str()) {
            ("GET", "/api/remote") => Ok(manager.status()),
            ("PUT", "/api/remote") => {
                let (service, body) = (self.clone(), call.body.clone());
                tokio::task::spawn_blocking(move || manager.configure(&service, &body)).await?
            }
            ("POST", "/api/remote/pair") => manager.pair(Some(call.text("host"))),
            ("POST", "/api/remote/devices/revoke") => {
                let id = Some(call.text("id")).filter(|id| !id.is_empty());
                ensure!(
                    id.is_some() || call.body["all"] == true,
                    "Choose a device, or all devices"
                );
                manager.revoke(id)
            }
            ("PUT", "/api/remote/ntfy") => {
                let body = call.body.clone();
                tokio::task::spawn_blocking(move || manager.set_ntfy(&body)).await?
            }
            ("POST", "/api/remote/ntfy/test") => manager.test_ntfy().await,
            _ => Err(call.unavailable()),
        }
    }
}
