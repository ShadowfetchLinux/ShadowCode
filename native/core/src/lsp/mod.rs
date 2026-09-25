//! Persistent language servers for the native agent loop.
//!
//! One server per (project, language) is started on first use and kept
//! alive: files the agent reads are opened in it, files it edits are synced
//! with didChange/didSave, and `publishDiagnostics` results are collected.
//! After an edit, errors the edit introduced (not ones already there) are
//! attached to the tool result. Idle servers stop after
//! `code_intel.lsp_idle_minutes`; a crashed server restarts on next use with
//! exponential backoff; at most `code_intel.max_servers` run at once.
pub mod client;
pub mod diagnostics;
pub mod install;
pub mod servers;

use crate::code_intel::CodeIntelConfig;
use anyhow::{bail, Context, Result};
use client::Client;
use serde_json::{json, Value};
use servers::{Launch, ServerLang};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};

const MAX_FILE_BYTES: u64 = 512_000;
const MAX_EDITED_FILES: usize = 8;
const QUIET: Duration = Duration::from_millis(200);
/// A server that ran this long before dying is not counted as crash-looping.
const STABLE_AFTER: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    root: PathBuf,
    lang: ServerLang,
    program: PathBuf,
    args: Vec<String>,
}

#[derive(Default)]
struct Slot {
    client: Option<Arc<Client>>,
    failures: u32,
    retry_at: Option<Instant>,
    last_error: Option<String>,
    name: String,
    starts: u32,
}

pub struct Pool {
    slots: Mutex<HashMap<Key, Arc<tokio::sync::Mutex<Slot>>>>,
    reaper: AtomicBool,
    idle_secs: AtomicU64,
}

impl std::fmt::Debug for Pool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pool").finish_non_exhaustive()
    }
}

impl Default for Pool {
    fn default() -> Self {
        Self::new()
    }
}

pub fn backoff(failures: u32) -> Duration {
    Duration::from_secs((1u64 << failures.min(9)).min(300))
}

static POOL: OnceLock<Pool> = OnceLock::new();

/// The process-wide pool used by the agent's tools.
pub fn pool() -> &'static Pool {
    POOL.get_or_init(Pool::new)
}

impl Pool {
    pub fn new() -> Self {
        Self {
            slots: Mutex::new(HashMap::new()),
            reaper: AtomicBool::new(false),
            idle_secs: AtomicU64::new(600),
        }
    }

    fn slot(&self, key: &Key) -> Arc<tokio::sync::Mutex<Slot>> {
        self.slots
            .lock()
            .map(|mut slots| slots.entry(key.clone()).or_default().clone())
            .unwrap_or_default()
    }

    fn all_slots(&self) -> Vec<(Key, Arc<tokio::sync::Mutex<Slot>>)> {
        self.slots
            .lock()
            .map(|slots| slots.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default()
    }

    /// A running client for this project and server, starting one if needed.
    pub async fn client(
        &self,
        root: &Path,
        launch: &Launch,
        config: &CodeIntelConfig,
    ) -> Result<Arc<Client>> {
        self.idle_secs
            .store(config.lsp_idle_minutes * 60, Ordering::SeqCst);
        let key = Key {
            root: root.to_owned(),
            lang: launch.lang,
            program: launch.program.clone(),
            args: launch.args.clone(),
        };
        let slot = self.slot(&key);
        let mut slot = slot.lock().await;
        slot.name = launch.name.clone();
        if let Some(client) = slot.client.clone() {
            if client.alive() {
                client.touch();
                return Ok(client);
            }
            // Crashed or exited: back off before the next start.
            slot.failures = if client.started.elapsed() > STABLE_AFTER {
                1
            } else {
                slot.failures + 1
            };
            slot.retry_at = Some(Instant::now() + backoff(slot.failures));
            slot.last_error = client.exit_reason();
            slot.client = None;
        }
        if let Some(retry_at) = slot.retry_at {
            let now = Instant::now();
            if now < retry_at {
                bail!(
                    "{} stopped unexpectedly and restarts in {}s{}",
                    launch.name,
                    (retry_at - now).as_secs() + 1,
                    slot.last_error
                        .as_deref()
                        .map(|e| format!(": {}", crate::tools::truncate(e, 400)))
                        .unwrap_or_default()
                );
            }
        }
        self.make_room(&key, config.max_servers).await;
        match Client::start(launch, root).await {
            Ok(client) => {
                slot.client = Some(client.clone());
                slot.retry_at = None;
                slot.starts += 1;
                Ok(client)
            }
            Err(error) => {
                slot.failures += 1;
                slot.retry_at = Some(Instant::now() + backoff(slot.failures));
                slot.last_error = Some(format!("{error:#}"));
                Err(error)
            }
        }
    }

    /// Stop the least recently used servers so a new one fits.
    async fn make_room(&self, keep: &Key, max: usize) {
        let mut running = Vec::new();
        for (key, slot) in self.all_slots() {
            if &key == keep {
                continue;
            }
            // A slot busy starting or checking is in use; leave it.
            let Ok(slot) = slot.try_lock() else {
                continue;
            };
            if let Some(client) = slot.client.as_ref().filter(|c| c.alive()) {
                running.push((client.idle_for(), key));
            }
        }
        let excess = (running.len() + 1).saturating_sub(max);
        running.sort_by_key(|(idle, _)| std::cmp::Reverse(*idle));
        for (_, key) in running.into_iter().take(excess) {
            let slot = self.slot(&key);
            let Ok(mut slot) = slot.try_lock() else {
                continue;
            };
            // Stopped on purpose: not a crash, no backoff.
            if let Some(client) = slot.client.take() {
                client.shutdown().await;
            }
        }
    }

    /// Stop servers idle longer than `code_intel.lsp_idle_minutes`.
    pub async fn stop_idle(&self) -> usize {
        self.stop_idle_after(Duration::from_secs(self.idle_secs.load(Ordering::SeqCst)))
            .await
    }

    pub async fn stop_idle_after(&self, idle: Duration) -> usize {
        let mut stopped = 0;
        for (_, slot) in self.all_slots() {
            let Ok(mut slot) = slot.try_lock() else {
                continue;
            };
            if let Some(client) = slot.client.clone() {
                if client.idle_for() >= idle {
                    client.shutdown().await;
                    slot.client = None;
                    stopped += 1;
                }
            }
        }
        stopped
    }

    /// Stop servers, optionally only those whose program is inside `under`.
    pub async fn stop_all(&self, under: Option<&Path>) -> usize {
        let mut stopped = 0;
        for (key, slot) in self.all_slots() {
            if under.is_some_and(|dir| !key.program.starts_with(dir)) {
                continue;
            }
            let mut slot = slot.lock().await;
            if let Some(client) = slot.client.take() {
                client.shutdown().await;
                stopped += 1;
            }
            slot.retry_at = None;
            slot.failures = 0;
        }
        stopped
    }

    fn start_reaper(&'static self) {
        if self.reaper.swap(true, Ordering::SeqCst) {
            return;
        }
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                handle.spawn(async move {
                    loop {
                        tokio::time::sleep(Duration::from_secs(30)).await;
                        self.stop_idle().await;
                    }
                });
            }
            Err(_) => self.reaper.store(false, Ordering::SeqCst),
        }
    }

    pub fn status(&self) -> Vec<Value> {
        let mut out = Vec::new();
        for (key, slot) in self.all_slots() {
            let Ok(slot) = slot.try_lock() else {
                out.push(json!({"root": key.root, "language": key.lang.key(), "state": "busy"}));
                continue;
            };
            let client = slot.client.as_ref().filter(|c| c.alive());
            let state = match (client, slot.retry_at) {
                (Some(c), _) if c.ready() => "ready",
                (Some(_), _) => "loading",
                (None, Some(t)) if t > Instant::now() => "backoff",
                (None, _) if slot.last_error.is_some() => "stopped",
                (None, _) => "idle",
            };
            out.push(json!({
                "root": key.root,
                "language": key.lang.key(),
                "server": slot.name,
                "program": key.program,
                "state": state,
                "pid": client.and_then(|c| c.pid()),
                "idle_sec": client.map(|c| c.idle_for().as_secs()),
                "starts": slot.starts,
                "failures": slot.failures,
                "last_error": slot.last_error.as_deref().map(|e| crate::tools::truncate(e, 600)),
            }));
        }
        out
    }
}

/// What the LSP helpers need to know about one project.
#[derive(Clone, Debug)]
pub struct Env {
    pub root: PathBuf,
    pub config: CodeIntelConfig,
    /// The managed npm prefix for language servers, when the profile is known.
    pub managed: Option<PathBuf>,
    pub pool: &'static Pool,
}

impl Env {
    pub fn new(root: &Path, config: &crate::config::Config, data_dir: Option<&Path>) -> Self {
        Self {
            root: root.to_owned(),
            config: CodeIntelConfig::lenient(config),
            managed: data_dir.map(managed_dir),
            pool: pool(),
        }
    }
    pub fn with_pool(mut self, pool: &'static Pool) -> Self {
        self.pool = pool;
        self
    }
    fn launch_for(&self, rel: &str) -> Option<(Launch, &'static str)> {
        if !self.config.lsp {
            return None;
        }
        let (lang, language_id) = servers::for_path(Path::new(rel))?;
        let launch = servers::resolve(lang, &self.config, self.managed.as_deref()).ok()?;
        Some((launch, language_id))
    }
    async fn client(&self, launch: &Launch) -> Result<Arc<Client>> {
        self.pool.start_reaper();
        self.pool.client(&self.root, launch, &self.config).await
    }
    /// Like `client`, but gives up waiting at `deadline` while the server
    /// keeps starting in the background for next time. Ok(None) = starting.
    async fn client_within(
        &self,
        launch: &Launch,
        deadline: Instant,
    ) -> Result<Option<Arc<Client>>> {
        self.pool.start_reaper();
        let (pool, root, launch, config) = (
            self.pool,
            self.root.clone(),
            launch.clone(),
            self.config.clone(),
        );
        let start = tokio::spawn(async move { pool.client(&root, &launch, &config).await });
        match tokio::time::timeout_at(deadline.into(), start).await {
            Ok(Ok(result)) => result.map(Some),
            Ok(Err(error)) => Err(anyhow::anyhow!("Language server start failed: {error}")),
            Err(_) => Ok(None),
        }
    }
    fn read(&self, rel: &str) -> Option<String> {
        if crate::redaction::is_secret_path(rel) {
            return None;
        }
        // Symlinks may not lead out of the project.
        let full = self.root.join(rel).canonicalize().ok()?;
        let root = self.root.canonicalize().ok()?;
        if !full.starts_with(&root) {
            return None;
        }
        let metadata = std::fs::metadata(&full).ok()?;
        if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
            return None;
        }
        std::fs::read_to_string(full).ok()
    }
    fn uri(&self, rel: &str) -> String {
        client::uri_for(&self.root.join(rel))
    }
    fn relative(&self, uri: &str) -> Option<String> {
        let path = client::path_for(uri)?;
        Some(
            path.strip_prefix(&self.root)
                .ok()?
                .to_string_lossy()
                .into_owned(),
        )
    }
}

/// Where managed language servers live under ShadowCode's data folder.
pub fn managed_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("language-servers")
}

/// True when a server could handle this file (cheap; does not start one).
pub fn handles(env: &Env, rel: &str) -> bool {
    env.launch_for(rel).is_some()
}

/// Open a file the agent is reading in its server, in the background, so a
/// later edit has a baseline and a warm server.
pub fn warm(env: Env, rel: String) {
    if env.launch_for(&rel).is_none() {
        return;
    }
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    handle.spawn(async move {
        let Some((launch, language_id)) = env.launch_for(&rel) else {
            return;
        };
        let Some(text) = env.read(&rel) else {
            return;
        };
        if let Ok(client) = env.client(&launch).await {
            let _ = client.sync(&env.uri(&rel), language_id, &text, false).await;
        }
    });
}

fn line_of(text: &str, line: usize) -> Option<&str> {
    text.lines().nth(line)
}

/// After an edit: sync each changed file and report errors the edit
/// introduced. `files` holds (relative path, text before the edit; None for
/// a new file). Returns None when no changed file has a language server.
pub async fn check_edits(env: &Env, files: &[(String, Option<String>)]) -> Option<Value> {
    if !env.config.lsp || !env.config.diagnostics_on_edit {
        return None;
    }
    let started = Instant::now();
    let deadline = started + Duration::from_millis(env.config.diagnostics_wait_ms);
    let mut new_errors = Vec::new();
    let mut checked = Vec::new();
    let mut pending = Vec::new();
    let mut unverified = Vec::new();
    let mut unavailable = Vec::new();
    let mut names = Vec::new();
    let mut attempted = false;
    for (rel, before) in files.iter().take(MAX_EDITED_FILES) {
        let Some((launch, language_id)) = env.launch_for(rel) else {
            continue;
        };
        attempted = true;
        let Some(after) = env.read(rel) else {
            continue;
        };
        let client = match env.client_within(&launch, deadline).await {
            Ok(Some(client)) => client,
            Ok(None) => {
                pending.push(json!(rel));
                continue;
            }
            Err(error) => {
                unavailable.push(json!({"path": rel, "reason": crate::tools::truncate(&format!("{error:#}"), 400)}));
                continue;
            }
        };
        if !names.contains(&client.name) {
            names.push(client.name.clone());
        }
        let uri = env.uri(rel);
        if !client.ready() {
            // Still loading the project (rust-analyzer): keep the document in
            // sync, but do not hold the edit up waiting for it.
            let _ = client.sync(&uri, language_id, &after, true).await;
            pending.push(json!(rel));
            continue;
        }
        // Baseline: what the server said about the text before the edit.
        let mut baseline: Option<Vec<Value>> = None;
        if let Some(before) = before {
            if let Some((version, digest)) = client.document(&uri).await {
                if digest == client::digest(before) {
                    baseline = client
                        .published(&uri)
                        .filter(|p| p.version.is_none_or(|v| v == version))
                        .map(|p| p.items);
                }
            }
            if baseline.is_none() && Instant::now() < deadline {
                let seq = client.seq();
                if let Ok((version, sent)) = client.sync(&uri, language_id, before, false).await {
                    let half = Instant::now() + (deadline - Instant::now()) / 2;
                    baseline = if sent || client.pulls() {
                        client.settle(&uri, seq, Some(version), half, QUIET).await
                    } else {
                        client.published(&uri)
                    }
                    .map(|p| p.items);
                }
            }
        } else {
            baseline = Some(Vec::new());
        }
        let others_before = client.all_published();
        let seq = client.seq();
        let (version, sent) = match client.sync(&uri, language_id, &after, true).await {
            Ok(synced) => synced,
            Err(error) => {
                unavailable.push(json!({"path": rel, "reason": format!("{error:#}")}));
                continue;
            }
        };
        let published = if sent || client.pulls() {
            client
                .settle(&uri, seq, Some(version), deadline, QUIET)
                .await
        } else {
            client.published(&uri)
        };
        let Some(published) = published else {
            pending.push(json!(rel));
            continue;
        };
        checked.push(json!(rel));
        match &baseline {
            Some(before) => {
                for diagnostic in diagnostics::new_errors(before, &published.items) {
                    let line = diagnostic["range"]["start"]["line"].as_u64().unwrap_or(0) as usize;
                    new_errors.push(diagnostics::render(rel, diagnostic, line_of(&after, line)));
                }
            }
            None => {
                let errors = published
                    .items
                    .iter()
                    .filter(|d| diagnostics::is_error(d))
                    .count();
                if errors > 0 {
                    unverified.push(json!({"path": rel, "errors": errors}));
                }
            }
        }
        // Other open files the server re-checked because of this edit.
        for (other_uri, other) in client.all_published() {
            if other_uri == uri || other.seq <= seq {
                continue;
            }
            let (Some(previous), Some(other_rel)) =
                (others_before.get(&other_uri), env.relative(&other_uri))
            else {
                continue;
            };
            let text = env.read(&other_rel);
            for diagnostic in diagnostics::new_errors(&previous.items, &other.items) {
                let line = diagnostic["range"]["start"]["line"].as_u64().unwrap_or(0) as usize;
                new_errors.push(diagnostics::render(
                    &other_rel,
                    diagnostic,
                    text.as_deref().and_then(|t| line_of(t, line)),
                ));
            }
        }
    }
    if !attempted {
        return None;
    }
    let truncated = new_errors.len() > diagnostics::MAX_REPORTED;
    new_errors.truncate(diagnostics::MAX_REPORTED);
    let mut out = json!({"new_errors": new_errors, "checked": checked});
    if !names.is_empty() {
        out["servers"] = json!(names);
    }
    if truncated {
        out["truncated"] = json!(true);
    }
    if !pending.is_empty() {
        out["pending"] = json!(pending);
    }
    if !unverified.is_empty() {
        out["unverified"] = json!(unverified);
    }
    if !unavailable.is_empty() {
        out["unavailable"] = json!(unavailable);
    }
    out["note"] = json!(
        if !out["new_errors"].as_array().is_some_and(|a| a.is_empty()) {
            "The language server reports these errors introduced by this edit. Fix them or explain why they are expected."
        } else if !pending.is_empty() {
            "The language server had not finished checking; call get_diagnostics on the file later."
        } else if !unverified.is_empty() {
            "The file has errors, but the server was too slow to tell whether this edit caused them; call get_diagnostics."
        } else if checked.is_empty() {
            "No language server result for this edit."
        } else {
            "The language server reports no new errors from this edit."
        }
    );
    Some(out)
}

/// All current diagnostics for one file from its language server. None when
/// no server handles the file.
pub async fn file_diagnostics(env: &Env, rel: &str, wait: Duration) -> Result<Option<Value>> {
    let Some((lang, _)) = servers::for_path(Path::new(rel)) else {
        return Ok(None);
    };
    let launch = match servers::resolve(lang, &env.config, env.managed.as_deref()) {
        Ok(launch) if env.config.lsp => launch,
        Ok(_) => return Ok(None),
        Err(missing) => {
            return Ok(Some(json!({
                "ok": false,
                "path": rel,
                "error": servers::missing_note(lang, &missing),
            })))
        }
    };
    let (_, language_id) = servers::for_path(Path::new(rel)).context("unsupported file")?;
    let text = env
        .read(rel)
        .context("File is missing, too large, or a secret path")?;
    let client = env.client(&launch).await?;
    let uri = env.uri(rel);
    let seq = client.seq();
    let (version, sent) = client.sync(&uri, language_id, &text, true).await?;
    let deadline = Instant::now() + wait;
    // A server still loading the project would report too little.
    let published = if !client.wait_ready(deadline).await {
        None
    } else {
        match client.published(&uri) {
            Some(p) if !sent && !client.pulls() && p.version.is_none_or(|v| v == version) => {
                Some(p)
            }
            _ => {
                client
                    .settle(&uri, seq, Some(version), deadline, QUIET)
                    .await
            }
        }
    };
    let Some(published) = published else {
        return Ok(Some(json!({
            "ok": false,
            "path": rel,
            "server": client.name,
            "pending": true,
            "error": if client.ready() {
                format!("{} did not report within {}s", client.name, wait.as_secs())
            } else {
                format!("{} is still loading the project; try again shortly", client.name)
            },
        })));
    };
    let mut items: Vec<&Value> = published.items.iter().collect();
    items.sort_by_key(|d| {
        (
            d["severity"].as_i64().unwrap_or(1),
            d["range"]["start"]["line"].as_u64().unwrap_or(0),
        )
    });
    let total = items.len();
    let errors = items.iter().filter(|d| diagnostics::is_error(d)).count();
    let shown: Vec<Value> = items
        .into_iter()
        .take(50)
        .map(|d| {
            let line = d["range"]["start"]["line"].as_u64().unwrap_or(0) as usize;
            diagnostics::render(rel, d, line_of(&text, line))
        })
        .collect();
    Ok(Some(json!({
        "ok": true,
        "path": rel,
        "server": client.name,
        "errors": errors,
        "total": total,
        "truncated": total > shown.len(),
        "diagnostics": shown,
        "note": format!("Live diagnostics from {} (persistent language server).", client.name),
    })))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locate {
    Definition,
    References,
}

/// Go to definition / find references at a 1-based line and column. None
/// when no server handles the file.
pub async fn locate(
    env: &Env,
    kind: Locate,
    rel: &str,
    line: usize,
    column: usize,
) -> Result<Option<Value>> {
    let Some((launch, language_id)) = env.launch_for(rel) else {
        return Ok(None);
    };
    let text = env
        .read(rel)
        .context("File is missing, too large, or a secret path")?;
    let line_text = text
        .lines()
        .nth(line.saturating_sub(1))
        .context("line is past the end of the file")?;
    let character = client::utf16_column(line_text, column.saturating_sub(1));
    let client = env.client(&launch).await?;
    let uri = env.uri(rel);
    client.sync(&uri, language_id, &text, false).await?;
    let position = json!({"line": line.saturating_sub(1), "character": character});
    let (method, params) = match kind {
        Locate::Definition => (
            "textDocument/definition",
            json!({"textDocument": {"uri": uri}, "position": position}),
        ),
        Locate::References => (
            "textDocument/references",
            json!({"textDocument": {"uri": uri}, "position": position, "context": {"includeDeclaration": true}}),
        ),
    };
    let result = client
        .request(method, params, Duration::from_secs(15))
        .await?;
    let raw: Vec<Value> = match result {
        Value::Array(items) => items,
        Value::Null => Vec::new(),
        single => vec![single],
    };
    let total = raw.len();
    let mut locations = Vec::new();
    let mut files: HashMap<String, Option<String>> = HashMap::new();
    for item in raw.iter().take(80) {
        let (uri, range) = if item.get("targetUri").is_some() {
            (&item["targetUri"], &item["targetSelectionRange"])
        } else {
            (&item["uri"], &item["range"])
        };
        let Some(uri) = uri.as_str() else {
            continue;
        };
        let line = range["start"]["line"].as_u64().unwrap_or(0) as usize;
        let utf16 = range["start"]["character"].as_u64().unwrap_or(0) as usize;
        match env.relative(uri) {
            Some(target) => {
                let text = files
                    .entry(target.clone())
                    .or_insert_with(|| env.read(&target));
                let source_line = text.as_deref().and_then(|t| t.lines().nth(line));
                let column = source_line.map_or(utf16, |l| client::char_column(l, utf16));
                locations.push(json!({
                    "path": target,
                    "line": line + 1,
                    "column": column + 1,
                    "preview": source_line.map(|l| crate::tools::truncate(l.trim(), 200)),
                }));
            }
            None => locations.push(json!({
                "path": client::path_for(uri).map(|p| p.to_string_lossy().into_owned()),
                "line": line + 1,
                "column": utf16 + 1,
                "outside_project": true,
            })),
        }
    }
    Ok(Some(json!({
        "ok": !locations.is_empty(),
        "source": format!("lsp:{}", client.name),
        "count": locations.len(),
        "truncated": total > locations.len(),
        "locations": locations,
        "note": if locations.is_empty() {
            format!("{} found nothing at {rel}:{line}:{column}.", client.name)
        } else {
            format!("Type-aware result from {}.", client.name)
        },
    })))
}

/// Per-language server availability for status views.
pub fn languages(config: &CodeIntelConfig, managed: Option<&Path>) -> Vec<Value> {
    servers::ALL
        .iter()
        .map(|lang| {
            let resolved = servers::resolve(*lang, config, managed);
            let mut row = json!({
                "language": lang.key(),
                "label": lang.label(),
                "managed_package": lang.managed_package(),
                "enabled": config.lsp,
            });
            match resolved {
                Ok(launch) => {
                    row["available"] = json!(true);
                    row["server"] = json!(launch.name);
                    row["path"] = json!(launch.program);
                    row["source"] = json!(launch.source);
                }
                Err(missing) => {
                    row["available"] = json!(false);
                    row["note"] = json!(servers::missing_note(*lang, &missing));
                    row["install_hint"] = json!(lang.install_hint());
                }
            }
            row
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_and_caps() {
        assert_eq!(backoff(1), Duration::from_secs(2));
        assert_eq!(backoff(3), Duration::from_secs(8));
        assert_eq!(backoff(40), Duration::from_secs(300));
    }
}
