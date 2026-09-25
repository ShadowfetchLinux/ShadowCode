//! Which enabled project MCP servers a vendor CLI run receives.
//!
//! Only servers the user enabled for this project (the same activation grant
//! the native loop uses, with an unchanged definition) are shared, and only
//! for trusted projects outside read-only mode. Servers that need a stored
//! secret (`env_refs`, `api_key_env`) or carry literal environment values
//! stay with the native loop: vendor command lines and session payloads must
//! not carry credentials. `mcp.share_with_cli_agents: false` turns sharing off.
use super::registry;
use crate::{
    cli_agent::McpServerSpec,
    config::{Config, PermissionLevel},
    workspace::Workspace,
};

/// The servers to pass, and a note for each enabled server left out.
pub fn plan(workspace: &Workspace, config: &Config) -> (Vec<McpServerSpec>, Vec<String>) {
    let mut servers = Vec::new();
    let mut skipped = Vec::new();
    if config.mcp.get("share_with_cli_agents") == Some(&serde_json::Value::Bool(false))
        || config.permissions.level == PermissionLevel::ReadOnly
        || !config.is_trusted(&workspace.path)
    {
        return (servers, skipped);
    }
    let Ok(grants) = registry::activations(config) else {
        return (servers, skipped);
    };
    for grant in grants
        .into_iter()
        .filter(|a| a.workspace == workspace.path.to_string_lossy())
        .take(4)
    {
        let entry = match registry::read(workspace, config, &grant.server) {
            Ok(entry) if entry.hash == grant.hash => entry,
            Ok(_) => {
                skipped.push(format!(
                    "{}: definition changed since it was enabled",
                    grant.server
                ));
                continue;
            }
            Err(error) => {
                skipped.push(format!("{}: {error:#}", grant.server));
                continue;
            }
        };
        if registry::authorize_start(workspace, config, &entry).is_err() {
            skipped.push(format!("{}: not allowed by current permissions", entry.id));
            continue;
        }
        let definition = &entry.definition;
        if !definition.env.is_empty()
            || !definition.env_refs.is_empty()
            || definition.api_key_env.is_some()
        {
            skipped.push(format!(
                "{}: uses stored secrets or environment values, so it stays with ShadowCode's own agent",
                definition.name
            ));
            continue;
        }
        servers.push(McpServerSpec {
            name: definition.name.clone(),
            command: definition.command.clone().unwrap_or_default(),
            url: definition.url.clone().unwrap_or_default(),
        });
    }
    (servers, skipped)
}

pub fn servers(workspace: &Workspace, config: &Config) -> Vec<McpServerSpec> {
    plan(workspace, config).0
}
