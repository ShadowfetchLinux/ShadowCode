//! Native command-line frontend. The same command service, permissions, event
//! journal, and engine are used by local runs and an already open desktop.
macro_rules! outln {
    ($($arg:tt)*) => { writeln!(std::io::stdout(), $($arg)*).context("Could not write terminal output")? };
}
macro_rules! out {
    ($($arg:tt)*) => { write!(std::io::stdout(), $($arg)*).context("Could not write terminal output")? };
}
macro_rules! errln {
    ($($arg:tt)*) => { writeln!(std::io::stderr(), $($arg)*).context("Could not write terminal status")? };
}
macro_rules! err {
    ($($arg:tt)*) => { write!(std::io::stderr(), $($arg)*).context("Could not write terminal status")? };
}
pub mod args;
pub(crate) mod backend;
mod watch;
use crate::{
    paths::{self, AppPaths},
    workspace::Workspace,
};
use anyhow::{bail, ensure, Context, Result};
pub use args::Options;
use args::{Background, Command, Mcp, Run, TaskOptions};
use backend::Backend;
use clap::Parser;
use serde_json::{json, Value};
use std::{
    io::{IsTerminal, Write},
    path::{Path, PathBuf},
};
pub struct Outcome {
    pub code: i32,
    pub value: Value,
    pub raw: Option<String>,
}
impl Outcome {
    fn value(value: Value) -> Self {
        Self {
            code: if value.get("ok") == Some(&Value::Bool(false)) || value["kind"] == "error" {
                1
            } else {
                0
            },
            value,
            raw: None,
        }
    }
}
impl Options {
    pub fn parse_args() -> Self {
        Self::parse()
    }
    pub fn desktop(&self) -> bool {
        matches!(self.command, None | Some(Command::Ui))
    }
    pub fn mcp_stdio(&self) -> bool {
        matches!(
            self.command,
            Some(Command::Mcp {
                action: Some(Mcp::Serve { .. })
            })
        )
    }
    pub fn events(&self) -> bool {
        match &self.command {
            Some(Command::Run(run)) => run.options.events,
            Some(Command::Command { task, .. } | Command::Skill { task, .. }) => task.events,
            _ => false,
        }
    }
    pub fn paths(&self) -> Result<AppPaths> {
        match &self.profile {
            Some(root) => AppPaths::isolated(&expand(root)?),
            None => AppPaths::discover(),
        }
    }
}
fn expand(path: &Path) -> Result<PathBuf> {
    let text = path.to_string_lossy();
    Ok(if text == "~" {
        PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?)
    } else if let Some(rest) = text.strip_prefix("~/") {
        PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?).join(rest)
    } else {
        path.into()
    })
}
fn query(path: &str, pairs: &[(&str, &str)]) -> Result<String> {
    let mut url = reqwest::Url::parse(&format!("http://ipc.local{path}"))?;
    url.query_pairs_mut().extend_pairs(pairs.iter().copied());
    Ok(format!("{}?{}", url.path(), url.query().unwrap_or("")))
}
fn unique(rows: &[Value], prefix: &str, label: &str) -> Result<String> {
    ensure!(!prefix.is_empty(), "Provide a {label} ID");
    let found: Vec<_> = rows
        .iter()
        .filter_map(|row| row["id"].as_str())
        .filter(|id| id.starts_with(prefix))
        .collect();
    ensure!(
        found.len() == 1,
        "Choose a unique {label} ID prefix; {} matches found",
        found.len()
    );
    Ok(found[0].into())
}
async fn sessions(backend: &Backend) -> Result<Vec<Value>> {
    Ok(backend
        .call("GET", "/api/sessions?limit=10000", Value::Null)
        .await?["sessions"]
        .as_array()
        .context("Session list missing")?
        .clone())
}
async fn session(backend: &Backend, id: Option<&str>, workspace: &Path) -> Result<String> {
    let rows = sessions(backend).await?;
    if let Some(id) = id {
        unique(&rows, id, "conversation")
    } else {
        rows.iter()
            .find(|row| row["workspace"].as_str() == workspace.to_str())
            .and_then(|row| row["id"].as_str())
            .map(str::to_owned)
            .context("No conversation in this project; supply --session or run a task first")
    }
}
async fn task(
    backend: &Backend,
    workspace: &Path,
    request: &str,
    options: &TaskOptions,
    json_output: bool,
    workflow: Option<(&str, &str)>,
) -> Result<Outcome> {
    ensure!(
        !options.detach || backend.persistent,
        "Detached tasks require an open desktop or `shadowcode serve`"
    );
    ensure!(
        !options.interactive || std::io::stdin().is_terminal(),
        "--interactive requires a terminal on stdin"
    );
    let sid = match &options.session {
        Some(id) => Some(session(backend, Some(id), workspace).await?),
        None => None,
    };
    let mut body = json!({"workspace":workspace,"session_id":sid,"model":options.model,"purpose":options.purpose,"queue":options.queue});
    let result = if let Some((name, args)) = workflow {
        body["name"] = json!(name);
        body["args"] = json!(args);
        backend.call("POST", "/api/commands/run", body).await?
    } else {
        body["task"] = json!(request);
        backend.call("POST", "/api/jobs", body).await?
    };
    let started = if workflow.is_some() {
        result["metadata"]["job"].clone()
    } else {
        result.clone()
    };
    if started["id"].is_null() {
        return Ok(Outcome::value(result));
    }
    if options.detach {
        return Ok(Outcome::value(started));
    }
    watch::job(backend, started, options, json_output, true).await
}
/// Terminal work runs before any GTK/Tauri initialization, including in AppImage.
pub async fn run(options: Options) -> Result<i32> {
    let parent = crate::lifecycle::extraction_parent();
    let workspace = Workspace::open(&expand(
        &options
            .workspace
            .clone()
            .unwrap_or(std::env::current_dir()?),
    )?)?
    .path;
    let paths = options.paths()?;
    if let Some(Command::Mcp {
        action: Some(action @ (Mcp::Serve { .. } | Mcp::Register { .. })),
    }) = &options.command
    {
        let (allow_write, allow_approvals) = match action {
            Mcp::Serve {
                allow_write,
                allow_approvals,
            }
            | Mcp::Register {
                allow_write,
                allow_approvals,
            } => (*allow_write, *allow_approvals),
            _ => unreachable!(),
        };
        if matches!(action, Mcp::Register { .. }) {
            let current = std::env::current_exe()?;
            let appimage = std::env::var_os("APPIMAGE")
                .map(PathBuf::from)
                .filter(|image| {
                    image.is_absolute()
                        && image.is_file()
                        && std::env::var_os("APPDIR")
                            .is_some_and(|dir| current.starts_with(PathBuf::from(dir)))
                });
            let mut args = Vec::new();
            if appimage.is_some() {
                args.push("--appimage-extract-and-run".into());
            }
            let executable = appimage.unwrap_or(current);
            args.extend([
                "--workspace".to_owned(),
                workspace.to_string_lossy().into_owned(),
            ]);
            if let Some(profile) = &options.profile {
                args.extend([
                    "--profile".into(),
                    expand(profile)?
                        .canonicalize()?
                        .to_string_lossy()
                        .into_owned(),
                ]);
            }
            args.extend(["mcp".into(), "serve".into()]);
            if allow_write {
                args.push("--allow-write".into());
            }
            if allow_approvals {
                args.push("--allow-approvals".into());
            }
            outln!(
                "{}",
                serde_json::to_string_pretty(
                    &json!({"mcpServers":{"shadowcode":{"command":executable,"args":args}}})
                )?
            );
            return Ok(0);
        }
        ensure!(
            !options.json,
            "MCP stdout is reserved for JSON-RPC; omit --json"
        );
        let cancel = tokio_util::sync::CancellationToken::new();
        let signal = cancel.clone();
        let listener = tokio::spawn(async move {
            crate::lifecycle::interrupted(parent).await;
            signal.cancel();
        });
        let result = crate::mcp::server::serve_io(
            paths,
            workspace,
            crate::mcp::server::Access {
                allow_write,
                allow_approvals,
            },
            tokio::io::stdin(),
            tokio::io::stdout(),
            cancel,
        )
        .await;
        listener.abort();
        result?;
        return Ok(0);
    }
    let serving = matches!(options.command, Some(Command::Serve));
    let events = options.events();
    let backend = Backend::open(paths, workspace.clone(), serving, parent).await?;
    let result = execute(&backend, &workspace, &options).await;
    let closed = backend.close().await;
    let outcome = match (result, closed) {
        (Ok(outcome), Ok(())) => outcome,
        (Err(error), Ok(())) => return Err(error),
        (result, Err(error)) => {
            return Err(error.context(format!(
                "Command result: {}",
                if result.is_ok() {
                    "completed"
                } else {
                    "failed"
                }
            )))
        }
    };
    if events {
        outln!(
            "{}",
            serde_json::to_string(
                &json!({"type":"result","exit_code":outcome.code,"result":outcome.value})
            )?
        );
    } else if options.json {
        outln!("{}", serde_json::to_string_pretty(&outcome.value)?);
    } else if let Some(raw) = outcome.raw {
        // Redirected exports are exact bytes. Interactive terminal rendering
        // and all human command output still remove terminal control codes.
        if matches!(options.command, Some(Command::Export { .. }))
            && !std::io::stdout().is_terminal()
        {
            std::io::stdout().write_all(raw.as_bytes())?;
        } else {
            std::io::stdout().write_all(watch::plain(&raw).as_bytes())?;
        }
        std::io::stdout().flush()?;
    } else {
        display(&outcome.value)?;
    }
    Ok(outcome.code)
}
fn display(value: &Value) -> Result<()> {
    if let Some(headline) = value["headline"].as_str() {
        outln!("{}", watch::plain(headline));
        if let Some(body) = value["body"].as_str().filter(|body| !body.is_empty()) {
            outln!("{}", watch::plain(body));
        }
        if let Some(diff) = value["diff"].as_str().filter(|diff| !diff.is_empty()) {
            outln!("{}", watch::plain(diff));
        }
        for item in value["items"].as_array().into_iter().flatten() {
            outln!(
                "{}: {}",
                watch::plain(item["label"].as_str().unwrap_or("")),
                watch::plain(item["value"].as_str().unwrap_or(""))
            );
        }
        if let Some(id) = value["metadata"]["session_id"].as_str() {
            outln!("Conversation: {id}");
        }
        if !value["metadata"]["panel"].is_null()
            || value["kind"] == "overlay"
            || !value["metadata"]["action"].is_null()
        {
            outln!(
                "Desktop action: {}",
                watch::plain(&value["metadata"].to_string())
            );
        }
    } else if let Some(summary) = value["summary"].as_str() {
        outln!(
            "{}\n{}",
            watch::plain(summary),
            watch::plain(&format!(
                "{} · conversation {} · job {}",
                value["status"].as_str().unwrap_or(""),
                value["session_id"].as_str().unwrap_or(""),
                value["id"].as_str().unwrap_or("")
            ))
        );
    } else {
        outln!("{}", watch::plain(&serde_json::to_string_pretty(value)?));
    }
    Ok(())
}
async fn execute(backend: &Backend, workspace: &Path, options: &Options) -> Result<Outcome> {
    let command = options
        .command
        .as_ref()
        .context("No CLI command selected")?;
    let value = match command {
        Command::Ui => bail!("Desktop startup must use the native window"),
        Command::Serve => {
            ensure!(
                backend.service().is_some(),
                "This profile already has an engine"
            );
            errln!("ShadowCode {} · serving {}\nPress Ctrl-C to stop managed work and close the engine.",crate::VERSION,workspace.display());
            watch::interrupted(backend.parent).await;
            return Ok(Outcome::value(json!({"status":"stopped"})));
        }
        Command::Run(Run {
            task: request,
            options: task_options,
        }) => {
            return task(
                backend,
                workspace,
                request,
                task_options,
                options.json,
                None,
            )
            .await
        }
        Command::Command {
            name,
            args,
            task: task_options,
        } => {
            return task(
                backend,
                workspace,
                "",
                task_options,
                options.json,
                Some((name.as_deref().unwrap_or("help"), args)),
            )
            .await
        }
        Command::Skill {
            name,
            args,
            list,
            task: task_options,
        } => {
            if *list || name.is_none() {
                backend
                    .call("GET", "/api/workspace/skills", Value::Null)
                    .await?
            } else {
                return task(
                    backend,
                    workspace,
                    "",
                    task_options,
                    options.json,
                    Some(("skill", &format!("{} {args}", name.as_ref().unwrap()))),
                )
                .await;
            }
        }
        Command::Exec { command, timeout } => {
            let request = backend.call(
                "POST",
                "/api/workspace/exec",
                json!({"workspace":workspace,"command":command,"timeout":timeout}),
            );
            let value = tokio::select! {result=request=>result?,_=watch::interrupted(backend.parent)=>return Ok(Outcome{code:130,value:json!({"status":"cancelled","command":command}),raw:None})};
            if options.json {
                value
            } else {
                return Ok(Outcome {
                    code: if value["ok"] == true { 0 } else { 1 },
                    raw: Some(format!(
                        "{}{}\nExit: {}\n",
                        value["stdout"].as_str().unwrap_or(""),
                        value["stderr"].as_str().unwrap_or(""),
                        value["exit_code"]
                    )),
                    value,
                });
            }
        }
        Command::Hooks {
            enable,
            disable,
            hash,
        } => {
            if let Some(path) = enable.as_ref().or(disable.as_ref()) {
                backend.call("POST","/api/hooks/activation",json!({"workspace":workspace,"path":path,"hash":hash.as_deref().unwrap_or(""),"enabled":enable.is_some()})).await?
            } else {
                backend.call("GET", "/api/hooks", Value::Null).await?
            }
        }
        Command::Mcp { action } => match action {
            Some(Mcp::Serve { .. } | Mcp::Register { .. }) => {
                unreachable!("MCP transport handled before CLI engine setup")
            }
            None => backend.call("GET", "/api/mcp/servers", Value::Null).await?,
            Some(Mcp::Add { definition, hash }) => {
                use std::io::Read;
                let file = std::fs::File::open(definition).context("Cannot open MCP definition")?;
                ensure!(
                    file.metadata()?.is_file(),
                    "MCP definition must be a regular file"
                );
                let mut text = String::new();
                file.take(32_001).read_to_string(&mut text)?;
                ensure!(text.len() <= 32_000, "MCP definition exceeds 32 KB");
                let definition: Value = serde_yaml_ng::from_str(&text)
                    .map_err(|_| anyhow::anyhow!("Invalid MCP JSON/YAML definition"))?;
                backend
                    .call(
                        "POST",
                        "/api/mcp/servers",
                        json!({"definition":definition,"hash":hash.as_deref().unwrap_or("")}),
                    )
                    .await?
            }
            Some(Mcp::Enable { server, hash }) => {
                backend
                    .call(
                        "POST",
                        "/api/mcp/activation",
                        json!({"workspace":workspace,"server":server,"hash":hash,"enabled":true}),
                    )
                    .await?
            }
            Some(Mcp::Disable { server }) => {
                backend
                    .call(
                        "POST",
                        "/api/mcp/activation",
                        json!({"workspace":workspace,"server":server,"enabled":false}),
                    )
                    .await?
            }
            Some(Mcp::Remove { server, hash }) => {
                backend
                    .call(
                        "POST",
                        "/api/mcp/servers/delete",
                        json!({"server":server,"hash":hash}),
                    )
                    .await?
            }
        },
        Command::Status => {
            let mut status = backend
                .call("GET", "/api/workspace/status", Value::Null)
                .await?;
            let jobs = backend.call("GET", "/api/jobs", Value::Null).await?;
            status["jobs"] = json!(jobs["jobs"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|job| job["workspace"].as_str() == workspace.to_str()
                    && matches!(
                        job["status"].as_str(),
                        Some("queued" | "running" | "cancelling")
                    ))
                .collect::<Vec<_>>());
            status
        }
        Command::Trust => {
            backend
                .call("POST", "/api/projects/trust", json!({"path":workspace}))
                .await?
        }
        Command::Health { test_model } => {
            let mut health = backend.call("GET", "/api/health", Value::Null).await?;
            if *test_model {
                let result = backend.call("POST", "/api/models/test", json!({})).await?;
                health["ok"] = result["ok"].clone();
                health["provider_test"] = result;
            }
            health
        }
        Command::Models {
            selected,
            provider,
            endpoint,
            api_key_env,
            context_limit,
            no_detect,
        } => {
            if let Some(id) = selected {
                if provider.is_some() || endpoint.is_some() {
                    let registered=backend.call("POST","/api/models/register",json!({"name":id,"provider":provider,"endpoint":endpoint,"api_key_env":api_key_env,"context_limit":context_limit})).await?;
                    backend
                        .call(
                            "POST",
                            "/api/models/select",
                            json!({"id":registered["model"]["default"]}),
                        )
                        .await?
                } else {
                    backend
                        .call("POST", "/api/models/select", json!({"id":id}))
                        .await?
                }
            } else {
                ensure!(
                    provider.is_none()
                        && endpoint.is_none()
                        && api_key_env.is_none()
                        && context_limit.is_none(),
                    "Model registration options require --use"
                );
                backend
                    .call(
                        "GET",
                        if *no_detect {
                            "/api/models?detect=false"
                        } else {
                            "/api/models"
                        },
                        Value::Null,
                    )
                    .await?
            }
        }
        Command::Config { key, value } => {
            let config = backend.call("GET", "/api/config", Value::Null).await?;
            match (key, value) {
                (None, None) => config,
                (Some(key), None) => {
                    let mut selected = &config;
                    for part in key.split('.') {
                        selected = selected
                            .get(part)
                            .with_context(|| format!("Unknown setting: {key}"))?;
                    }
                    selected.clone()
                }
                (Some(key), Some(value)) => {
                    let parts: Vec<_> = key.split('.').collect();
                    ensure!(
                        parts.len() <= 8 && !parts.iter().any(|part| part.is_empty()),
                        "Invalid setting key"
                    );
                    let mut existing = &config;
                    for part in &parts {
                        existing = existing
                            .get(*part)
                            .with_context(|| format!("Unknown setting: {key}"))?;
                    }
                    let mut patch = serde_json::from_str(value).unwrap_or_else(|_| json!(value));
                    for part in parts.into_iter().rev() {
                        patch = json!({part:patch});
                    }
                    backend
                        .call("PUT", "/api/config", json!({"values":patch}))
                        .await?
                }
                _ => bail!("Provide a setting key before its value"),
            }
        }
        Command::Sessions {
            query: search,
            rename,
            delete,
        } => {
            if rename.is_some() || *delete {
                let id = unique(
                    &sessions(backend).await?,
                    search
                        .as_deref()
                        .context("Provide a conversation ID prefix")?,
                    "conversation",
                )?;
                backend
                    .call(
                        if *delete { "DELETE" } else { "PATCH" },
                        format!("/api/sessions/{id}"),
                        json!({"title":rename}),
                    )
                    .await?
            } else {
                backend
                    .call(
                        "GET",
                        query(
                            "/api/sessions",
                            &[("q", search.as_deref().unwrap_or("")), ("limit", "1000")],
                        )?,
                        Value::Null,
                    )
                    .await?
            }
        }
        Command::Export {
            session: id,
            format,
            output,
        } => {
            let id = session(backend, id.as_deref(), workspace).await?;
            let export = backend
                .call(
                    "GET",
                    format!("/api/sessions/{id}/export?format={format}"),
                    Value::Null,
                )
                .await?;
            let content = export["content"].as_str().context("Export body missing")?;
            if let Some(path) = output {
                let path = expand(path)?;
                let path = if path.is_absolute() {
                    path
                } else {
                    std::env::current_dir()?.join(path)
                };
                paths::atomic_write(&path, content.as_bytes(), false)?;
                json!({"ok":true,"path":path,"bytes":content.len()})
            } else {
                return Ok(Outcome {
                    code: 0,
                    value: export.clone(),
                    raw: Some(content.into()),
                });
            }
        }
        Command::Jobs {
            id,
            watch: watch_job,
            cancel,
            interactive,
        } => {
            let rows = backend.call("GET", "/api/jobs", Value::Null).await?;
            if let Some(id) = id {
                let id = unique(rows["jobs"].as_array().context("Jobs missing")?, id, "job")?;
                let job = backend
                    .call(
                        if *cancel { "POST" } else { "GET" },
                        format!("/api/jobs/{id}{}", if *cancel { "/cancel" } else { "" }),
                        json!({}),
                    )
                    .await?;
                if *watch_job {
                    return watch::job(
                        backend,
                        job,
                        &TaskOptions {
                            interactive: *interactive,
                            approval: args::ApprovalMode::Wait,
                            ..Default::default()
                        },
                        options.json,
                        false,
                    )
                    .await;
                }
                job
            } else {
                ensure!(
                    !watch_job && !cancel,
                    "Supply a job ID with --watch or --cancel"
                );
                rows
            }
        }
        Command::Approvals {
            session: id,
            id: approval,
            decision,
        } => {
            let sid = match id {
                Some(id) => Some(session(backend, Some(id), workspace).await?),
                None => None,
            };
            if let (Some(id), Some(decision)) = (approval, decision) {
                ensure!(
                    id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
                    "Invalid approval ID"
                );
                let sid = sid.context("Approval decisions require --session")?;
                backend
                    .call(
                        "POST",
                        format!("/api/approvals/{id}"),
                        json!({"session_id":sid,"decision":decision}),
                    )
                    .await?
            } else {
                ensure!(
                    approval.is_none(),
                    "Supply --decision approve or deny with --id"
                );
                backend
                    .call(
                        "GET",
                        query(
                            "/api/approvals",
                            &[("session_id", sid.as_deref().unwrap_or(""))],
                        )?,
                        Value::Null,
                    )
                    .await?
            }
        }
        Command::Goal {
            instruction,
            run,
            detach,
            interactive,
        } => {
            ensure!(
                !interactive || std::io::stdin().is_terminal(),
                "--interactive requires a terminal on stdin"
            );
            ensure!(
                !detach || backend.persistent,
                "Detached goals require an open desktop or shadowcode serve"
            );
            let goal = backend
                .call(
                    "POST",
                    "/api/goals",
                    json!({"workspace":workspace,"instruction":instruction,"run":run}),
                )
                .await?;
            if *run && !*detach {
                return watch::goal(
                    backend,
                    goal["id"].as_str().context("Goal ID missing")?,
                    *interactive,
                    options.json,
                )
                .await;
            }
            goal
        }
        Command::Goals {
            resume,
            pause,
            interactive,
        } => {
            ensure!(
                !interactive || std::io::stdin().is_terminal(),
                "--interactive requires a terminal on stdin"
            );
            let goals = backend.call("GET", "/api/goals", Value::Null).await?;
            if let Some(prefix) = resume.as_ref().or(pause.as_ref()) {
                let id = unique(
                    goals["goals"].as_array().context("Goals missing")?,
                    prefix,
                    "goal",
                )?;
                let value = backend
                    .call(
                        "POST",
                        format!(
                            "/api/goals/{id}/{}",
                            if pause.is_some() { "pause" } else { "run" }
                        ),
                        json!({}),
                    )
                    .await?;
                if resume.is_some() {
                    return watch::goal(backend, &id, *interactive, options.json).await;
                }
                value
            } else {
                goals
            }
        }
        Command::Background { action } => match action {
            Background::List => backend.call("GET", "/api/background", Value::Null).await?,
            Background::Start { name, command } => {
                ensure!(backend.persistent,"Background processes need an open desktop or `shadowcode serve`; their owner stays running to manage logs and cleanup");
                backend
                    .call(
                        "POST",
                        "/api/background",
                        json!({"name":name,"command":command}),
                    )
                    .await?
            }
            Background::Stop { id } | Background::Logs { id } => {
                let rows = backend.call("GET", "/api/background", Value::Null).await?;
                let id = unique(
                    rows["tasks"]
                        .as_array()
                        .context("Process history missing")?,
                    id,
                    "process",
                )?;
                let stop = matches!(action, Background::Stop { .. });
                backend
                    .call(
                        if stop { "POST" } else { "GET" },
                        format!("/api/background/{id}{}", if stop { "/stop" } else { "" }),
                        json!({}),
                    )
                    .await?
            }
        },
        Command::Checkpoints {
            session: id,
            restore,
            undo,
        } => {
            let sid = session(backend, Some(id), workspace).await?;
            backend.call("POST","/api/commands/run",json!({"name":if *undo{"undo"}else if restore.is_some(){"rollback"}else{"checkpoints"},"args":restore.as_deref().unwrap_or(""),"session_id":sid})).await?
        }
    };
    Ok(Outcome::value(value))
}
