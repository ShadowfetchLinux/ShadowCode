//! `/api/code-intel/*`: language server status and managed installs, the
//! code index, search, the repo map, and optional embedding models.
use super::*;
use crate::code_intel::{embeddings, repo_map, search, CodeIntelConfig};
use crate::lsp;

impl Service {
    pub(super) async fn code_intel(
        &self,
        method: &str,
        path: &str,
        query: &HashMap<String, String>,
        body: &Value,
    ) -> Result<Value> {
        let config = self.config()?;
        let data_dir = crate::code_intel::data_dir(self.engine.paths());
        let managed = lsp::managed_dir(&data_dir);
        let workspace = self.workspace()?;
        let text = |key: &str| body[key].as_str().unwrap_or("").to_owned();
        match (method, path) {
            ("GET", "/api/code-intel/status") => {
                self.code_intel_status(&config, &data_dir, &workspace).await
            }
            ("POST", "/api/code-intel/config") => {
                ensure!(body.is_object(), "Send the code_intel settings to change");
                let mut current = serde_json::to_value(CodeIntelConfig::lenient(&config))?;
                for (key, value) in body.as_object().into_iter().flatten() {
                    current[key] = value.clone();
                }
                let next: CodeIntelConfig = serde_json::from_value(current)
                    .map_err(|e| anyhow::anyhow!("Invalid code_intel settings: {e}"))?;
                next.validate()?;
                Config::update(self.engine.paths(), |config| {
                    config
                        .extra
                        .insert("code_intel".into(), serde_json::to_value(&next)?);
                    Ok(())
                })?;
                if !next.lsp {
                    lsp::pool().stop_all(None).await;
                }
                Ok(json!({"ok": true, "config": next}))
            }
            ("POST", "/api/code-intel/install") => {
                ensure!(
                    !config.offline(),
                    "ShadowCode is in offline mode; switch the network mode to online to download a language server"
                );
                let package = lsp::install::package(&text("package"))
                    .context("Choose typescript or python")?;
                let started = lsp::install::start(managed.clone(), package)?;
                Ok(
                    json!({"ok": true, "started": started, "managed": lsp::install::status(&managed)}),
                )
            }
            ("POST", "/api/code-intel/uninstall") => {
                let package = lsp::install::package(&text("package"))
                    .context("Choose typescript or python")?;
                let removed = lsp::install::remove(&managed, package).await?;
                Ok(
                    json!({"ok": true, "removed": removed, "managed": lsp::install::status(&managed)}),
                )
            }
            ("POST", "/api/code-intel/servers/stop") => {
                let stopped = lsp::pool().stop_all(None).await;
                let embedding = embeddings::stop().await;
                Ok(json!({"ok": true, "stopped": stopped, "embedding_server_stopped": embedding}))
            }
            ("POST", "/api/code-intel/embeddings/install") => {
                ensure!(
                    !config.offline(),
                    "ShadowCode is in offline mode; switch the network mode to online to download a model"
                );
                let entry =
                    embeddings::catalog_entry(&text("model")).context("Unknown embedding model")?;
                let paths = self.engine.paths().clone();
                let (dir, root) = (data_dir.clone(), workspace.clone());
                let started = embeddings::start_install(data_dir.clone(), entry, move |result| {
                    if result.is_err() {
                        return;
                    }
                    // Use the new model unless another one was chosen, then
                    // embed the open project in the background.
                    let _ = Config::update(&paths, |config| {
                        let mut intel = CodeIntelConfig::lenient(config);
                        if intel.embedding_model.is_empty() {
                            intel.embedding_model = entry.id.into();
                            config
                                .extra
                                .insert("code_intel".into(), serde_json::to_value(&intel)?);
                        }
                        Ok(())
                    });
                    if let Ok(config) = Config::load(&paths, Some(&root)) {
                        if let Some(semantic) = embeddings::Semantic::resolve(&dir, &config) {
                            embeddings::spawn_backfill(root, semantic);
                        }
                    }
                });
                Ok(
                    json!({"ok": true, "started": started, "models": embeddings::catalog_json(&data_dir, &CodeIntelConfig::lenient(&config))}),
                )
            }
            ("POST", "/api/code-intel/embeddings/remove") => {
                let entry =
                    embeddings::catalog_entry(&text("model")).context("Unknown embedding model")?;
                embeddings::stop().await;
                let removed = embeddings::remove(&data_dir, entry)?;
                Config::update(self.engine.paths(), |config| {
                    let mut intel = CodeIntelConfig::lenient(config);
                    if intel.embedding_model == entry.id {
                        intel.embedding_model.clear();
                        config
                            .extra
                            .insert("code_intel".into(), serde_json::to_value(&intel)?);
                    }
                    Ok(())
                })?;
                let config = self.config()?;
                Ok(
                    json!({"ok": true, "removed": removed, "models": embeddings::catalog_json(&data_dir, &CodeIntelConfig::lenient(&config))}),
                )
            }
            ("POST", "/api/code-intel/reindex") => {
                let root = workspace.clone();
                let stats = tokio::task::spawn_blocking(move || {
                    crate::symbol_index::ensure_index(&root, &[], true)?;
                    crate::symbol_index::stats(&root)
                })
                .await
                .context("Index worker stopped")??;
                let semantic = embeddings::Semantic::resolve(&data_dir, &config);
                let embedding = semantic
                    .map(|semantic| embeddings::spawn_backfill(workspace.clone(), semantic))
                    .unwrap_or(false);
                Ok(json!({"ok": true, "index": stats, "embedding_started": embedding}))
            }
            ("POST", "/api/code-intel/search") => {
                let request = search::Request {
                    query: text("query"),
                    path: text("path"),
                    max_hits: body["max_hits"].as_u64().unwrap_or(10) as usize,
                };
                let semantic = embeddings::Semantic::resolve(&data_dir, &config);
                search::search_code(workspace, request, semantic).await
            }
            ("GET", "/api/code-intel/repo-map") => {
                let tokens = query
                    .get("tokens")
                    .and_then(|t| t.parse::<usize>().ok())
                    .unwrap_or(1024);
                let task = query.get("query").cloned().unwrap_or_default();
                let root = workspace.clone();
                tokio::task::spawn_blocking(move || {
                    let focus = crate::symbol_index::recent_edits(&root, 12);
                    repo_map::build_json(&root, &focus, &task, tokens)
                })
                .await
                .context("Index worker stopped")?
            }
            _ => bail!("Unknown code intelligence command: {method} {path}"),
        }
    }

    async fn code_intel_status(
        &self,
        config: &Config,
        data_dir: &Path,
        workspace: &Path,
    ) -> Result<Value> {
        let (intel, config_error) = match CodeIntelConfig::from_config(config) {
            Ok(intel) => (intel, None),
            Err(error) => (CodeIntelConfig::default(), Some(format!("{error:#}"))),
        };
        let managed = lsp::managed_dir(data_dir);
        let (languages, managed_status, npm, node) = {
            let (intel, managed) = (intel.clone(), managed.clone());
            tokio::task::spawn_blocking(move || {
                (
                    lsp::languages(&intel, Some(&managed)),
                    lsp::install::status(&managed),
                    lsp::install::npm(),
                    lsp::servers::find_program("node"),
                )
            })
            .await
            .context("Status worker stopped")?
        };
        let root = workspace.to_owned();
        let index = tokio::task::spawn_blocking(move || crate::symbol_index::stats(&root))
            .await
            .context("Status worker stopped")?
            .unwrap_or(Value::Null);
        let active = embeddings::active(data_dir, &intel);
        let coverage = match active {
            Some(entry) => {
                let (root, dir) = (workspace.to_owned(), data_dir.to_owned());
                tokio::task::spawn_blocking(move || embeddings::coverage(&root, &dir, entry))
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .map(|(embedded, chunks)| json!({"embedded": embedded, "chunks": chunks}))
            }
            None => None,
        };
        let runtime = crate::local_engine::runtime_candidates(&config.local_engine.llama_binary)
            .into_iter()
            .next()
            .map(|(path, _)| path);
        Ok(json!({
            "config": intel,
            "config_error": config_error,
            "offline": config.offline(),
            "languages": languages,
            "servers": lsp::pool().status(),
            "managed": managed_status,
            "managed_dir": managed,
            "npm": {"available": npm.is_some(), "path": npm, "node": node},
            "index": index,
            "embeddings": {
                "models": embeddings::catalog_json(data_dir, &intel),
                "active": active.map(|m| m.id),
                "runtime": runtime,
                "server": embeddings::running().await,
                "coverage": coverage,
                "backfill": embeddings::backfill_status(workspace),
            },
        }))
    }
}
