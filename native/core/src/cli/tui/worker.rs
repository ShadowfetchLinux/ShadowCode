use super::model::Transcript;
use crate::{
    control::{Client, Endpoint, OwnedJobs},
    paths::AppPaths,
    service::Request,
    workspace::Workspace,
};
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use std::{path::PathBuf, time::Duration};
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ListKind {
    Sessions,
    Models,
    Projects,
    Commands,
}
#[derive(Clone, Debug)]
pub(super) struct Choice {
    pub id: String,
    pub label: String,
}
#[derive(Clone, Debug, Default)]
pub(super) struct View {
    pub workspace: PathBuf,
    pub session: String,
    pub title: String,
    pub model: String,
    pub theme: String,
    pub trusted: bool,
    pub permission: String,
    pub transcript: Transcript,
    pub job: Value,
    pub approvals: Vec<Value>,
    pub error: String,
    pub busy: bool,
    pub older: bool,
    pub list: Vec<Choice>,
    pub list_kind: Option<ListKind>,
    pub commands: Vec<String>,
    pub revision: u64,
}
#[derive(Debug)]
pub(super) enum Action {
    Send(String, String),
    New,
    Session(String),
    Project(PathBuf),
    Model(String),
    Catalog(ListKind, String),
    Cancel,
    Decide(String, bool),
    Trust,
    Older,
    Latest,
}
struct Worker {
    endpoint: Endpoint,
    client: Client,
    owner: OwnedJobs,
    view: View,
    tx: watch::Sender<View>,
    owned_count: usize,
    owned: Vec<String>,
    tick: u64,
}
impl Worker {
    async fn call(&self, method: &str, path: impl Into<String>, body: Value) -> Result<Value> {
        self.client
            .dispatch(Request {
                method: method.into(),
                path: path.into(),
                body,
            })
            .await
    }
    fn publish(&mut self) {
        self.view.revision += 1;
        self.tx.send_replace(self.view.clone());
    }
    async fn config(&mut self) -> Result<()> {
        let cfg = self.call("GET", "/api/config", Value::Null).await?;
        self.view.model = format!(
            "{} · {}",
            cfg["model"]["name"].as_str().unwrap_or(""),
            cfg["model"]["provider"].as_str().unwrap_or("")
        );
        self.view.theme = cfg["ui"]["theme"].as_str().unwrap_or("dark").into();
        self.view.permission = cfg["permissions"]["level"]
            .as_str()
            .unwrap_or("read_only")
            .into();
        self.view.trusted = cfg["trusted_workspaces"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .any(|p| PathBuf::from(p).canonicalize().ok().as_ref() == Some(&self.view.workspace));
        Ok(())
    }
    async fn catalog(&mut self, kind: ListKind, search: &str) -> Result<()> {
        let (result, key) = match kind {
            ListKind::Sessions => (
                self.call(
                    "GET",
                    super::super::query(
                        "/api/sessions",
                        &[
                            (
                                "workspace",
                                self.view
                                    .workspace
                                    .to_str()
                                    .context("Project path must be UTF-8")?,
                            ),
                            ("q", search),
                            ("limit", "200"),
                        ],
                    )?,
                    Value::Null,
                )
                .await?,
                "sessions",
            ),
            ListKind::Models => (
                self.call("GET", "/api/models", Value::Null).await?,
                "models",
            ),
            ListKind::Projects => (
                self.call("GET", "/api/projects", Value::Null).await?,
                "projects",
            ),
            ListKind::Commands => (
                self.call("GET", "/api/commands", Value::Null).await?,
                "commands",
            ),
        };
        self.view.list = result[key]
            .as_array()
            .into_iter()
            .flatten()
            .take(500)
            .filter_map(|row| {
                let (id, label) = match kind {
                    ListKind::Sessions => (
                        row["id"].as_str()?,
                        format!(
                            "{} · {}",
                            row["title"].as_str().unwrap_or("Untitled"),
                            row["id"].as_str().unwrap_or("")
                        ),
                    ),
                    ListKind::Models => (
                        row["id"].as_str()?,
                        format!(
                            "{} · {}",
                            row["name"].as_str().unwrap_or(""),
                            row["provider"].as_str().unwrap_or("")
                        ),
                    ),
                    ListKind::Projects => (
                        row["path"].as_str().or(row["workspace"].as_str())?,
                        row["name"]
                            .as_str()
                            .or(row["path"].as_str())
                            .unwrap_or("Project")
                            .into(),
                    ),
                    ListKind::Commands => (
                        row["name"].as_str()?,
                        format!(
                            "/{} · {}",
                            row["name"].as_str().unwrap_or(""),
                            row["description"].as_str().unwrap_or("")
                        ),
                    ),
                };
                Some(Choice {
                    id: id.into(),
                    label,
                })
            })
            .collect();
        self.view.list_kind = Some(kind);
        if kind == ListKind::Commands {
            self.view.commands = self.view.list.iter().map(|c| c.id.clone()).collect();
        }
        Ok(())
    }
    async fn select(&mut self, id: &str) -> Result<()> {
        let session = self
            .call(
                "GET",
                format!("/api/sessions/{id}?summary=true"),
                Value::Null,
            )
            .await?;
        ensure!(
            session["workspace"].as_str() == self.view.workspace.to_str(),
            "Conversation belongs to a different project"
        );
        self.view.session = session["id"]
            .as_str()
            .context("Conversation ID missing")?
            .into();
        self.view.title = session["title"].as_str().unwrap_or("Conversation").into();
        self.client = self
            .endpoint
            .client(self.view.workspace.clone(), Some(self.view.session.clone()));
        self.load_latest().await
    }
    async fn new_session(&mut self) -> Result<()> {
        let session = self
            .call(
                "POST",
                "/api/sessions",
                json!({"workspace":self.view.workspace,"title":"Terminal conversation"}),
            )
            .await?;
        self.select(session["id"].as_str().context("Conversation ID missing")?)
            .await
    }
    async fn load_latest(&mut self) -> Result<()> {
        self.view.transcript = Transcript::default();
        self.view.older = false;
        let page = self
            .call(
                "GET",
                super::super::query(
                    "/api/events",
                    &[("session_id", &self.view.session), ("limit", "256")],
                )?,
                Value::Null,
            )
            .await?;
        let rows = page["events"].as_array().context("History missing")?;
        self.view.transcript.trimmed = rows.len() == 256;
        for row in rows {
            self.view.transcript.ingest(row);
        }
        self.view.approvals.clear();
        self.view.job = Value::Null;
        Ok(())
    }
    async fn poll(&mut self) -> Result<()> {
        if !self.view.session.is_empty() {
            if !self.view.older {
                // One bounded page per tick keeps input responsive during catch-up.
                let page = self
                    .call(
                        "GET",
                        format!(
                            "/api/sessions/{}/events?after={}&limit=128",
                            self.view.session, self.view.transcript.cursor
                        ),
                        Value::Null,
                    )
                    .await?;
                for row in page["events"].as_array().context("History missing")? {
                    self.view.transcript.ingest(row);
                }
            }
            self.view.job = self
                .call(
                    "GET",
                    super::super::query(
                        "/api/jobs/current",
                        &[
                            ("session_id", &self.view.session),
                            ("include_finished", "true"),
                        ],
                    )?,
                    Value::Null,
                )
                .await?["job"]
                .clone();
            self.view.approvals = self
                .call(
                    "GET",
                    super::super::query("/api/approvals", &[("session_id", &self.view.session)])?,
                    Value::Null,
                )
                .await?["approvals"]
                .as_array()
                .cloned()
                .unwrap_or_default();
        }
        self.tick += 1;
        if self.tick.is_multiple_of(10) {
            self.config().await?;
        }
        Ok(())
    }
    async fn rotate_owner(&mut self) -> Result<()> {
        if self.owned_count < 64 {
            return Ok(());
        }
        for id in &self.owned {
            let job = self
                .call("GET", format!("/api/jobs/{id}"), Value::Null)
                .await?;
            ensure!(
                !active(&job),
                "Finish this terminal's queued work before submitting more than 64 tasks"
            );
        }
        self.owner.close().await?;
        self.owner = self.client.own_jobs().await?;
        self.owned.clear();
        self.owned_count = 0;
        Ok(())
    }
    async fn send(&mut self, input: &str, purpose: &str) -> Result<()> {
        let input = input.trim();
        ensure!(!input.is_empty(), "Enter a task or command");
        let mut body = json!({"session_id":self.view.session,"workspace":self.view.workspace,"queue":true,"purpose":purpose});
        let result = if let Some(command) = input.strip_prefix('/') {
            let (name, args) = command
                .split_once(char::is_whitespace)
                .map(|(n, a)| (n, a.trim_start()))
                .unwrap_or((command, ""));
            match name {
                "new" | "clear" => {
                    self.new_session().await?;
                    return Ok(());
                }
                "resume" => {
                    ensure!(
                        args.len() == 32 && args.bytes().all(|b| b.is_ascii_hexdigit()),
                        "Use /resume with the full conversation ID, or Ctrl-P to choose one"
                    );
                    self.select(args).await?;
                    return Ok(());
                }
                "older" => {
                    self.older().await?;
                    return Ok(());
                }
                "latest" => {
                    self.load_latest().await?;
                    return Ok(());
                }
                "mode" => bail!("Use F3 to cycle Build, Plan, Review and Test"),
                "trust" => bail!("Use Ctrl-T and confirm the displayed project"),
                "project" => bail!("Use Ctrl-G to choose a project, or /open /absolute/path"),
                "open" => {
                    self.project(PathBuf::from(args)).await?;
                    return Ok(());
                }
                "theme" => {
                    ensure!(matches!(args, "light" | "dark"), "Usage: /theme light|dark");
                    self.call(
                        "PUT",
                        "/api/config",
                        json!({"values":{"ui":{"theme":args}}}),
                    )
                    .await?;
                    self.config().await?;
                    return Ok(());
                }
                "settings" => {
                    self.view.transcript.note("Terminal settings","F2 selects a model. F3 selects a task mode. Ctrl-T reviews project trust. /theme light|dark changes appearance. Use shadowcode config in another terminal for detailed configuration and credential references.");
                    return Ok(());
                }
                "export" => {
                    ensure!(!args.is_empty(), "Usage: /export /absolute/file.md");
                    let path = super::super::expand(std::path::Path::new(args))?;
                    ensure!(path.is_absolute(), "Export requires an absolute path");
                    let data = self
                        .call(
                            "GET",
                            format!("/api/sessions/{}/export?format=md", self.view.session),
                            Value::Null,
                        )
                        .await?;
                    use std::io::Write;
                    let mut file = tempfile::NamedTempFile::new_in(
                        path.parent().context("Export parent missing")?,
                    )?;
                    file.write_all(
                        data["content"]
                            .as_str()
                            .context("Export content missing")?
                            .as_bytes(),
                    )?;
                    file.as_file().sync_all()?;
                    file.persist_noclobber(&path)
                        .context("Export destination already exists or cannot be written")?;
                    self.view
                        .transcript
                        .note("Exported conversation", &path.display().to_string());
                    return Ok(());
                }
                "run" | "test" if !args.is_empty() => {
                    self.rotate_owner().await?;
                    body["command"] = json!(args);
                    self.owner.submit_test(body).await?
                }
                _ => {
                    body["name"] = json!(name);
                    body["args"] = json!(args);
                    let catalog = self.call("GET", "/api/commands", Value::Null).await?;
                    let custom = catalog["commands"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|r| r["name"] == name && r["source"] != "builtin");
                    if custom || matches!(name, "plan" | "review" | "skill" | "test") {
                        self.rotate_owner().await?;
                        self.owner.submit_workflow(body).await?
                    } else {
                        self.call("POST", "/api/commands/run", body).await?
                    }
                }
            }
        } else {
            self.rotate_owner().await?;
            body["task"] = json!(input);
            self.owner.submit(body).await?
        };
        let job = if result["metadata"]["job"].is_object() {
            &result["metadata"]["job"]
        } else {
            &result
        };
        if let Some(id) = job["id"].as_str().filter(|_| job["task_id"].is_string()) {
            self.owned.push(id.into());
            self.owned_count += 1;
            if self.view.session != job["session_id"].as_str().unwrap_or("") {
                self.select(job["session_id"].as_str().context("Task session missing")?)
                    .await?;
            }
            if self.view.older {
                self.load_latest().await?;
            }
        } else {
            if let Some(sid) = result["metadata"]["session_id"].as_str() {
                self.select(sid).await?;
            }
            let panel = result["metadata"]["panel"].as_str().unwrap_or("");
            let text = match panel {
                "skills" => self
                    .call("GET", "/api/workspace/skills", Value::Null)
                    .await?
                    .to_string(),
                "goals" => self
                    .call("GET", "/api/goals", Value::Null)
                    .await?
                    .to_string(),
                "background" => self
                    .call("GET", "/api/background", Value::Null)
                    .await?
                    .to_string(),
                "health" => self
                    .call("GET", "/api/doctor", Value::Null)
                    .await?
                    .to_string(),
                _ => format!(
                    "{}{}{}",
                    result["body"].as_str().unwrap_or(""),
                    result["diff"].as_str().unwrap_or(""),
                    if result["items"].as_array().is_some_and(|a| !a.is_empty()) {
                        result["items"].to_string()
                    } else {
                        String::new()
                    }
                ),
            };
            self.view
                .transcript
                .note(result["headline"].as_str().unwrap_or("Command"), &text);
        }
        Ok(())
    }
    async fn older(&mut self) -> Result<()> {
        if self.view.transcript.first == 0 {
            return Ok(());
        }
        let page = self
            .call(
                "GET",
                format!(
                    "/api/sessions/{}/events?before={}&limit=256",
                    self.view.session, self.view.transcript.first
                ),
                Value::Null,
            )
            .await?;
        let rows = page["events"].as_array().context("History missing")?;
        ensure!(!rows.is_empty(), "Beginning of saved history");
        self.view.transcript = Transcript::default();
        for row in rows {
            self.view.transcript.ingest(row);
        }
        self.view.older = true;
        Ok(())
    }
    async fn project(&mut self, path: PathBuf) -> Result<()> {
        let path = Workspace::open(&super::super::expand(&path)?)?.path;
        for id in &self.owned {
            let job = self
                .call("GET", format!("/api/jobs/{id}"), Value::Null)
                .await?;
            ensure!(
                !active(&job),
                "Stop or finish all terminal tasks before changing project"
            );
        }
        self.owner.close().await?;
        self.client = self.endpoint.client(path.clone(), None);
        self.owner = self.client.own_jobs().await?;
        self.view.workspace = path;
        self.owned.clear();
        self.owned_count = 0;
        self.config().await?;
        self.new_session().await
    }
    async fn action(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Send(input, purpose) => self.send(&input, &purpose).await?,
            Action::New => self.new_session().await?,
            Action::Session(id) => self.select(&id).await?,
            Action::Project(path) => self.project(path).await?,
            Action::Older => self.older().await?,
            Action::Latest => self.load_latest().await?,
            Action::Model(id) => {
                self.call("POST", "/api/models/select", json!({"id":id}))
                    .await?;
                self.config().await?;
            }
            Action::Catalog(kind, search) => self.catalog(kind, &search).await?,
            Action::Cancel => {
                if let Some(id) = self.view.job["id"].as_str() {
                    self.call("POST", format!("/api/jobs/{id}/cancel"), json!({}))
                        .await?;
                }
            }
            Action::Decide(id, approved) => {
                ensure!(
                    self.view.approvals.iter().any(|a| a["id"] == id),
                    "Approval changed; refresh and review it again"
                );
                self.call("POST",format!("/api/approvals/{id}"),json!({"session_id":self.view.session,"decision":if approved{"approve"}else{"deny"}})).await?;
            }
            Action::Trust => {
                self.call(
                    "POST",
                    "/api/projects/trust",
                    json!({"path":self.view.workspace}),
                )
                .await?;
                self.config().await?;
            }
        };
        Ok(())
    }
}
pub(super) fn active(job: &Value) -> bool {
    matches!(
        job["status"].as_str(),
        Some("queued" | "running" | "cancelling")
    )
}
pub(super) async fn run(
    paths: AppPaths,
    workspace: PathBuf,
    session: Option<String>,
    mut actions: mpsc::Receiver<Action>,
    tx: watch::Sender<View>,
    stop: CancellationToken,
) -> Result<()> {
    let endpoint = Endpoint::for_paths(&paths)?;
    let client = endpoint.client(workspace.clone(), None);
    let owner = client.own_jobs().await?;
    let mut worker = Worker {
        endpoint,
        client,
        owner,
        view: View {
            workspace,
            ..Default::default()
        },
        tx,
        owned_count: 0,
        owned: vec![],
        tick: 0,
    };
    let result = async {
        worker.config().await?;
        if let Some(id) = session {
            worker.select(&id).await?;
        } else {
            worker.new_session().await?;
        }
        worker.catalog(ListKind::Commands, "").await?;
        worker.publish();
        let mut interval = tokio::time::interval(Duration::from_millis(350));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                action=actions.recv()=>{
                    let Some(action)=action else {break};worker.view.busy=true;worker.view.error.clear();worker.publish();
                    if let Err(error)=worker.action(action).await {worker.view.error=format!("{error:#}");}
                    worker.view.busy=false;worker.publish();
                }
                _=interval.tick()=>{if let Err(error)=worker.poll().await{worker.view.error=format!("Engine update failed: {error:#}");}worker.publish();}
            }
        }
        Ok::<(), anyhow::Error>(())
    };
    let result = tokio::select! {result=result=>result,_=stop.cancelled()=>Ok(())};
    // EOF remains the fallback if a submission was interrupted before its reply.
    let closed = worker.owner.close().await;
    result?;
    closed
}
