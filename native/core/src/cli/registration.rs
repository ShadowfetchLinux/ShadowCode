//! Inert client configuration. No secret lookup, shell command, configuration
//! write, or server connection occurs while rendering a registration.
use super::args::McpClient;
use anyhow::{ensure, Context, Result};
use serde_json::json;
use std::path::Path;

fn toml_string(value: &str) -> Result<String> {
    // JSON basic-string escaping is also valid TOML here, except that TOML
    // forbids a literal DEL control character.
    Ok(serde_json::to_string(value)?.replace('\u{7f}', "\\u007F"))
}

pub(super) fn stdio(client: McpClient, executable: &Path, args: &[String]) -> Result<String> {
    let executable = executable
        .to_str()
        .context("MCP executable path must be UTF-8")?;
    if client == McpClient::Codex {
        let args = args
            .iter()
            .map(|arg| toml_string(arg))
            .collect::<Result<Vec<_>>>()?;
        return Ok(format!(
            "[mcp_servers.shadowcode]\ncommand = {}\nargs = [{}]",
            toml_string(executable)?,
            args.join(", ")
        ));
    }
    if matches!(client, McpClient::Claude | McpClient::Cursor) {
        ensure!(!executable.contains("${") && args.iter().all(|arg| !arg.contains("${")),
            "This client expands ${{...}} in paths; choose an executable/project/profile path without that sequence");
    }
    let mut server = json!({"command":executable,"args":args});
    if client != McpClient::Generic {
        server["type"] = json!("stdio");
    }
    Ok(serde_json::to_string_pretty(
        &json!({"mcpServers":{"shadowcode":server}}),
    )?)
}

pub(super) fn http(client: McpClient, url: &str, token_env: &str) -> Result<String> {
    ensure!(
        client != McpClient::Generic,
        "HTTP credential syntax depends on the client; select --client claude, cursor, or codex"
    );
    ensure!(
        crate::config::valid_secret_name(token_env),
        "Invalid bearer-secret reference"
    );
    let url = crate::mcp::http::validate_url(url)?;
    ensure!(
        url.scheme() == "http"
            && crate::mcp::http::loopback(&url)
            && url.port_or_known_default().is_some_and(|port| port != 0)
            && url.path() == "/mcp"
            && url.query().is_none(),
        "Use the running local gateway's http://loopback:port/mcp URL without query parameters"
    );
    if client == McpClient::Codex {
        return Ok(format!(
            "[mcp_servers.shadowcode]\nurl = {}\nbearer_token_env_var = {}",
            toml_string(url.as_str())?,
            toml_string(token_env)?
        ));
    }
    let token = if client == McpClient::Cursor {
        format!("${{env:{token_env}}}")
    } else {
        format!("${{{token_env}}}")
    };
    let mut server =
        json!({"url":url.as_str(),"headers":{"Authorization":format!("Bearer {token}")}});
    if client == McpClient::Claude {
        server["type"] = json!("http");
    }
    Ok(serde_json::to_string_pretty(
        &json!({"mcpServers":{"shadowcode":server}}),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn client_configurations_round_trip_literal_paths_without_injection() {
        let path = "/tmp/space \\\"\n[mcp_servers.evil]\nx = \\\"雪\u{7f}";
        let args = vec![
            "--workspace".into(),
            path.into(),
            "mcp".into(),
            "serve".into(),
        ];
        let text = stdio(McpClient::Codex, Path::new(path), &args).unwrap();
        let parsed: toml::Value = toml::from_str(&text).unwrap();
        assert_eq!(parsed["mcp_servers"].as_table().unwrap().len(), 1);
        let server = &parsed["mcp_servers"]["shadowcode"];
        assert_eq!(server["command"].as_str().unwrap(), path);
        assert_eq!(server["args"][1].as_str().unwrap(), path);
        for client in [McpClient::Generic, McpClient::Claude, McpClient::Cursor] {
            let parsed: Value =
                serde_json::from_str(&stdio(client, Path::new(path), &args).unwrap()).unwrap();
            assert_eq!(parsed["mcpServers"]["shadowcode"]["command"], path);
            assert_eq!(parsed["mcpServers"]["shadowcode"]["args"][1], path);
        }
        for client in [McpClient::Claude, McpClient::Cursor] {
            assert!(stdio(client, Path::new("/tmp/${EXEC}/app"), &[]).is_err());
            assert!(stdio(client, Path::new("/app"), &["${WORKSPACE}".into()]).is_err());
        }
    }

    #[test]
    fn http_outputs_client_specific_references_and_rejects_ambiguous_endpoints() {
        let url = "http://127.0.0.1:8765/mcp";
        for (client, expected) in [
            (McpClient::Claude, "Bearer ${SHADOW_MCP_HTTP_TOKEN}"),
            (McpClient::Cursor, "Bearer ${env:SHADOW_MCP_HTTP_TOKEN}"),
        ] {
            let config: Value =
                serde_json::from_str(&http(client, url, "SHADOW_MCP_HTTP_TOKEN").unwrap()).unwrap();
            assert_eq!(
                config["mcpServers"]["shadowcode"]["headers"]["Authorization"],
                expected
            );
        }
        let config: toml::Value =
            toml::from_str(&http(McpClient::Codex, url, "SHADOW_MCP_HTTP_TOKEN").unwrap()).unwrap();
        assert_eq!(
            config["mcp_servers"]["shadowcode"]["bearer_token_env_var"].as_str(),
            Some("SHADOW_MCP_HTTP_TOKEN")
        );
        assert!(http(McpClient::Generic, url, "TOKEN").is_err());
        for value in [
            "http://evil.example:8765/mcp",
            "http://127.0.0.1:0/mcp",
            "http://127.0.0.1:8765/sse",
            "http://127.0.0.1:8765/mcp?token=x",
            "http://secret@127.0.0.1/mcp",
            "http://127.0.0.1/mcp#x",
            "https://127.0.0.1/mcp",
        ] {
            assert!(http(McpClient::Codex, value, "TOKEN").is_err(), "{value}");
        }
        assert!(http(McpClient::Codex, url, "TOKEN\nsecret").is_err());
        assert!(http(McpClient::Codex, "http://[::1]:8765/mcp", "TOKEN").is_ok());
    }
}
