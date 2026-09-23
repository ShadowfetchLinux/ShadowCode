//! Official sign-in and sign-out for subscription CLIs.
//!
//! Connect runs the vendor's own login command (`codex login`,
//! `claude auth login`, `cursor-agent login`, `grok login`) as a supervised
//! child with the user's environment (minus provider API keys), so the vendor
//! opens the browser or prints a URL / device code itself. ShadowCode never
//! sees a password or token: it only relays the lines the CLI prints,
//! redacted, as `account.login {vendor, line, url}` events, and reports the
//! exit as `account.login.done {vendor, ok, detail}`. One login per vendor
//! runs at a time; it can be cancelled and stops after `LOGIN_TIMEOUT`.
//!
//! Disconnect runs the official logout command, which signs the CLI out for
//! the whole user account (shared with terminal use), then forgets cached
//! status, persisted usage, and stored native session ids for that vendor.
use super::{catalog::VendorCatalog, clip, redact, resolve_binary, CliAgentsConfig, Vendor};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::Path,
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::sync::CancellationToken;

pub const LOGIN_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const LOGOUT_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_LOGIN_LINES: usize = 200;

#[derive(Default)]
struct LoginSession {
    started_at: f64,
    lines: Vec<Value>,
    done: Option<Value>,
    cancel: CancellationToken,
}

/// In-memory login sessions, one per vendor. Dropping it (engine shutdown)
/// cancels every running login, which stops the login child.
#[derive(Default)]
pub struct Logins {
    sessions: Mutex<HashMap<Vendor, LoginSession>>,
}

impl Logins {
    fn begin(&self, vendor: Vendor) -> Option<CancellationToken> {
        let mut sessions = self.sessions.lock().ok()?;
        if sessions.get(&vendor).is_some_and(|s| s.done.is_none()) {
            return None;
        }
        let cancel = CancellationToken::new();
        sessions.insert(
            vendor,
            LoginSession {
                started_at: crate::now(),
                cancel: cancel.clone(),
                ..Default::default()
            },
        );
        Some(cancel)
    }
    fn push(&self, vendor: Vendor, line: Value) {
        if let Ok(mut sessions) = self.sessions.lock() {
            if let Some(session) = sessions.get_mut(&vendor) {
                if session.lines.len() >= MAX_LOGIN_LINES {
                    session.lines.remove(0);
                }
                session.lines.push(line);
            }
        }
    }
    fn finish(&self, vendor: Vendor, done: Value) {
        if let Ok(mut sessions) = self.sessions.lock() {
            if let Some(session) = sessions.get_mut(&vendor) {
                session.done = Some(done);
            }
        }
    }
    /// `{running, started_at, lines, done}` for the Accounts page.
    pub fn status(&self, vendor: Vendor) -> Value {
        let Ok(sessions) = self.sessions.lock() else {
            return json!({"running":false});
        };
        match sessions.get(&vendor) {
            Some(session) => json!({
                "vendor": vendor.id(),
                "running": session.done.is_none(),
                "started_at": session.started_at,
                "lines": session.lines,
                "done": session.done,
            }),
            None => json!({"vendor":vendor.id(),"running":false,"lines":[],"done":null}),
        }
    }
    pub fn running(&self, vendor: Vendor) -> bool {
        self.sessions
            .lock()
            .map(|s| s.get(&vendor).is_some_and(|s| s.done.is_none()))
            .unwrap_or(false)
    }
    /// Cancel a running login. Returns false when none was running.
    pub fn cancel(&self, vendor: Vendor) -> bool {
        let Ok(sessions) = self.sessions.lock() else {
            return false;
        };
        match sessions.get(&vendor) {
            Some(session) if session.done.is_none() => {
                session.cancel.cancel();
                true
            }
            _ => false,
        }
    }
    pub fn cancel_all(&self) {
        if let Ok(sessions) = self.sessions.lock() {
            for session in sessions.values() {
                session.cancel.cancel();
            }
        }
    }
}

impl Drop for Logins {
    fn drop(&mut self) {
        self.cancel_all();
    }
}

/// First https URL in a printed line, unless it carries a credential-looking
/// query parameter (an OAuth callback with `code=` or a token).
pub fn login_url(line: &str) -> Option<String> {
    let start = line.find("https://")?;
    let url: String = line[start..]
        .chars()
        .take_while(|c| !c.is_whitespace() && !matches!(c, '"' | '\'' | '<' | '>' | ')'))
        .collect();
    let parsed = reqwest::Url::parse(url.trim_end_matches(['.', ','])).ok()?;
    let sensitive = parsed.query_pairs().any(|(key, _)| {
        matches!(
            key.to_ascii_lowercase().as_str(),
            "code" | "token" | "access_token" | "id_token" | "refresh_token" | "api_key"
        )
    });
    (!sensitive).then(|| parsed.to_string())
}

fn login_command(binary: &Path, args: &[&str]) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(binary);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .env("NO_COLOR", "1");
    // The user's environment (browser, display, config dirs) so the vendor
    // flow works as in a terminal, minus provider API keys.
    // No separate process group: a browser the CLI launches must survive
    // when the login child is stopped.
    super::scrub_api_keys(&mut command);
    #[cfg(target_os = "linux")]
    unsafe {
        command.pre_exec(|| {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command
}

/// Start the official login flow. Returns immediately with
/// `{ok, state: started|already_running|unsupported, note, hint?}`.
pub async fn connect(
    catalog: &Arc<VendorCatalog>,
    vendor: Vendor,
    config: &CliAgentsConfig,
) -> Result<Value> {
    if vendor.login_command().is_empty() {
        return Ok(json!({
            "ok": false,
            "state": "unsupported",
            "note": "Antigravity has no sign-in command ShadowCode can run.",
            "hint": "Run `agy` once in a terminal and sign in there, then press Refresh.",
        }));
    }
    if !config.vendor_enabled(vendor) {
        bail!(
            "{} is disabled in Settings › Advanced",
            vendor.product_label()
        );
    }
    let configured = config.binary(vendor);
    let binary = resolve_binary(configured)
        .with_context(|| format!("`{configured}` was not found. {}", vendor.install_hint()))?;
    let Some(cancel) = catalog.logins().begin(vendor) else {
        return Ok(json!({
            "ok": true,
            "state": "already_running",
            "note": format!("A {} sign-in is already in progress.", vendor.product_label()),
        }));
    };
    let mut child = match login_command(&binary, vendor.login_command()).spawn() {
        Ok(child) => child,
        Err(error) => {
            let detail = format!("Could not start `{}`: {error}", binary.display());
            catalog
                .logins()
                .finish(vendor, json!({"ok":false,"detail":detail}));
            bail!(detail);
        }
    };
    let command_line = format!("{} {}", vendor.binary(), vendor.login_command().join(" "));
    let catalog = catalog.clone();
    let config = config.clone();
    tokio::spawn(async move {
        let (sender, mut receiver) = tokio::sync::mpsc::channel::<String>(64);
        for stream in [
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Send + Unpin>),
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Send + Unpin>),
        ]
        .into_iter()
        .flatten()
        {
            let sender = sender.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stream).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if sender.send(line).await.is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);
        let deadline = tokio::time::sleep(LOGIN_TIMEOUT);
        tokio::pin!(deadline);
        let relay = |line: String| {
            let raw = line.trim();
            if raw.is_empty() {
                return;
            }
            let text = clip(&redact(raw), 2000);
            let payload = json!({"vendor":vendor.id(),"line":text,"url":login_url(raw)});
            catalog.logins().push(vendor, payload.clone());
            catalog.broadcast("account.login", payload);
        };
        let outcome = loop {
            tokio::select! {
                Some(line) = receiver.recv() => relay(line),
                status = child.wait() => {
                    // A browser started by the CLI may keep the pipes open;
                    // take what is already buffered, briefly, then finish.
                    while let Ok(Some(line)) =
                        tokio::time::timeout(Duration::from_millis(300), receiver.recv()).await
                    {
                        relay(line);
                    }
                    break match status {
                        Ok(status) if status.success() => (true, format!("`{command_line}` finished")),
                        Ok(status) => (false, format!("`{command_line}` exited with status {}", status.code().unwrap_or(-1))),
                        Err(error) => (false, format!("`{command_line}` failed: {error}")),
                    };
                }
                _ = cancel.cancelled() => {
                    let _ = child.kill().await;
                    break (false, "Sign-in cancelled".to_owned());
                }
                _ = &mut deadline => {
                    let _ = child.kill().await;
                    break (false, format!("Sign-in timed out after {} minutes", LOGIN_TIMEOUT.as_secs() / 60));
                }
            }
        };
        // Re-probe: the login state and the account (and thus usage) may
        // have changed.
        catalog.forget_status(vendor).await;
        let status = catalog.refresh(vendor, &config, true).await;
        let done = json!({
            "vendor": vendor.id(),
            "ok": outcome.0,
            "detail": outcome.1,
            "availability": status.availability,
            "availability_label": status.availability.label(),
        });
        catalog.logins().finish(vendor, done.clone());
        catalog.broadcast("account.login.done", done);
    });
    Ok(json!({
        "ok": true,
        "state": "started",
        "note": format!("Running `{}`. Finish signing in with {} in the browser window or with the code it shows.", vendor.login_command().iter().fold(vendor.binary().to_owned(), |a, b| format!("{a} {b}")), vendor.product_label()),
    }))
}

/// Run the official logout command after the user confirmed the shared-CLI
/// note, then forget everything cached for the vendor and re-probe.
pub async fn disconnect(
    catalog: &Arc<VendorCatalog>,
    vendor: Vendor,
    config: &CliAgentsConfig,
) -> Result<Value> {
    if vendor.logout_command().is_empty() {
        return Ok(json!({
            "ok": false,
            "ran": [],
            "note": vendor.shared_cli_note(),
        }));
    }
    let configured = config.binary(vendor);
    let binary = resolve_binary(configured)
        .with_context(|| format!("`{configured}` was not found. {}", vendor.install_hint()))?;
    catalog.logins().cancel(vendor);
    let mut command = login_command(&binary, vendor.logout_command());
    let child = command
        .spawn()
        .with_context(|| format!("Could not start `{}`", binary.display()))?;
    // `wait_with_output` owns the child; on timeout it is dropped, and
    // `kill_on_drop` stops it.
    let output = match tokio::time::timeout(LOGOUT_TIMEOUT, child.wait_with_output()).await {
        Ok(output) => output?,
        Err(_) => {
            bail!(
                "`{} {}` did not finish within {} seconds",
                vendor.binary(),
                vendor.logout_command().join(" "),
                LOGOUT_TIMEOUT.as_secs()
            );
        }
    };
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    catalog.forget(vendor).await?;
    let status = catalog.refresh(vendor, config, true).await;
    let ran: Vec<String> = std::iter::once(vendor.binary().to_owned())
        .chain(vendor.logout_command().iter().map(|s| s.to_string()))
        .collect();
    Ok(json!({
        "ok": output.status.success(),
        "ran": ran,
        "output": clip(&redact(text.trim()), 2000),
        "note": vendor.shared_cli_note(),
        "availability": status.availability,
        "availability_label": status.availability.label(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_urls_are_kept_but_callbacks_with_codes_are_not() {
        assert_eq!(
            login_url("Open https://auth.example.com/authorize?client_id=x&code_challenge=abc to continue.").as_deref(),
            Some("https://auth.example.com/authorize?client_id=x&code_challenge=abc")
        );
        assert!(login_url("http://localhost:1455/callback?code=secret").is_none());
        assert!(login_url("https://localhost/cb?code=secret").is_none());
        assert!(login_url("no url here").is_none());
    }
}
