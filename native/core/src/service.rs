//! Application commands shared by Tauri IPC and the optional loopback transport.
use crate::{
    checkpoint,
    config::{self, Config, ModelConfig, PermissionLevel},
    engine::{Engine, JobOwner, StartRequest, WorkspaceReservation},
    model_registry,
    models::{self, ModelClient},
    paths::AppPaths,
    permissions::{self, Decision},
    process::{self, ProcessSpec},
    routing,
    store::MilestoneSpec,
    tools::truncate,
    workspace::Workspace,
};
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::Write,
    ops::Deref,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
mod commands;
#[cfg(unix)]
mod inspection;
mod memory;

#[derive(Clone)]
struct Selection {
    generation: u64,
    workspace: PathBuf,
    session: Option<String>,
}
type DetectionCache = Arc<tokio::sync::Mutex<Option<(Instant, Vec<Value>)>>>;
struct ManualWorkspace {
    workspace: Workspace,
    reservation: WorkspaceReservation,
}
impl Deref for ManualWorkspace {
    type Target = Workspace;
    fn deref(&self) -> &Workspace {
        &self.workspace
    }
}
#[derive(Clone)]
pub struct Service {
    pub engine: Engine,
    selection: Arc<RwLock<Selection>>,
    detection: DetectionCache,
    remember_selection: bool,
    job_owner: Option<JobOwner>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub body: Value,
}
impl Service {
    pub fn open(paths: AppPaths, workspace: Option<PathBuf>) -> Result<Self> {
        let workspace = workspace
            .or_else(|| paths.remembered_workspace())
            .unwrap_or(std::env::current_dir()?);
        let workspace = Workspace::open(&workspace)?.path;
        Ok(Self {
            engine: Engine::open(paths)?,
            selection: Arc::new(RwLock::new(Selection {
                generation: 0,
                workspace,
                session: None,
            })),
            detection: Arc::new(tokio::sync::Mutex::new(None)),
            remember_selection: true,
            job_owner: None,
        })
    }
    /// A transport client shares the engine, but has its own navigation state.
    /// Its requests must never activate a different project in the desktop.
    pub fn fork_selection(&self, workspace: PathBuf, session: Option<String>) -> Result<Self> {
        let workspace = Workspace::open(&workspace)?.path;
        if let Some(id) = &session {
            ensure!(
                self.engine
                    .store()
                    .session(id)?
                    .context("Session not found")?["workspace"]
                    .as_str()
                    == workspace.to_str(),
                "Session belongs to a different workspace"
            );
        }
        Ok(Self {
            engine: self.engine.clone(),
            selection: Arc::new(RwLock::new(Selection {
                workspace,
                session,
                generation: 0,
            })),
            detection: self.detection.clone(),
            remember_selection: false,
            job_owner: None,
        })
    }
    pub(crate) fn with_job_owner(mut self, owner: JobOwner) -> Self {
        self.job_owner = Some(owner);
        self
    }
    pub fn workspace(&self) -> Result<PathBuf> {
        Ok(self
            .selection
            .read()
            .map_err(|_| anyhow::anyhow!("Project selection lock poisoned"))?
            .workspace
            .clone())
    }
    fn config(&self) -> Result<Config> {
        Config::load(self.engine.paths(), Some(&self.workspace()?))
    }
    fn snapshot_selection(&self) -> Result<Selection> {
        Ok(self
            .selection
            .read()
            .map_err(|_| anyhow::anyhow!("Project selection lock poisoned"))?
            .clone())
    }
    fn select(&self, path: &Path, session: Option<String>) -> Result<()> {
        self.select_if(path, session, None)
    }
    fn select_if(&self, path: &Path, session: Option<String>, expected: Option<u64>) -> Result<()> {
        let workspace = Workspace::open(path)?.path;
        let mut selection = self
            .selection
            .write()
            .map_err(|_| anyhow::anyhow!("Project selection lock poisoned"))?;
        if expected.is_some_and(|generation| generation != selection.generation) {
            return Ok(());
        }
        if self.remember_selection {
            self.engine.paths().remember_workspace(&workspace)?;
        }
        self.engine.store().touch_project(&workspace)?;
        *selection = Selection {
            workspace,
            session,
            generation: selection.generation.wrapping_add(1),
        };
        Ok(())
    }
    pub async fn detected(&self, refresh: bool) -> Vec<Value> {
        let mut cache = self.detection.lock().await;
        if !refresh {
            if let Some((time, providers)) = &*cache {
                if time.elapsed() < Duration::from_secs(30) {
                    return providers.clone();
                }
            }
        }
        let providers = models::detect().await;
        *cache = Some((Instant::now(), providers.clone()));
        providers
    }
    pub async fn dispatch(&self, request: Request) -> Result<Value> {
        ensure!(
            request.path.starts_with("/api/") && request.path.len() <= 16000,
            "Invalid application command path"
        );
        ensure!(
            request.body.to_string().len() <= 8_000_000,
            "Request exceeds 8 MB"
        );
        let url = reqwest::Url::parse(&format!("http://ipc.local{}", request.path))?;
        let query: HashMap<String, String> = url.query_pairs().into_owned().collect();
        let path = url.path();
        let parts: Vec<_> = path.trim_matches('/').split('/').collect();
        let body = &request.body;
        let store = self.engine.store();
        let text = |key: &str| body[key].as_str().unwrap_or("");
        let q = |key: &str| query.get(key).map(String::as_str).unwrap_or("");
        match (request.method.as_str(), path) {
            ("GET", "/api/resolve") => {
                return Ok(json!({"id":store.resolve_id(q("kind"),q("prefix"))?}))
            }
            #[cfg(unix)]
            ("GET", "/api/plugins") => {
                return crate::plugins::catalog(
                    self.engine.paths(),
                    &Workspace::open(&self.workspace()?)?,
                );
            }
            #[cfg(unix)]
            ("POST", "/api/plugins/preview") => {
                return crate::plugins::preview(&crate::plugins::resolve(body)?);
            }
            #[cfg(unix)]
            ("POST", "/api/plugins/install" | "/api/plugins/remove") => {
                let workspace = self.mutable_workspace()?;
                ensure!(
                    text("workspace") == workspace.path.to_string_lossy(),
                    "Project changed; refresh plugins before changing an installation"
                );
                let removing = path.ends_with("/remove");
                let result = if removing {
                    crate::plugins::remove(
                        self.engine.paths(),
                        &workspace,
                        text("name"),
                        text("hash"),
                    )?
                } else {
                    crate::plugins::install(
                        self.engine.paths(),
                        &workspace,
                        crate::plugins::resolve(body)?,
                        text("hash"),
                    )?
                };
                store.add_event(
                    "plugin.installation",
                    &json!({"workspace":workspace.path,"removed":removing,"result":result}),
                    None,
                    None,
                )?;
                return Ok(
                    json!({"result":result,"catalog":crate::plugins::catalog(self.engine.paths(), &workspace)?}),
                );
            }
            #[cfg(unix)]
            ("POST", "/api/sqlite") => {
                return crate::sqlite::inspect(
                    Arc::new(Workspace::open(&self.workspace()?)?),
                    serde_json::from_value(body.clone())?,
                    CancellationToken::new(),
                )
                .await
            }
            ("GET", "/api/workspace/understand") => return self.project_map(false).await,
            ("POST", "/api/workspace/understand") => {
                return self.project_map(body["save"] == true).await
            }
            ("GET", "/api/workspace/why") => {
                return self
                    .change_history(
                        q("path"),
                        if q("count").is_empty() {
                            8
                        } else {
                            q("count").parse().context("Invalid history count")?
                        },
                    )
                    .await
            }
            ("GET", "/api/doctor") => return self.doctor(q("test_model") == "true").await,
            #[cfg(unix)]
            ("GET", "/api/mcp/servers") => {
                let workspace = Workspace::open(&self.workspace()?)?;
                let config = Config::load(self.engine.paths(), Some(&workspace.path))?;
                return crate::mcp::registry::catalog(&workspace, &config);
            }
            #[cfg(unix)]
            ("POST", "/api/mcp/servers" | "/api/mcp/servers/delete") => {
                let workspace = Workspace::open(&self.workspace()?)?;
                let config = Config::update(self.engine.paths(), |config| {
                    if path.ends_with("/delete") {
                        crate::mcp::registry::remove_server(config, text("server"), text("hash"))?;
                    } else {
                        crate::mcp::registry::save_server(
                            config,
                            body["definition"].clone(),
                            text("hash"),
                        )?;
                    }
                    Ok(())
                })?;
                store.add_event("mcp.registration", &json!({"workspace":workspace.path,"server":if path.ends_with("/delete") { text("server").to_owned() } else { format!("config:{}",body["definition"]["name"].as_str().unwrap_or("")) },"removed":path.ends_with("/delete")}), None, None)?;
                return crate::mcp::registry::catalog(&workspace, &config);
            }
            #[cfg(unix)]
            ("POST", "/api/mcp/activation") => {
                let workspace = Workspace::open(&self.workspace()?)?;
                ensure!(
                    text("workspace") == workspace.path.to_string_lossy(),
                    "Project changed; reload MCP servers before enabling one"
                );
                let enabled = body["enabled"]
                    .as_bool()
                    .context("Choose whether to enable this MCP server")?;
                let config = Config::update(self.engine.paths(), |config| {
                    let effective = Config::load(self.engine.paths(), Some(&workspace.path))?;
                    if enabled {
                        let entry =
                            crate::mcp::registry::read(&workspace, &effective, text("server"))?;
                        crate::mcp::registry::authorize_start(&workspace, &effective, &entry)?;
                    }
                    crate::mcp::registry::activate(
                        &workspace,
                        config,
                        text("server"),
                        text("hash"),
                        enabled,
                    )?;
                    Ok(())
                })?;
                store.add_event("mcp.activation", &json!({"workspace":workspace.path,"server":text("server"),"hash":text("hash"),"enabled":enabled}), None, None)?;
                return crate::mcp::registry::catalog(&workspace, &config);
            }
            ("GET", "/api/hooks") => {
                let workspace = Workspace::open(&self.workspace()?)?;
                let config = Config::load(self.engine.paths(), Some(&workspace.path))?;
                return Ok(crate::hooks::catalog(&workspace, &config));
            }
            ("POST", "/api/hooks/activation") => {
                let workspace = Workspace::open(&self.workspace()?)?;
                ensure!(
                    text("workspace") == workspace.path.to_string_lossy(),
                    "Project changed; reload hooks before enabling a command"
                );
                let enabled = body["enabled"]
                    .as_bool()
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
                        text("path"),
                        text("hash"),
                        enabled,
                    )?;
                    Ok(())
                })?;
                store.add_event("hook.activation",&json!({"workspace":workspace.path,"path":text("path"),"hash":text("hash"),"enabled":enabled}),None,None)?;
                return Ok(crate::hooks::catalog(&workspace, &config));
            }
            ("GET", "/api/commands") => return self.command_catalog(),
            ("POST", "/api/memory") => return self.memory(body),
            ("POST", "/api/commands/run") => return self.run_command(body).await,
            ("GET", "/api/version") => {
                return Ok(
                    json!({"name":"ShadowCode","version":crate::VERSION,"runtime":"rust","transport":"native","pid":std::process::id()}),
                )
            }
            ("GET", "/api/health") => {
                let cfg = self.config()?;
                return Ok(
                    json!({"ok":true,"app":"ShadowCode","version":crate::VERSION,"workspace":self.workspace()?,"model":cfg.model,"onboarding":cfg.onboarding,"provider":{"ok":cfg.model.provider!="mock","name":cfg.model.provider,"detail":if cfg.model.provider=="mock"{"Select a model to run coding tasks"}else{"Configured; use Test model to verify connectivity"}},"runtime":"rust"}),
                );
            }
            ("GET", "/api/workspace/status") => {
                let cfg = self.config()?;
                return Ok(
                    json!({"workspace":self.workspace()?,"model":cfg.model,"permissions":cfg.permissions,"onboarding":cfg.onboarding,"routing":cfg.routing}),
                );
            }
            ("GET", "/api/config") => return Ok(json!(self.config()?)),
            ("GET", "/api/background") => {
                let tasks = self
                    .engine
                    .background()
                    .list(&self.workspace()?)?
                    .into_iter()
                    .map(|mut task| {
                        if task.command.len() > 4000 {
                            task.command = format!("{}…", truncate(&task.command, 4000));
                        }
                        let clipped = task.output.len() > 4000;
                        if clipped {
                            let mut cut = task.output.len() - 4000;
                            while !task.output.is_char_boundary(cut) {
                                cut += 1;
                            }
                            task.output.drain(..cut);
                        }
                        let mut value = json!(task);
                        value["output_preview_truncated"] = json!(clipped);
                        value
                    })
                    .collect::<Vec<_>>();
                return Ok(json!({"tasks":tasks}));
            }
            ("POST", "/api/background") => {
                let selection = self
                    .selection
                    .read()
                    .map_err(|_| anyhow::anyhow!("Project selection lock poisoned"))?
                    .clone();
                let config = Config::load(self.engine.paths(), Some(&selection.workspace))?;
                return Ok(json!(self.engine.background().start(
                    &selection.workspace,
                    &config,
                    selection.session,
                    text("name"),
                    text("command")
                )?));
            }
            ("PUT", "/api/config") => {
                let mut values = body["values"].clone();
                ensure!(values.is_object(), "values must be an object");
                if let Some(object) = values.as_object_mut() {
                    object.remove("api_key");
                }
                let cfg = Config::update(self.engine.paths(), |cfg| {
                    // Settings use the provider's model name as `default`. Give
                    // that configuration a stable identity before saving it.
                    if values["model"].is_object() {
                        let old = cfg.model.clone();
                        let mut merged = serde_json::to_value(&old)?;
                        config::merge(&mut merged, values["model"].clone());
                        let mut model: ModelConfig = serde_json::from_value(merged)?;
                        if model.default == model.name || !model_registry::same_target(&old, &model)
                        {
                            model.default = model_registry::model_id(
                                &model.provider,
                                &model.endpoint,
                                &model.name,
                            );
                        }
                        model_registry::validate(&model)?;
                        self.check_model_identity(&model)?;
                        self.register(&old)?;
                        values["model"] = json!(model);
                    }
                    let mut preview = serde_json::to_value(&*cfg)?;
                    config::merge(&mut preview, values.clone());
                    let updated: Config = serde_json::from_value(preview)?;
                    updated.validate()?;
                    if !text("api_key").is_empty() {
                        let key = if text("api_key_env").is_empty() {
                            values
                                .pointer("/model/api_key_env")
                                .and_then(Value::as_str)
                                .unwrap_or(&cfg.model.api_key_env)
                        } else {
                            text("api_key_env")
                        };
                        config::set_secret(self.engine.paths(), key, text("api_key"))?;
                    }
                    *cfg = updated;
                    Ok(())
                })?;
                self.register(&cfg.model)?;
                return Ok(json!(cfg));
            }
            ("GET", "/api/routing") => {
                let cfg = self.config()?;
                self.register(&cfg.model)?;
                return routing::view(&store, &cfg);
            }
            ("PUT", "/api/routing") => {
                let values = &body["values"];
                routing::validate(values)?;
                let cfg = self.config()?;
                // An explicit edit must name a known model. Missing models in
                // previously saved configurations are handled as visible fallbacks.
                for role in routing::PURPOSES {
                    if let Some(id) = values[role]
                        .as_str()
                        .filter(|id| !matches!(*id, "" | "default" | "mock"))
                    {
                        let model = model_registry::resolve(&store, id, &cfg.model)?;
                        ensure!(model.provider != "mock", "Choose a coding model for {role}");
                    }
                }
                let cfg = Config::patch(self.engine.paths(), json!({"routing":values}))?;
                return routing::view(&store, &cfg);
            }
            ("GET", "/api/onboarding") => {
                let cfg = self.config()?;
                let providers = self.detected(false).await;
                let default = if cfg.model.provider != "mock" {
                    cfg.model.provider.as_str()
                } else if providers
                    .iter()
                    .any(|p| p["provider"] == "ollama" && p["running"] == true)
                {
                    "ollama"
                } else {
                    "mock"
                };
                return Ok(
                    json!({"completed":cfg.onboarding["completed"].as_bool().unwrap_or(false),"suggested_workspace":self.workspace()?,"providers":models::presets(),"detected":providers,"levels":["read_only","workspace","elevated"],"defaults":{"provider":default,"permission_level":"workspace","theme":"light"}}),
                );
            }
            ("POST", "/api/onboarding") => {
                let workspace = expand_path(text("workspace"))?;
                let workspace = Workspace::open(&workspace)?.path;
                let cfg = Config::update(self.engine.paths(), |cfg| {
                    cfg.model = self.model_from_body(body, &cfg.model);
                    cfg.permissions.level =
                        serde_json::from_value(json!(if text("permission_level").is_empty() {
                            "workspace"
                        } else {
                            text("permission_level")
                        }))?;
                    cfg.permissions.network = body["network"].as_bool().unwrap_or(false);
                    cfg.ui["theme"] = json!(if text("theme").is_empty() {
                        "light"
                    } else {
                        text("theme")
                    });
                    cfg.onboarding = json!({"completed":true,"workspace":workspace});
                    if !cfg.is_trusted(&workspace) {
                        cfg.trusted_workspaces
                            .push(workspace.to_string_lossy().into_owned());
                    }
                    cfg.validate()?;
                    if !text("api_key").is_empty() {
                        config::set_secret(
                            self.engine.paths(),
                            &cfg.model.api_key_env,
                            text("api_key"),
                        )?;
                    }
                    Ok(())
                })?;
                let session = store.create_session(&workspace, &cfg.model.default, "Welcome")?;
                let sid = session["id"]
                    .as_str()
                    .context("Session missing ID")?
                    .to_owned();
                self.select(&workspace, Some(sid.clone()))?;
                self.register(&cfg.model)?;
                return Ok(json!({"ok":true,"workspace":workspace,"session_id":sid}));
            }
            ("GET", "/api/providers/detect") => {
                return Ok(json!({"providers":self.detected(q("refresh")=="1").await}))
            }
            ("GET", "/api/providers") => {
                let detected = self.detected(false).await;
                let mut presets = models::presets();
                for preset in &mut presets {
                    if let Some(found) = detected
                        .iter()
                        .find(|v| v["provider"] == preset["provider"])
                    {
                        preset["running"] = found["running"].clone();
                    }
                }
                return Ok(json!({"providers":presets}));
            }
            ("GET", "/api/models") => {
                if q("detect") != "false" {
                    model_registry::record_detected(
                        &store,
                        &self.detected(q("refresh") == "1").await,
                    )?;
                }
                let cfg = self.config()?;
                self.register(&cfg.model)?;
                return Ok(json!({"models":model_registry::catalog(&store,&cfg.model)?}));
            }
            ("POST", "/api/models/test") => {
                let cfg = self.config()?;
                let model = self.model_from_body(body, &cfg.model);
                ensure!(
                    model.provider != "mock",
                    "The offline preview is not a coding model"
                );
                let started = Instant::now();
                let client = ModelClient::new(model.clone(), self.engine.paths())?;
                let cancel = CancellationToken::new();
                let result=tokio::time::timeout(Duration::from_secs(45),client.chat(&[json!({"role":"user","content":"Reply with one short sentence confirming you can respond."})],&[],cancel.clone(),|_|{})).await;
                return Ok(match result {
                    Ok(Ok(reply)) => {
                        json!({"ok":!reply.text.trim().is_empty(),"reply":truncate(&reply.text,4000),"usage":reply.usage,"latency_ms":started.elapsed().as_millis(),"model":model.name,"capabilities":{"completion":true}})
                    }
                    Ok(Err(error)) => {
                        json!({"ok":false,"error":format!("{error:#}"),"latency_ms":started.elapsed().as_millis()})
                    }
                    Err(_) => {
                        cancel.cancel();
                        json!({"ok":false,"error":"Model test timed out after 45 seconds","latency_ms":started.elapsed().as_millis()})
                    }
                });
            }
            ("POST", "/api/models/register" | "/api/models/select") => {
                let cfg = self.config()?;
                let model = if !text("provider").is_empty() {
                    self.model_from_body(body, &cfg.model)
                } else {
                    self.resolve_model(text("id"), &cfg.model)?
                };
                self.check_model_identity(&model)?;
                self.register(&cfg.model)?;
                self.register(&model)?;
                if path.ends_with("select") {
                    Config::patch(self.engine.paths(), json!({"model":model}))?;
                }
                return Ok(json!({"ok":true,"model":model}));
            }
            ("GET", "/api/projects") => return Ok(json!({"projects":store.projects()?})),
            ("POST", "/api/projects" | "/api/projects/trust") => {
                let workspace = Workspace::open(&expand_path(text("path"))?)?.path;
                let cfg = if path.ends_with("trust") {
                    Config::update(self.engine.paths(), |cfg| {
                        if !cfg.is_trusted(&workspace) {
                            cfg.trusted_workspaces
                                .push(workspace.to_string_lossy().into_owned());
                        }
                        Ok(())
                    })?
                } else {
                    Config::load(self.engine.paths(), None)?
                };
                if !cfg.is_trusted(&workspace) {
                    return Ok(
                        json!({"needs_trust":true,"path":workspace,"name":workspace.file_name(),"permissions":cfg.permissions}),
                    );
                }
                let session = store.create_session(
                    &workspace,
                    &cfg.model.default,
                    workspace
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Project"),
                )?;
                let sid = session["id"]
                    .as_str()
                    .context("Missing session ID")?
                    .to_owned();
                self.select(&workspace, Some(sid.clone()))?;
                return Ok(
                    json!({"ok":true,"needs_trust":false,"path":workspace,"session_id":sid}),
                );
            }
            ("GET", "/api/sessions") => {
                return Ok(
                    json!({"sessions":store.sessions_in(q("q"),query_limit(&query,100,10000),(!q("workspace").is_empty()).then(||Path::new(q("workspace"))))?}),
                )
            }
            ("GET", "/api/goals") => {
                let workspace = self.workspace()?;
                return Ok(
                    json!({"goals":store.goals(if matches!(q("all"),"true"|"1"){None}else{Some(&workspace)})?}),
                );
            }
            ("POST", "/api/goals") => {
                let workspace = if text("workspace").is_empty() {
                    self.workspace()?
                } else {
                    Workspace::open(&expand_path(text("workspace"))?)?.path
                };
                if let Some(sid) = body["session_id"].as_str().filter(|s| !s.is_empty()) {
                    ensure!(
                        store.session(sid)?.context("Session not found")?["workspace"].as_str()
                            == workspace.to_str(),
                        "Session belongs to a different workspace"
                    );
                }
                let milestones = if body.get("milestones").is_some() {
                    serde_json::from_value::<Vec<MilestoneSpec>>(body["milestones"].clone())?
                } else {
                    MilestoneSpec::default_plan()
                };
                let goal = store.create_goal(&workspace, text("instruction"), &milestones)?;
                if body["run"].as_bool().unwrap_or(false) {
                    let goal = self.engine.start_goal(
                        goal["id"].as_str().context("Goal ID missing")?,
                        body["session_id"].as_str().filter(|s| !s.is_empty()),
                    )?;
                    self.select(&workspace, goal["session_id"].as_str().map(str::to_owned))?;
                    return Ok(goal);
                }
                return Ok(goal);
            }
            ("POST", "/api/sessions") => {
                let workspace = if text("workspace").is_empty() {
                    self.workspace()?
                } else {
                    Workspace::open(&expand_path(text("workspace"))?)?.path
                };
                let cfg = Config::load(self.engine.paths(), Some(&workspace))?;
                let session =
                    store.create_session(&workspace, &cfg.model.default, text("title"))?;
                self.select(&workspace, session["id"].as_str().map(str::to_owned))?;
                return Ok(session);
            }
            ("GET", "/api/jobs") => return Ok(json!({"jobs":store.active_and_recent_jobs(1000)?})),
            ("GET", "/api/jobs/current") => {
                let job = store.current_job(q("session_id"), q("include_finished") == "true")?;
                return Ok(json!({"job":job}));
            }
            ("POST", "/api/jobs/test") => {
                let workspace = self.workspace()?;
                if let Some(path) = body["workspace"].as_str() {
                    ensure!(
                        Workspace::open(Path::new(path))?.path == workspace,
                        "Test task belongs to another workspace"
                    );
                }
                let command = if text("command").trim().is_empty() {
                    crate::project::test_command(
                        &crate::project::inspect(Arc::new(Workspace::open(&workspace)?)).await?,
                    )?
                } else {
                    text("command").to_owned()
                };
                let job = self
                    .engine
                    .start_command(
                        StartRequest {
                            workspace,
                            task: String::new(),
                            session_id: body["session_id"].as_str().map(str::to_owned),
                            model: None,
                            mode: "command".into(),
                            queue: body["queue"].as_bool().unwrap_or(false),
                        },
                        crate::engine::CommandRequest {
                            command,
                            timeout_sec: body["timeout"].as_u64().unwrap_or(300),
                        },
                        self.job_owner.as_ref(),
                    )
                    .await?;
                return Ok(json!(job));
            }
            ("POST", "/api/jobs" | "/api/run") => {
                let selection = self
                    .selection
                    .read()
                    .map_err(|_| anyhow::anyhow!("Project lock poisoned"))?
                    .clone();
                let workspace = if text("workspace").is_empty() {
                    selection.workspace.clone()
                } else {
                    Workspace::open(&expand_path(text("workspace"))?)?.path
                };
                let cfg = Config::load(self.engine.paths(), Some(&workspace))?;
                ensure!(
                    cfg.is_trusted(&workspace),
                    "Trust this project before starting an agent task"
                );
                let purpose = text("purpose");
                let mode = match purpose {
                    "planner" | "plan" | "planning" | "researcher" | "architecture" => "plan",
                    "reviewer" | "review" => "review",
                    _ => "code",
                };
                let model = if text("model").is_empty() {
                    None
                } else {
                    Some(self.resolve_model(text("model"), &cfg.model)?)
                };
                let job = self
                    .engine
                    .start_limited_owned(
                        StartRequest {
                            workspace: workspace.clone(),
                            task: text("task").into(),
                            session_id: body["session_id"]
                                .as_str()
                                .filter(|s| !s.is_empty())
                                .map(str::to_owned)
                                .or_else(|| {
                                    if workspace == selection.workspace {
                                        selection.session.clone()
                                    } else {
                                        None
                                    }
                                }),
                            model,
                            mode: mode.into(),
                            queue: body["queue"].as_bool().unwrap_or(false),
                        },
                        purpose,
                        body.get("permission_limit")
                            .filter(|v| !v.is_null())
                            .map(|v| serde_json::from_value(v.clone()))
                            .transpose()?,
                        self.job_owner.as_ref(),
                    )
                    .await?;
                self.select_if(
                    &job.workspace,
                    Some(job.session_id.clone()),
                    Some(selection.generation),
                )?;
                return Ok(json!(job));
            }
            ("GET", "/api/approvals") => {
                return Ok(
                    json!({"approvals":self.engine.approvals().list((!q("session_id").is_empty()).then_some(q("session_id")))}),
                )
            }
            ("GET", "/api/events") => {
                let sid = if !q("session_id").is_empty() {
                    q("session_id").to_owned()
                } else {
                    self.selection
                        .read()
                        .map_err(|_| anyhow::anyhow!("Project lock poisoned"))?
                        .session
                        .clone()
                        .unwrap_or_default()
                };
                return Ok(
                    json!({"events":store.recent_events(&sid,query_limit(&query,240,10000))?}),
                );
            }
            ("GET", "/api/workspace/files") => {
                let workspace = Workspace::open(&self.workspace()?)?;
                let path = if q("path").is_empty() { "." } else { q("path") };
                let relative = workspace.relative(path)?;
                return Ok(
                    json!({"entries":workspace.list(path)?,"workspace":workspace.path,"path":relative,"parent":if relative==Path::new("."){String::new()}else{relative.parent().filter(|p|!p.as_os_str().is_empty()).unwrap_or(Path::new(".")).to_string_lossy().into_owned()}}),
                );
            }
            ("GET", "/api/workspace/file") => {
                let file = Workspace::open(&self.workspace()?)?.read(q("path"))?;
                return Ok(
                    json!({"path":file.path,"content":truncate(&file.content,200000),"hash":file.hash,"truncated":file.content.len()>200000}),
                );
            }
            ("GET", "/api/workspace/instructions") => {
                let ws = Workspace::open(&self.workspace()?)?;
                let snapshot = ws.snapshot(".shadow/instructions.md")?;
                return Ok(
                    json!({"exists":snapshot.bytes.is_some(),"content":snapshot.bytes.map(String::from_utf8).transpose()?.unwrap_or_default(),"path":".shadow/instructions.md"}),
                );
            }
            ("PUT", "/api/workspace/instructions") => {
                let ws = self.mutable_workspace()?;
                ws.write(".shadow/instructions.md", text("content").as_bytes(), None)?;
                return Ok(json!({"ok":true}));
            }
            ("GET", "/api/workspace/skills") => {
                let ws = Workspace::open(&self.workspace()?)?;
                let catalog = crate::workflows::discover(&ws);
                let skills: Vec<_> = catalog
                    .definitions
                    .into_iter()
                    .filter(|item| item.info.kind == "skill")
                    .collect();
                return Ok(json!({"skills":skills,"issues":catalog.issues}));
            }
            ("PUT", "/api/workspace/skills") => {
                let name = text("name");
                ensure!(
                    !name.is_empty()
                        && name.len() <= 80
                        && name
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')),
                    "Skill name may contain letters, numbers, - and _"
                );
                let ws = self.mutable_workspace()?;
                let path = format!(".shadow/skills/{name}.md");
                crate::workflows::Definition::parse(&path, "skill", text("content"), "")?;
                ws.write(
                    &path,
                    text("content").as_bytes(),
                    body["expected_hash"].as_str(),
                )?;
                return Ok(json!({"ok":true}));
            }
            ("POST", "/api/workspace/attach") => {
                let name = Path::new(text("filename"))
                    .file_name()
                    .and_then(|v| v.to_str())
                    .context("Invalid attachment name")?;
                ensure!(
                    !name.is_empty() && name.len() <= 200,
                    "Invalid attachment name"
                );
                let path = format!(".shadow/attachments/{}-{name}", crate::id());
                self.mutable_workspace()?
                    .write(&path, text("text").as_bytes(), Some("missing"))?;
                return Ok(json!({"path":path}));
            }
            ("POST", "/api/workspace/exec") => {
                let selection = self.snapshot_selection()?;
                let workspace = if text("workspace").is_empty() {
                    selection.workspace.clone()
                } else {
                    expand_path(text("workspace"))?
                };
                let ws = self.mutable_workspace_at(&workspace)?;
                let cfg = Config::load(self.engine.paths(), Some(&ws.path))?;
                let command = text("command");
                ensure!(
                    !command.trim().is_empty() && command.len() <= 64000,
                    "Invalid command"
                );
                if let Decision::Deny(reason) =
                    permissions::check(&cfg.permissions, "exec", &json!({"command":command}))
                {
                    bail!(reason);
                }
                // This route is the terminal Run button: the exact command was
                // supplied by the user. Agent commands use ApprovalHub instead.
                let session = body["session_id"].as_str().map(str::to_owned).or_else(|| {
                    (ws.path == selection.workspace)
                        .then_some(selection.session)
                        .flatten()
                });
                if let Some(sid) = &session {
                    ensure!(
                        store.session(sid)?.context("Session not found")?["workspace"].as_str()
                            == ws.path.to_str(),
                        "Session belongs to a different workspace"
                    );
                }
                let spec = ProcessSpec::shell(
                    command,
                    ws.path.clone(),
                    Duration::from_secs(
                        body["timeout"]
                            .as_u64()
                            .unwrap_or(60)
                            .clamp(1, cfg.agent.tool_timeout_sec),
                    ),
                );
                let result = process::run(spec, ws.reservation.cancellation(), None).await?;
                store.add_event(
                    "terminal.completed",
                    &json!({"command":command,"result":result}),
                    session.as_deref(),
                    None,
                )?;
                let mut result = json!(result);
                result["command"] = json!(command);
                return Ok(result);
            }
            ("GET", "/api/workspace/git") => return self.git_status().await,
            ("GET", "/api/workspace/diff") => return self.git_diff(q("path")).await,
            ("POST", "/api/workspace/diff/hunk") => return self.hunk_action(body).await,
            ("POST", "/api/workspace/git/add") => {
                let ws = self.mutable_workspace()?;
                let paths = body["paths"].as_array().context("paths must be an array")?;
                ensure!(
                    !paths.is_empty() && paths.len() <= 200,
                    "Choose files to stage"
                );
                let mut args = vec!["add".into(), "--".into()];
                for path in paths {
                    args.push(
                        ws.relative(path.as_str().context("Invalid path")?)?
                            .to_string_lossy()
                            .into_owned(),
                    );
                }
                let result = Self::git_in(&ws.path, args, ws.reservation.cancellation()).await?;
                ensure!(result["ok"] == true, "{}", result["stderr"]);
                return Ok(json!({"ok":true}));
            }
            ("POST", "/api/workspace/git/commit") => {
                let ws = self.mutable_workspace()?;
                ensure!(
                    !text("message").trim().is_empty() && text("message").len() <= 32000,
                    "Commit message required"
                );
                let result = Self::git_in(
                    &ws.path,
                    vec![
                        "-c".into(),
                        "commit.gpgSign=false".into(),
                        "commit".into(),
                        "-m".into(),
                        text("message").into(),
                    ],
                    ws.reservation.cancellation(),
                )
                .await?;
                ensure!(result["ok"] == true, "{}", result["stderr"]);
                return Ok(json!({"ok":true}));
            }
            _ => {}
        }
        if parts.get(1) == Some(&"background") && parts.len() >= 3 {
            let task = self.engine.background().get(parts[2])?;
            ensure!(
                Path::new(&task.cwd) == self.workspace()?,
                "Switch to this process's project before managing it"
            );
            match (request.method.as_str(), parts.get(3).copied(), parts.len()) {
                ("GET", None, 3) => return Ok(json!(task)),
                ("POST", Some("stop"), 4) => {
                    return Ok(json!(self.engine.background().stop(parts[2]).await?))
                }
                _ => {}
            }
        }
        if parts.get(1) == Some(&"goals") && parts.len() >= 3 {
            let gid = parts[2];
            match (request.method.as_str(), parts.get(3).copied(), parts.len()) {
                ("GET", None, 3) => return store.goal(gid),
                ("DELETE", None, 3) => {
                    self.engine.delete_goal(gid)?;
                    return Ok(json!({"ok":true}));
                }
                ("POST", Some("run"), 4) => {
                    let goal = self
                        .engine
                        .start_goal(gid, body["session_id"].as_str().filter(|s| !s.is_empty()))?;
                    self.select(
                        Path::new(
                            goal["workspace"]
                                .as_str()
                                .context("Goal workspace missing")?,
                        ),
                        goal["session_id"].as_str().map(str::to_owned),
                    )?;
                    return Ok(goal);
                }
                ("POST", Some("pause" | "abandon"), 4) => {
                    return self.engine.stop_goal(gid, parts[3] == "abandon").await
                }
                ("POST", Some("milestones"), 5) => {
                    return self.engine.update_goal_milestone(
                        gid,
                        parts[4],
                        text("status"),
                        text("detail"),
                    )
                }
                _ => {}
            }
        }
        if parts.get(1) == Some(&"sessions") && parts.len() >= 3 {
            let sid = parts[2];
            let session = store.session(sid)?.context("Session not found")?;
            match (request.method.as_str(), parts.get(3).copied()) {
                ("GET", None) => {
                    return if q("summary") == "true" {
                        Ok(session)
                    } else {
                        self.session(sid)
                    }
                }
                ("PATCH", None) => {
                    store.rename_session(sid, text("title"))?;
                    return Ok(json!({"ok":true}));
                }
                ("DELETE", None) => {
                    self.engine.delete_session(sid)?;
                    let mut selection = self
                        .selection
                        .write()
                        .map_err(|_| anyhow::anyhow!("Project lock poisoned"))?;
                    if selection.session.as_deref() == Some(sid) {
                        selection.session = None;
                    }
                    return Ok(json!({"ok":true}));
                }
                ("POST", Some("activate")) => {
                    self.select(
                        Path::new(
                            session["workspace"]
                                .as_str()
                                .context("Session missing workspace")?,
                        ),
                        Some(sid.into()),
                    )?;
                    return self.session(sid);
                }
                ("POST", Some("branch")) => {
                    let workspace = PathBuf::from(
                        store.session(sid)?.context("Conversation does not exist")?["workspace"]
                            .as_str()
                            .context("Conversation has no workspace")?,
                    );
                    let memory =
                        crate::memory::archive(self.engine.paths(), &store, &workspace, sid)?;
                    return store.branch_session_with_memory(sid, text("title"), &memory);
                }
                ("GET", Some("events")) => {
                    if !q("before").is_empty() {
                        let before: i64 = q("before").parse().context("Invalid history cursor")?;
                        ensure!(before > 0, "History cursor must be positive");
                        return Ok(
                            json!({"events":store.recent_events_through(sid,before-1,query_limit(&query,256,2000))?}),
                        );
                    }
                    return Ok(
                        json!({"events":store.events_after(sid,q("after").parse().unwrap_or(0),None,query_limit(&query,512,10000))?}),
                    );
                }
                ("GET", Some("export")) => return self.export(sid, q("format")),
                ("GET", Some("cost")) => {
                    let tasks: Vec<_> = store.tasks(sid, 10000)?.into_iter().map(|task| json!({"task_id":task["id"],"prompt":task["prompt"],"status":task["status"],"usage":task["usage_json"].as_str().and_then(|v|serde_json::from_str::<Value>(v).ok()).unwrap_or(json!({}))})).collect();
                    return Ok(
                        json!({"session_id":sid,"tasks":tasks,"usage":session["usage_json"].as_str().and_then(|v|serde_json::from_str::<Value>(v).ok()).unwrap_or(json!({})),"cost":null,"note":"Provider pricing is not configured; token usage is shown."}),
                    );
                }
                ("GET", Some("pins")) => return Ok(json!({"pins":store.pins(sid)?})),
                ("POST", Some("pins")) => {
                    return Ok(json!({"id":store.add_pin(sid,text("label"),text("body"))?}))
                }
                ("DELETE", Some("pins")) => {
                    store.delete_pin(sid, parts.get(4).context("Pin ID required")?.parse()?)?;
                    return Ok(json!({"ok":true}));
                }
                _ => {}
            }
        }
        if parts.get(1) == Some(&"jobs") && parts.len() >= 3 {
            let job = self.engine.job(parts[2])?.context("Job not found")?;
            match (request.method.as_str(), parts.get(3).copied()) {
                ("GET", None) => return Ok(json!(job)),
                ("POST", Some("cancel")) => {
                    return Ok(json!(if body["only_if_queued"] == true {
                        self.engine.cancel_queued(&job.id).await?
                    } else {
                        self.engine.cancel(&job.id).await?
                    }))
                }
                ("GET", Some("events")) => {
                    return Ok(
                        json!({"events":store.events_after(&job.session_id,q("after").parse().unwrap_or(0),job.finished_at.map(|_|job.event_cursor),query_limit(&query,512,2000))?,"job":job}),
                    )
                }
                _ => {}
            }
        }
        if parts.get(1) == Some(&"approvals") && parts.len() == 3 && request.method == "POST" {
            let sid = if text("session_id").is_empty() {
                self.current_session()?
                    .context("Select the task waiting for approval")?
            } else {
                text("session_id").to_owned()
            };
            ensure!(
                matches!(text("decision"), "approve" | "deny"),
                "Choose approve or deny"
            );
            return Ok(json!(self.engine.approvals().decide(
                parts[2],
                &sid,
                text("decision") == "approve"
            )?));
        }
        if parts.get(1) == Some(&"checkpoints")
            && parts.get(2) == Some(&"tasks")
            && parts.len() >= 4
        {
            let task = store.task(parts[3])?.context("Task not found")?;
            let session = store
                .session(task["session_id"].as_str().context("Task has no session")?)?
                .context("Session not found")?;
            let ws = Workspace::open(Path::new(
                session["workspace"]
                    .as_str()
                    .context("Session has no workspace")?,
            ))?;
            if request.method == "GET" {
                let checkpoint = checkpoint::summary(&store, &ws, parts[3])?;
                return Ok(
                    json!({"rewindable":checkpoint["changes"].as_u64().unwrap_or(0)>0 && checkpoint["restored"]!=true,"checkpoint":checkpoint}),
                );
            }
            if request.method == "POST" && parts.get(4) == Some(&"restore") {
                ensure!(
                    ws.path == self.workspace()?,
                    "Activate this task's workspace before rewinding"
                );
                let _reservation = self.mutable_workspace()?;
                ensure!(
                    _reservation.path == ws.path,
                    "Project selection changed; activate this task before rewinding"
                );
                return Ok(json!({"ok":true,"restored":checkpoint::restore(&store,&ws,parts[3])?}));
            }
        }
        bail!(
            "Application command is not available: {} {}",
            request.method,
            path
        )
    }
    fn current_session(&self) -> Result<Option<String>> {
        Ok(self
            .selection
            .read()
            .map_err(|_| anyhow::anyhow!("Project lock poisoned"))?
            .session
            .clone())
    }
    fn mutable_workspace(&self) -> Result<ManualWorkspace> {
        self.mutable_workspace_at(&self.workspace()?)
    }
    fn mutable_workspace_at(&self, path: &Path) -> Result<ManualWorkspace> {
        let workspace = Workspace::open(path)?;
        let cfg = Config::load(self.engine.paths(), Some(&workspace.path))?;
        ensure!(
            cfg.permissions.level != PermissionLevel::ReadOnly,
            "This project is in read-only mode"
        );
        ensure!(
            cfg.is_trusted(&workspace.path),
            "Trust this project before changing files or running commands"
        );
        let reservation = self.engine.reserve_workspace(&workspace.path)?;
        Ok(ManualWorkspace {
            workspace,
            reservation,
        })
    }
    fn model_from_body(&self, body: &Value, fallback: &ModelConfig) -> ModelConfig {
        let provider = body["provider"]
            .as_str()
            .filter(|v| !v.is_empty())
            .unwrap_or(&fallback.provider);
        let preset = models::preset(provider);
        let provider_changed = provider != fallback.provider;
        let name = body["name"]
            .as_str()
            .filter(|v| !v.is_empty())
            .or_else(|| body["model"].as_str())
            .or_else(|| body["id"].as_str())
            .unwrap_or(&fallback.name);
        let endpoint = body["endpoint"]
            .as_str()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| {
                if provider_changed {
                    preset["endpoint"].as_str().unwrap_or("")
                } else {
                    &fallback.endpoint
                }
            });
        let target_changed = provider_changed
            || endpoint.trim_end_matches('/') != fallback.endpoint.trim_end_matches('/');
        ModelConfig {
            default: if body["id"]
                .as_str()
                .is_some_and(|id| !id.is_empty() && id != name)
            {
                body["id"].as_str().unwrap().into()
            } else {
                model_registry::model_id(provider, endpoint, name)
            },
            provider: provider.into(),
            name: name.into(),
            endpoint: endpoint.into(),
            api_key_env: body["api_key_env"]
                .as_str()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| {
                    if target_changed {
                        preset["api_key_env"].as_str().unwrap_or("OPENAI_API_KEY")
                    } else {
                        &fallback.api_key_env
                    }
                })
                .into(),
            context_limit: body["context_limit"]
                .as_u64()
                .map(|v| v as usize)
                .unwrap_or_else(|| {
                    if target_changed {
                        recommended_context(&json!(provider), None)
                    } else {
                        fallback.context_limit
                    }
                }),
        }
    }
    fn register(&self, model: &ModelConfig) -> Result<()> {
        model_registry::validate(model)?;
        let store = self.engine.store();
        let mut metadata = store
            .models()?
            .into_iter()
            .find(|row| {
                row["id"] == model.default
                    && row["provider"] == model.provider
                    && row["endpoint"] == model.endpoint
            })
            .map(|row| row["metadata"].clone())
            .filter(Value::is_object)
            .unwrap_or(json!({}));
        metadata["api_key_env"] = json!(model.api_key_env);
        store.upsert_model(&json!({"id":model.default,"name":model.name,"provider":model.provider,"endpoint":model.endpoint,"context_limit":model.context_limit,"metadata":metadata}))
    }
    fn check_model_identity(&self, model: &ModelConfig) -> Result<()> {
        let current = self.config()?.model;
        if current.default == model.default {
            ensure!(model_registry::same_target(&current, model), "Model ID already belongs to a different provider, endpoint, or model; choose a unique ID");
        }
        if let Some(row) = self
            .engine
            .store()
            .models()?
            .iter()
            .find(|row| row["id"] == model.default)
        {
            ensure!(model_registry::same_target(&model_registry::from_row(row)?, model), "Model ID already belongs to a different provider, endpoint, or model; choose a unique ID");
        }
        Ok(())
    }
    fn resolve_model(&self, id: &str, fallback: &ModelConfig) -> Result<ModelConfig> {
        model_registry::resolve(&self.engine.store(), id, fallback)
    }
    fn session(&self, id: &str) -> Result<Value> {
        let store = self.engine.store();
        let mut session = store.session(id)?.context("Session not found")?;
        let cursor = store.event_cursor(id)?;
        session["tasks"] = json!(store.tasks(id, 10000)?);
        session["events"] = json!(store.recent_events_through(id, cursor, 10000)?);
        session["event_cursor"] = json!(cursor);
        Ok(session)
    }
    fn export(&self, id: &str, format: &str) -> Result<Value> {
        let store = self.engine.store();
        let mut session = store.session(id)?.context("Session not found")?;
        let cursor = store.event_cursor(id)?;
        let mut after = 0;
        let mut events = Vec::new();
        let mut bytes = 0;
        loop {
            let page = store.events_after(id, after, Some(cursor), 1000)?;
            if page.is_empty() {
                break;
            }
            for event in &page {
                bytes += event.to_string().len();
            }
            ensure!(
                bytes <= 32_000_000,
                "This conversation exceeds the 32 MB export limit"
            );
            after = page
                .last()
                .and_then(|v| v["id"].as_i64())
                .context("Missing event cursor")?;
            events.extend(page);
        }
        session["events"] = json!(events);
        session["event_cursor"] = json!(cursor);
        session["tasks"] = json!(store.tasks(id, 10000)?);
        if format == "json" {
            let content = serde_json::to_string_pretty(&self.export_memory(session)?)?;
            ensure!(
                content.len() <= 32_000_000,
                "This conversation exceeds the 32 MB export limit"
            );
            return Ok(
                json!({"filename":format!("shadowcode-{id}.json"),"content":content,"mime":"application/json"}),
            );
        }
        let mut text = format!(
            "# {}\n\nWorkspace: `{}`\n\n",
            session["title"].as_str().unwrap_or("ShadowCode task"),
            session["workspace"].as_str().unwrap_or("")
        );
        for event in session["events"].as_array().into_iter().flatten() {
            match event["type"].as_str().unwrap_or("") {
                "user.message" => text.push_str(&format!(
                    "## User\n\n{}\n\n",
                    event["payload"]["text"].as_str().unwrap_or("")
                )),
                "model.delta" => text.push_str(&format!(
                    "## ShadowCode\n\n{}\n\n",
                    event["payload"]["text"].as_str().unwrap_or("")
                )),
                "tool.completed" => text.push_str(&format!(
                    "- Tool `{}`: {}\n",
                    event["payload"]["tool"].as_str().unwrap_or(""),
                    if event["payload"]["success"] == true {
                        "succeeded"
                    } else {
                        "failed"
                    }
                )),
                _ => {}
            }
        }
        let session = self.export_memory(session)?;
        if let Some(notes) = session["inherited_notes"]
            .as_str()
            .filter(|s| !s.is_empty())
        {
            text.push_str(&format!("\n## Inherited task notes\n\n{notes}\n"));
        }
        for (id, notes) in session["task_notes"].as_object().into_iter().flatten() {
            text.push_str(&format!(
                "\n## Task notes · {id}\n\n{}\n",
                notes.as_str().unwrap_or("")
            ));
        }
        ensure!(
            text.len() <= 32_000_000,
            "This conversation exceeds the 32 MB export limit"
        );
        Ok(json!({"filename":format!("shadowcode-{id}.md"),"content":text,"mime":"text/markdown"}))
    }
    fn export_memory(&self, mut session: Value) -> Result<Value> {
        let store = self.engine.store();
        let workspace = Path::new(
            session["workspace"]
                .as_str()
                .context("Conversation has no workspace")?,
        );
        let sid = session["id"].as_str().context("Conversation has no ID")?;
        let seed = store
            .query(
                "SELECT value FROM session_meta WHERE session_id=? AND key='memory_seed'",
                [sid],
            )?
            .pop();
        let mut notes = serde_json::Map::new();
        let mut bytes = session.to_string().len();
        for task in session["tasks"].as_array().into_iter().flatten() {
            let id = task["id"].as_str().context("Task has no ID")?;
            let note = crate::memory::task_notes(self.engine.paths(), &store, workspace, id)?;
            if !note.is_empty() {
                bytes += note.len();
                ensure!(
                    bytes <= 32_000_000,
                    "This conversation exceeds the 32 MB export limit"
                );
                notes.insert(id.to_owned(), json!(note));
            }
        }
        session["task_notes"] = json!(notes);
        session["inherited_notes"] = seed.map(|v| v["value"].clone()).unwrap_or(Value::Null);
        Ok(session)
    }
    async fn git_in(
        workspace: &Path,
        args: Vec<String>,
        cancel: CancellationToken,
    ) -> Result<Value> {
        let mut base = vec![
            "--no-pager".into(),
            "--no-optional-locks".into(),
            "--literal-pathspecs".into(),
            "-c".into(),
            "core.fsmonitor=false".into(),
            "-c".into(),
            "core.hooksPath=/dev/null".into(),
            "-c".into(),
            "color.ui=false".into(),
            "-c".into(),
            "core.quotepath=false".into(),
        ];
        base.extend(args);
        let refs: Vec<_> = base.iter().map(String::as_str).collect();
        Ok(json!(
            process::run(
                ProcessSpec::command("git", &refs, workspace.to_owned()),
                cancel,
                None
            )
            .await?
        ))
    }
    async fn git_status(&self) -> Result<Value> {
        let workspace = self.workspace()?;
        let git = |args| Self::git_in(&workspace, args, CancellationToken::new());
        let status = git(vec!["status".into(), "--porcelain=v1".into(), "-b".into()]).await?;
        if status["ok"] != true {
            return Ok(
                json!({"repo":false,"status":"","log":"","diff":"","files":[],"error":status["stderr"]}),
            );
        }
        let log = git(vec![
            "log".into(),
            "-12".into(),
            "--oneline".into(),
            "--no-show-signature".into(),
        ])
        .await?;
        let raw = git(vec!["status".into(), "--porcelain=v1".into(), "-z".into()]).await?;
        ensure!(raw["ok"] == true, "Git status failed: {}", raw["stderr"]);
        let mut files = Vec::new();
        let mut records = raw["stdout"].as_str().unwrap_or("").split('\0');
        while let Some(record) = records.next() {
            if record.len() < 4 {
                continue;
            }
            let label = &record[..2];
            let mut entry = json!({"path":&record[3..],"label":label.trim(),"index":&record[..1],"work":&record[1..2]});
            if label.contains('R') || label.contains('C') {
                entry["original_path"] = json!(records.next().unwrap_or(""));
            }
            files.push(entry);
        }
        Ok(
            json!({"repo":true,"status":status["stdout"],"porcelain":status["stdout"],"log":log["stdout"],"diff":"","files":files,"truncated":raw["truncated"]}),
        )
    }
    async fn git_diff(&self, path: &str) -> Result<Value> {
        Self::git_diff_in(&self.workspace()?, path, CancellationToken::new()).await
    }
    async fn git_diff_in(workspace: &Path, path: &str, cancel: CancellationToken) -> Result<Value> {
        let path = if path.is_empty() {
            String::new()
        } else {
            Workspace::open(workspace)?
                .relative(path)?
                .to_string_lossy()
                .into_owned()
        };
        let git = |args| Self::git_in(workspace, args, cancel.clone());
        let mut args = vec![
            "diff".into(),
            "--no-ext-diff".into(),
            "--no-textconv".into(),
            "--no-renames".into(),
        ];
        let mut staged = args.clone();
        staged.push("--cached".into());
        if !path.is_empty() {
            args.extend(["--".into(), path.clone()]);
            staged.extend(["--".into(), path.clone()]);
        }
        let mut normal = git(args).await?;
        let staged = git(staged).await?;
        ensure!(
            normal["ok"] == true && staged["ok"] == true,
            "Could not read Git changes: {} {}",
            normal["stderr"],
            staged["stderr"]
        );
        let mut untracked = false;
        if !path.is_empty() {
            let others = git(vec![
                "ls-files".into(),
                "--others".into(),
                "--exclude-standard".into(),
                "-z".into(),
                "--".into(),
                path.clone(),
            ])
            .await?;
            ensure!(
                others["ok"] == true && others["truncated"] != true,
                "Could not identify the selected file"
            );
            untracked = others["stdout"]
                .as_str()
                .unwrap_or("")
                .split('\0')
                .any(|entry| entry == path);
            if untracked {
                normal = git(vec![
                    "diff".into(),
                    "--no-index".into(),
                    "--no-ext-diff".into(),
                    "--no-textconv".into(),
                    "--".into(),
                    "/dev/null".into(),
                    path.clone(),
                ])
                .await?;
                ensure!(
                    normal["exit_code"] == 0 || normal["exit_code"] == 1,
                    "Could not preview new file: {}",
                    normal["stderr"]
                );
            }
        }
        let text = normal["stdout"].as_str().unwrap_or("");
        let staged_text = staged["stdout"].as_str().unwrap_or("");
        Ok(
            json!({"path":path,"diff":text,"staged":staged_text,"hunks":parse_hunks(text),"staged_hunks":parse_hunks(staged_text),"untracked":untracked,"binary":text.contains("Binary files ") || staged_text.contains("Binary files "),"truncated":normal["truncated"]==true || staged["truncated"]==true}),
        )
    }
    async fn hunk_action(&self, body: &Value) -> Result<Value> {
        let ws = self.mutable_workspace()?;
        let path = ws
            .relative(body["path"].as_str().context("Choose a file")?)?
            .to_string_lossy()
            .into_owned();
        ensure!(
            !path.contains(['\n', '\r', '\t', '"']),
            "Stage files with special path characters as a whole file"
        );
        let action = body["action"].as_str().unwrap_or("");
        ensure!(
            matches!(action, "accept" | "reject"),
            "Choose accept or reject"
        );
        let current = Self::git_diff_in(&ws.path, &path, ws.reservation.cancellation()).await?;
        ensure!(
            current["untracked"] != true,
            "Stage new files as a whole file"
        );
        ensure!(
            current["binary"] != true && current["truncated"] != true,
            "Stage binary or truncated changes as a whole file"
        );
        ensure!(
            current["hunks"]
                .as_array()
                .is_some_and(|hunks| hunks.contains(&body["hunk"])),
            "This diff has changed. Refresh it before applying a hunk"
        );
        let diff = current["diff"].as_str().context("Missing diff")?;
        let prefix = diff.split_once("\n@@ ").context("Missing hunk header")?.0;
        let mut patch = format!(
            "{prefix}\n{}\n",
            body["hunk"]["header"]
                .as_str()
                .context("Missing hunk header")?
        );
        for line in body["hunk"]["lines"]
            .as_array()
            .context("Missing hunk lines")?
        {
            patch.push_str(match line["kind"].as_str() {
                Some("add") => "+",
                Some("del") => "-",
                Some("meta") => "",
                _ => " ",
            });
            patch.push_str(line["text"].as_str().context("Missing hunk text")?);
            patch.push('\n');
        }
        let mut input = tempfile::NamedTempFile::new()?;
        input.write_all(patch.as_bytes())?;
        input.flush()?;
        let result = Self::git_in(
            &ws.path,
            vec![
                "apply".into(),
                "--recount".into(),
                "--unidiff-zero".into(),
                if action == "accept" {
                    "--cached"
                } else {
                    "--reverse"
                }
                .into(),
                "--".into(),
                input.path().to_string_lossy().into_owned(),
            ],
            ws.reservation.cancellation(),
        )
        .await?;
        ensure!(
            result["ok"] == true,
            "Could not apply hunk: {}",
            result["stderr"]
        );
        Ok(json!({"ok":true,"action":action,"path":path}))
    }
}
fn active(job: &Value) -> bool {
    matches!(
        job["status"].as_str(),
        Some("queued" | "running" | "cancelling")
    )
}
fn query_limit(query: &HashMap<String, String>, default: usize, max: usize) -> usize {
    query
        .get("limit")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(default)
        .clamp(1, max)
}
fn expand_path(path: &str) -> Result<PathBuf> {
    ensure!(!path.is_empty(), "Project path required");
    Ok(if path == "~" {
        PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?)
    } else if let Some(tail) = path.strip_prefix("~/") {
        PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?).join(tail)
    } else {
        PathBuf::from(path)
    })
}
fn recommended_context(provider: &Value, reported: Option<u64>) -> usize {
    model_registry::recommended_context(provider.as_str().unwrap_or(""), reported)
}
pub fn parse_hunks(diff: &str) -> Vec<Value> {
    let mut hunks = Vec::new();
    let mut current = None;
    for line in diff.lines() {
        if line.starts_with("diff --git ") || line.starts_with("@@ ") {
            if let Some(hunk) = current.take() {
                hunks.push(hunk);
            }
            if line.starts_with("@@ ") {
                current = Some(json!({"header":line,"lines":[]}));
            }
        } else if let Some(hunk) = &mut current {
            if let Some(lines) = hunk["lines"].as_array_mut() {
                let (kind, text) = match line.as_bytes().first() {
                    Some(b'+') => ("add", &line[1..]),
                    Some(b'-') => ("del", &line[1..]),
                    Some(b' ') => ("ctx", &line[1..]),
                    _ => ("meta", line),
                };
                lines.push(json!({"kind":kind,"text":text}));
            }
        }
    }
    if let Some(hunk) = current {
        hunks.push(hunk);
    }
    hunks
}
