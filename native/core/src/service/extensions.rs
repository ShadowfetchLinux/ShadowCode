//! `/api/plugins…`, `/api/mcp…`, `/api/hooks…` and `/api/sqlite`: project
//! extensions. Installing or enabling one always names the project and the
//! reviewed content hash, so a stale screen cannot change another project.
use super::*;

#[derive(Default, Deserialize)]
#[serde(default)]
struct ActivationBody {
    workspace: Text,
    server: Text,
    path: Text,
    name: Text,
    hash: Text,
    enabled: Flag,
}

impl Service {
    pub(super) async fn extension_routes(&self, call: &Arc<Call>) -> Result<Value> {
        #[cfg(unix)]
        if (call.method.as_str(), call.path.as_str()) == ("POST", "/api/sqlite") {
            return crate::sqlite::inspect(
                Arc::new(Workspace::open(&self.workspace()?)?),
                serde_json::from_value(call.body.clone())?,
                CancellationToken::new(),
            )
            .await;
        }
        self.blocking(call, Self::extension_routes_sync).await
    }
    fn extension_routes_sync(&self, call: &Call) -> Result<Value> {
        let store = self.engine.store();
        let body: ActivationBody = call.body()?;
        let path = call.path.as_str();
        match (call.method.as_str(), path) {
            #[cfg(unix)]
            ("GET", "/api/plugins") => {
                crate::plugins::catalog(self.engine.paths(), &Workspace::open(&self.workspace()?)?)
            }
            #[cfg(unix)]
            ("POST", "/api/plugins/preview") => {
                crate::plugins::preview(&crate::plugins::resolve(&call.body)?)
            }
            #[cfg(unix)]
            ("POST", "/api/plugins/install" | "/api/plugins/remove") => {
                let workspace = self.mutable_workspace()?;
                ensure!(
                    body.workspace.as_str() == workspace.path.to_string_lossy(),
                    "Project changed; refresh plugins before changing an installation"
                );
                let removing = path.ends_with("/remove");
                let result = if removing {
                    crate::plugins::remove(
                        self.engine.paths(),
                        &workspace,
                        body.name.as_str(),
                        body.hash.as_str(),
                    )?
                } else {
                    crate::plugins::install(
                        self.engine.paths(),
                        &workspace,
                        crate::plugins::resolve(&call.body)?,
                        body.hash.as_str(),
                    )?
                };
                store.add_event(
                    "plugin.installation",
                    &json!({"workspace":workspace.path,"removed":removing,"result":result}),
                    None,
                    None,
                )?;
                Ok(
                    json!({"result":result,"catalog":crate::plugins::catalog(self.engine.paths(), &workspace)?}),
                )
            }
            #[cfg(unix)]
            ("GET", "/api/mcp/servers") => {
                let workspace = Workspace::open(&self.workspace()?)?;
                let config = Config::load(self.engine.paths(), Some(&workspace.path))?;
                crate::mcp::registry::catalog(&workspace, &config)
            }
            #[cfg(unix)]
            ("POST", "/api/mcp/servers" | "/api/mcp/servers/delete") => {
                let workspace = Workspace::open(&self.workspace()?)?;
                let deleting = path.ends_with("/delete");
                let config = Config::update(self.engine.paths(), |config| {
                    if deleting {
                        crate::mcp::registry::remove_server(
                            config,
                            body.server.as_str(),
                            body.hash.as_str(),
                        )?;
                    } else {
                        crate::mcp::registry::save_server(
                            config,
                            call.body["definition"].clone(),
                            body.hash.as_str(),
                        )?;
                    }
                    Ok(())
                })?;
                let server = if deleting {
                    body.server.as_str().to_owned()
                } else {
                    format!(
                        "config:{}",
                        call.body["definition"]["name"].as_str().unwrap_or("")
                    )
                };
                store.add_event(
                    "mcp.registration",
                    &json!({"workspace":workspace.path,"server":server,"removed":deleting}),
                    None,
                    None,
                )?;
                crate::mcp::registry::catalog(&workspace, &config)
            }
            #[cfg(unix)]
            ("POST", "/api/mcp/activation") => {
                let workspace = Workspace::open(&self.workspace()?)?;
                ensure!(
                    body.workspace.as_str() == workspace.path.to_string_lossy(),
                    "Project changed; reload MCP servers before enabling one"
                );
                let enabled = body
                    .enabled
                    .0
                    .context("Choose whether to enable this MCP server")?;
                let server = body.server.as_str();
                let config = Config::update(self.engine.paths(), |config| {
                    let effective = Config::load(self.engine.paths(), Some(&workspace.path))?;
                    if enabled {
                        let entry = crate::mcp::registry::read(&workspace, &effective, server)?;
                        crate::mcp::registry::authorize_start(&workspace, &effective, &entry)?;
                    }
                    crate::mcp::registry::activate(
                        &workspace,
                        config,
                        server,
                        body.hash.as_str(),
                        enabled,
                    )?;
                    Ok(())
                })?;
                store.add_event("mcp.activation", &json!({"workspace":workspace.path,"server":server,"hash":body.hash.as_str(),"enabled":enabled}), None, None)?;
                crate::mcp::registry::catalog(&workspace, &config)
            }
            ("GET", "/api/hooks") => {
                let workspace = Workspace::open(&self.workspace()?)?;
                let config = Config::load(self.engine.paths(), Some(&workspace.path))?;
                Ok(crate::hooks::catalog(&workspace, &config))
            }
            ("POST", "/api/hooks/activation") => {
                let workspace = Workspace::open(&self.workspace()?)?;
                ensure!(
                    body.workspace.as_str() == workspace.path.to_string_lossy(),
                    "Project changed; reload hooks before enabling a command"
                );
                let enabled = body
                    .enabled
                    .0
                    .context("Choose whether to enable this hook")?;
                let config = Config::update(self.engine.paths(), |config| {
                    let effective = Config::load(self.engine.paths(), Some(&workspace.path))?;
                    // A project read-only overlay remains authoritative at activation.
                    if enabled {
                        ensure!(
                            effective.permissions.level != PermissionLevel::ReadOnly,
                            "Lifecycle commands cannot be enabled in read-only mode"
                        );
                    }
                    crate::hooks::activate(
                        &workspace,
                        config,
                        body.path.as_str(),
                        body.hash.as_str(),
                        enabled,
                    )?;
                    Ok(())
                })?;
                store.add_event("hook.activation",&json!({"workspace":workspace.path,"path":body.path.as_str(),"hash":body.hash.as_str(),"enabled":enabled}),None,None)?;
                Ok(crate::hooks::catalog(&workspace, &config))
            }
            _ => Err(call.unavailable()),
        }
    }
}
