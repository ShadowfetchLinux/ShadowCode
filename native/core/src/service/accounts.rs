//! `/api/accounts…`, `/api/cli-agents`, `/api/openrouter…` and
//! `/api/allowance`: vendor CLI subscriptions (connect, login, the
//! Antigravity agent server), the OpenRouter key and what is left to run.
use super::*;
use crate::cli_agent::{antigravity_server, Vendor};

impl Service {
    pub(super) async fn account_routes(&self, call: &Arc<Call>) -> Result<Value> {
        match (call.method.as_str(), call.path.as_str()) {
            ("GET", "/api/allowance") => return self.allowance(call).await,
            ("GET", "/api/openrouter") => {
                let cfg = self.config()?;
                return Ok(
                    crate::openrouter::status(self.engine.paths(), cfg.offline(), true).await,
                );
            }
            ("POST", "/api/openrouter/key") => {
                let cfg = self.config()?;
                let key = call.text("api_key");
                // Removing a key needs no network; checking a new one does.
                ensure!(
                    key.trim().is_empty() || !cfg.offline(),
                    "Offline mode: OpenRouter is off"
                );
                crate::openrouter::save_key(self.engine.paths(), key).await?;
                if !key.trim().is_empty() {
                    // The first save also fetches the model list.
                    let _ = crate::openrouter::refresh(self.engine.paths(), false).await;
                }
                return Ok(
                    crate::openrouter::status(self.engine.paths(), cfg.offline(), true).await,
                );
            }
            ("POST", "/api/openrouter/refresh") => {
                let cfg = self.config()?;
                ensure!(!cfg.offline(), "Offline mode: OpenRouter is off");
                crate::openrouter::refresh(self.engine.paths(), true).await?;
                return Ok(
                    crate::openrouter::status(self.engine.paths(), cfg.offline(), true).await,
                );
            }
            ("GET", "/api/accounts") => {
                let cfg = self.config()?;
                let vendors = self.engine.vendors();
                return Ok(json!({
                    // `cached=1`: what is known without probing (persisted
                    // usage shows "Last checked …" before the first refresh).
                    "vendors": if call.q("cached") == "1" {
                        vendors.status_cached_json().await
                    } else {
                        vendors.status_json(&cfg.cli_agents, call.q("refresh") == "1").await
                    },
                    "config": cfg.cli_agents,
                    "local_engine": crate::local_engine::catalog_with(&cfg.local_engine, Some(self.engine.local_runtime())),
                }));
            }
            ("GET", "/api/cli-agents") => {
                let cfg = self.config()?;
                return Ok(json!({
                    "vendors": self.engine.vendors().status_json(&cfg.cli_agents, call.q("refresh") == "1").await,
                    "config": cfg.cli_agents
                }));
            }
            _ => {}
        }
        let parts = call.parts();
        if call.family() == "accounts" && parts.len() == 4 {
            return self.account_action(call, parts[2], parts[3]).await;
        }
        Err(call.unavailable())
    }
    /// `/api/accounts/<vendor>/<action>`.
    async fn account_action(&self, call: &Call, vendor: &str, action: &str) -> Result<Value> {
        let vendor = Vendor::parse(vendor).with_context(|| format!("Unknown account {vendor}"))?;
        let cfg = self.config()?;
        let catalog = self.engine.vendors();
        let confirmed = call.body["confirm"].as_bool() == Some(true);
        match (call.method.as_str(), action) {
            ("POST", "connect") => {
                crate::cli_agent::auth::connect(&catalog, vendor, &cfg.cli_agents).await
            }
            ("GET", "login") => Ok(catalog.logins().status(vendor)),
            // Antigravity's agent server: install on request, show
            // progress, remove.
            ("GET", "install") if vendor == Vendor::Antigravity => Ok(
                antigravity_server::install_status(cfg.cli_agents.binary(vendor)),
            ),
            ("POST", "install") if vendor == Vendor::Antigravity => {
                ensure!(!cfg.offline(), "Offline mode: downloads are off");
                ensure!(
                    confirmed,
                    "Confirm the {:.0} MB download first",
                    antigravity_server::ARCHIVE_BYTES as f64 / 1e6
                );
                let started = antigravity_server::start_install();
                let mut status = antigravity_server::install_status(cfg.cli_agents.binary(vendor));
                status["started"] = json!(started);
                Ok(status)
            }
            ("POST", "uninstall") if vendor == Vendor::Antigravity => {
                antigravity_server::uninstall()?;
                catalog.forget_status(vendor).await;
                Ok(antigravity_server::install_status(
                    cfg.cli_agents.binary(vendor),
                ))
            }
            ("POST", "cancel-login") => Ok(json!({"ok": catalog.logins().cancel(vendor)})),
            ("POST", "disconnect") => {
                // Logout signs the CLI out everywhere for this user; the
                // UI shows shared_cli_note and sends confirm: true.
                if !confirmed {
                    return Ok(json!({
                        "ok": false,
                        "needs_confirm": true,
                        "ran": [],
                        "note": vendor.shared_cli_note(),
                    }));
                }
                crate::cli_agent::auth::disconnect(&catalog, vendor, &cfg.cli_agents).await
            }
            ("POST", "refresh") => {
                let status = catalog.refresh(vendor, &cfg.cli_agents, true).await;
                Ok(status.to_doctor_json())
            }
            _ => Err(call.unavailable()),
        }
    }
    /// GET /api/allowance: everything the user can run and how much of it is
    /// left, from what each source reports. `refresh=1` re-checks vendor
    /// accounts; otherwise their status is at most 5 minutes old.
    async fn allowance(&self, call: &Call) -> Result<Value> {
        let cfg = self.config()?;
        let workspace = self.workspace()?;
        let vendors = self
            .engine
            .vendors()
            .status_json(&cfg.cli_agents, call.q("refresh") == "1")
            .await;
        let openrouter = crate::openrouter::status(self.engine.paths(), cfg.offline(), true).await;
        let local_cfg = cfg.local_engine.clone();
        let engine = self.engine.clone();
        let local = tokio::task::spawn_blocking(move || {
            crate::local_engine::catalog_with(&local_cfg, Some(engine.local_runtime()))
        })
        .await?;
        let fallback = self
            .engine
            .local_fallback(&cfg, Path::new(&workspace))
            .await?;
        Ok(crate::allowance::build(
            &vendors,
            &openrouter,
            &local,
            fallback
                .as_ref()
                .map(|(id, name)| (id.as_str(), name.as_str())),
            cfg.limits["on_limit"].as_str().unwrap_or("local"),
            crate::now(),
        ))
    }
}
