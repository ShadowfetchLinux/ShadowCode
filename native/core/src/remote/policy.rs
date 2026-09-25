//! What a remote client may ask of the application API, and what it sees.
//!
//! Remote clients hold an owner's access token, so they use the same routes
//! as the window, with these exceptions:
//! - `/api/remote…` (tokens, binding, phone notifications) is managed only on
//!   this computer, so a stolen token cannot mint more tokens, widen the
//!   bind address or turn terminals on.
//! - Interactive terminals (`/api/terminals…`) and the direct command runner
//!   (`/api/workspace/exec`) are refused unless the user turned on "Allow
//!   terminals over remote access". Agent shell commands still go through
//!   the usual approvals.
//! - Secret files (`.env`, `secrets.env`, keys…) are not shown, the profile's
//!   own folders cannot be opened as a project, and response bodies pass
//!   through [`redact_response`], which removes recognizable credentials.
//! - A request body that carries a redaction placeholder is refused, so a
//!   redacted value is never written back over the real one.
use crate::{paths::AppPaths, redaction};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub struct Access {
    pub allow_terminals: bool,
}

/// Why a remote request was refused (always HTTP 403).
#[derive(Debug, PartialEq, Eq)]
pub struct Refusal(pub &'static str);

pub const TERMINALS_OFF: &str = "Terminals are turned off for remote access. Turn on \"Allow terminals over remote access\" in Settings › Remote access on the computer running ShadowCode.";
pub const MANAGED_LOCALLY: &str =
    "Remote access is managed in Settings › Remote access on the computer running ShadowCode.";
pub const SECRET_FILE: &str = "Secret files such as .env are not shown over remote access.";
pub const PROFILE_FOLDER: &str =
    "ShadowCode's own settings folder cannot be opened over remote access.";
pub const REDACTED_INPUT: &str = "This text contains a hidden secret. Edit it on the computer running ShadowCode.";
pub const INVALID_PATH: &str = "Invalid application command path";

/// The path's segments after `/api/`, or `None` for a path the router could
/// read differently (encoded characters, empty or dot segments).
pub fn segments(path: &str) -> Option<Vec<&str>> {
    let path = path.split('?').next().unwrap_or(path);
    let rest = path.strip_prefix("/api/")?;
    if path.len() > 4096 || path.contains(['%', '\\', '\0']) {
        return None;
    }
    let parts: Vec<&str> = rest.trim_end_matches('/').split('/').collect();
    if parts
        .iter()
        .any(|p| p.is_empty() || *p == "." || *p == "..")
    {
        return None;
    }
    Some(parts)
}

fn query_value(path: &str, key: &str) -> Option<String> {
    let url = reqwest::Url::parse(&format!("http://remote.local{path}")).ok()?;
    url.query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}

fn contains_placeholder(value: &Value) -> bool {
    match value {
        Value::String(text) => text.contains(redaction::placeholder()),
        Value::Array(items) => items.iter().any(contains_placeholder),
        Value::Object(map) => map.values().any(contains_placeholder),
        _ => false,
    }
}

fn expand(text: &str) -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match (text, home) {
        ("~", Some(home)) => home,
        (t, Some(home)) if t.starts_with("~/") => home.join(&t[2..]),
        (t, _) => PathBuf::from(t),
    }
}

/// True when `candidate` is one of the profile's folders or inside one.
/// Opening a folder that merely contains the profile (the home folder) is
/// allowed; secret files there are still refused by name.
pub fn inside_profile(candidate: &str, paths: &AppPaths) -> bool {
    if candidate.is_empty() {
        return false;
    }
    let resolve = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let candidate = resolve(&expand(candidate));
    [&paths.config, &paths.data, &paths.state]
        .into_iter()
        .any(|root| candidate.starts_with(resolve(root)))
}

/// Every string field named `workspace` (the routes that switch projects),
/// plus `path` for the project routes.
fn project_paths<'a>(parts: &[&str], body: &'a Value) -> Vec<&'a str> {
    let mut found = Vec::new();
    if let Some(workspace) = body["workspace"].as_str() {
        found.push(workspace);
    }
    if parts.first() == Some(&"projects") {
        if let Some(path) = body["path"].as_str() {
            found.push(path);
        }
    }
    found
}

/// Decide one remote request. `path` includes its query string.
pub fn check(
    method: &str,
    path: &str,
    body: &Value,
    access: &Access,
    paths: &AppPaths,
) -> Result<(), Refusal> {
    let parts = segments(path).ok_or(Refusal(INVALID_PATH))?;
    let family = parts.first().copied().unwrap_or("");
    match family {
        "remote" | "views" | "runtime" | "owned-jobs" => return Err(Refusal(MANAGED_LOCALLY)),
        "terminals" if !access.allow_terminals => return Err(Refusal(TERMINALS_OFF)),
        _ => {}
    }
    if parts == ["workspace", "exec"] && !access.allow_terminals {
        return Err(Refusal(TERMINALS_OFF));
    }
    if method == "GET"
        && matches!(parts.as_slice(), ["workspace", "file" | "diff"])
        && query_value(path, "path").is_some_and(|p| redaction::is_secret_path(&p))
    {
        return Err(Refusal(SECRET_FILE));
    }
    if contains_placeholder(body) {
        return Err(Refusal(REDACTED_INPUT));
    }
    if project_paths(&parts, body)
        .into_iter()
        .any(|p| inside_profile(p, paths))
    {
        return Err(Refusal(PROFILE_FOLDER));
    }
    Ok(())
}

/// Remove recognizable credentials from a response a remote client will see.
pub fn redact_response(value: &mut Value) {
    redaction::redact_known_secrets(value);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn paths() -> (tempfile::TempDir, AppPaths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(dir.path()).unwrap();
        (dir, paths)
    }

    #[test]
    fn terminals_and_exec_need_the_explicit_switch() {
        let (_dir, paths) = paths();
        let off = Access {
            allow_terminals: false,
        };
        let on = Access {
            allow_terminals: true,
        };
        for (method, path) in [
            ("GET", "/api/terminals"),
            ("POST", "/api/terminals"),
            ("POST", "/api/terminals/abc/input"),
            ("GET", "/api/terminals/abc/output?cursor=0"),
            ("POST", "/api/workspace/exec"),
        ] {
            assert_eq!(
                check(method, path, &Value::Null, &off, &paths),
                Err(Refusal(TERMINALS_OFF)),
                "{path}"
            );
            assert!(check(method, path, &Value::Null, &on, &paths).is_ok());
        }
        assert!(check("GET", "/api/feed", &Value::Null, &off, &paths).is_ok());
    }

    #[test]
    fn remote_management_and_ambiguous_paths_are_refused() {
        let (_dir, paths) = paths();
        let on = Access {
            allow_terminals: true,
        };
        for path in [
            "/api/remote",
            "/api/remote/pair",
            "/api/remote/devices/revoke",
            "/api/views",
            "/api/owned-jobs",
        ] {
            assert_eq!(
                check("POST", path, &Value::Null, &on, &paths),
                Err(Refusal(MANAGED_LOCALLY))
            );
        }
        for path in [
            "/api/%74erminals",
            "/api//terminals",
            "/api/./terminals",
            "/api/sessions/../terminals",
            "/other",
            "/api/",
        ] {
            assert_eq!(
                check("GET", path, &Value::Null, &on, &paths),
                Err(Refusal(INVALID_PATH)),
                "{path}"
            );
        }
    }

    #[test]
    fn secret_files_profile_folders_and_placeholders() {
        let (_dir, paths) = paths();
        let access = Access {
            allow_terminals: false,
        };
        assert_eq!(
            check("GET", "/api/workspace/file?path=.env", &Value::Null, &access, &paths),
            Err(Refusal(SECRET_FILE))
        );
        assert_eq!(
            check(
                "GET",
                "/api/workspace/diff?path=config%2Fsecrets.env",
                &Value::Null,
                &access,
                &paths
            ),
            Err(Refusal(SECRET_FILE))
        );
        assert!(check("GET", "/api/workspace/file?path=src/main.rs", &Value::Null, &access, &paths).is_ok());
        let config = paths.config.to_string_lossy().into_owned();
        assert_eq!(
            check("POST", "/api/projects", &json!({"path": config}), &access, &paths),
            Err(Refusal(PROFILE_FOLDER))
        );
        assert_eq!(
            check(
                "POST",
                "/api/sessions",
                &json!({"workspace": paths.data.join("x").to_string_lossy()}),
                &access,
                &paths
            ),
            Err(Refusal(PROFILE_FOLDER))
        );
        assert!(check("POST", "/api/projects", &json!({"path": "/tmp"}), &access, &paths).is_ok());
        assert_eq!(
            check(
                "PUT",
                "/api/workspace/instructions",
                &json!({"content": format!("key {}", redaction::placeholder())}),
                &access,
                &paths
            ),
            Err(Refusal(REDACTED_INPUT))
        );
    }

    #[test]
    fn responses_lose_credentials_but_keep_ids() {
        let github = format!("{}{}", "ghp_", "abcdefghijklmnopqrstuvwxyz012345");
        let mut value = json!({
            "id": "0f3c9a4e2b7d4e1f8a6b5c3d2e1f0a9b",
            "hash": "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
            "api_key_env": "OPENAI_API_KEY",
            "token": "abc-plain-value",
            "tokens": 1234,
            "mcp": {"servers": [{"env": {"GITHUB_TOKEN": github.clone()}}]},
            "text": format!("pushed with {github}"),
        });
        redact_response(&mut value);
        let text = value.to_string();
        assert!(!text.contains(&github));
        assert!(!text.contains("abc-plain-value"));
        assert_eq!(value["id"], "0f3c9a4e2b7d4e1f8a6b5c3d2e1f0a9b");
        assert_eq!(
            value["hash"],
            "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
        );
        assert_eq!(value["api_key_env"], "OPENAI_API_KEY");
        assert_eq!(value["tokens"], 1234);
    }
}
