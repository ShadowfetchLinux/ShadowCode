//! `/api/workspace…`: the selected project's files, instructions, skills,
//! attachments, the terminal Run button, project inspection and Git (the Git
//! helpers live in `git.rs`). Git, exec and inspection await processes; the
//! file routes are synchronous and run on the blocking pool.
use super::*;

#[derive(Default, Deserialize)]
#[serde(default)]
struct ExecBody {
    workspace: Text,
    command: Text,
    session_id: Text,
    timeout: Loose<u64>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct FileBody {
    name: Text,
    content: Text,
    expected_hash: Text,
    filename: Text,
    text: Text,
    data_base64: Text,
}

impl Service {
    pub(super) async fn workspace_routes(&self, call: &Arc<Call>) -> Result<Value> {
        match (call.method.as_str(), call.path.as_str()) {
            ("GET", "/api/workspace/understand") => self.project_map(false).await,
            ("POST", "/api/workspace/understand") => {
                self.project_map(call.body["save"] == true).await
            }
            ("GET", "/api/workspace/why") => {
                self.change_history(
                    call.q("path"),
                    if call.q("count").is_empty() {
                        8
                    } else {
                        call.q("count").parse().context("Invalid history count")?
                    },
                )
                .await
            }
            ("POST", "/api/workspace/exec") => self.exec(call).await,
            ("GET", "/api/workspace/git") => self.git_status().await,
            ("GET", "/api/workspace/diff") => self.git_diff(call.q("path")).await,
            ("POST", "/api/workspace/diff/hunk") => self.hunk_action(&call.body).await,
            ("POST", "/api/workspace/git/add") => self.git_add(&call.body).await,
            ("POST", "/api/workspace/git/commit") => self.git_commit(call.text("message")).await,
            _ => self.blocking(call, Self::workspace_files).await,
        }
    }
    fn workspace_files(&self, call: &Call) -> Result<Value> {
        let body: FileBody = call.body()?;
        match (call.method.as_str(), call.path.as_str()) {
            ("GET", "/api/workspace/status") => {
                let cfg = self.config()?;
                Ok(
                    json!({"workspace":self.workspace()?,"model":cfg.model,"permissions":cfg.permissions,"onboarding":cfg.onboarding,"routing":cfg.routing,"trusted":cfg.is_trusted(&self.workspace()?)}),
                )
            }
            ("GET", "/api/workspace/files") => {
                let workspace = Workspace::open(&self.workspace()?)?;
                let path = if call.q("path").is_empty() {
                    "."
                } else {
                    call.q("path")
                };
                let relative = workspace.relative(path)?;
                Ok(
                    json!({"entries":workspace.list(path)?,"workspace":workspace.path,"path":relative,"parent":if relative==Path::new("."){String::new()}else{relative.parent().filter(|p|!p.as_os_str().is_empty()).unwrap_or(Path::new(".")).to_string_lossy().into_owned()}}),
                )
            }
            ("GET", "/api/workspace/file") => {
                let file = Workspace::open(&self.workspace()?)?.read(call.q("path"))?;
                Ok(
                    json!({"path":file.path,"content":truncate(&file.content,200000),"hash":file.hash,"truncated":file.content.len()>200000}),
                )
            }
            ("GET", "/api/workspace/instructions") => {
                let ws = Workspace::open(&self.workspace()?)?;
                let snapshot = ws.snapshot(".shadow/instructions.md")?;
                Ok(
                    json!({"exists":snapshot.bytes.is_some(),"content":snapshot.bytes.map(String::from_utf8).transpose()?.unwrap_or_default(),"path":".shadow/instructions.md"}),
                )
            }
            ("PUT", "/api/workspace/instructions") => {
                let ws = self.mutable_workspace()?;
                ws.write(
                    ".shadow/instructions.md",
                    body.content.as_str().as_bytes(),
                    None,
                )?;
                Ok(json!({"ok":true}))
            }
            ("GET", "/api/workspace/skills") => {
                let ws = Workspace::open(&self.workspace()?)?;
                let catalog = crate::workflows::discover(&ws);
                let skills: Vec<_> = catalog
                    .definitions
                    .into_iter()
                    .filter(|item| item.info.kind == "skill")
                    .collect();
                Ok(json!({"skills":skills,"issues":catalog.issues}))
            }
            ("PUT", "/api/workspace/skills") => {
                let name = body.name.as_str();
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
                crate::workflows::Definition::parse(&path, "skill", body.content.as_str(), "")?;
                ws.write(
                    &path,
                    body.content.as_str().as_bytes(),
                    body.expected_hash.0.as_deref(),
                )?;
                Ok(json!({"ok":true}))
            }
            ("POST", "/api/workspace/attach") => {
                let name = Path::new(body.filename.as_str())
                    .file_name()
                    .and_then(|v| v.to_str())
                    .context("Invalid attachment name")?;
                ensure!(
                    !name.is_empty() && name.len() <= 200,
                    "Invalid attachment name"
                );
                let path = format!(".shadow/attachments/{}-{name}", crate::id());
                self.attachment_workspace()?.write(
                    &path,
                    body.text.as_str().as_bytes(),
                    Some("missing"),
                )?;
                Ok(json!({"path":path,"kind":"text"}))
            }
            ("POST", "/api/workspace/attach-image") => {
                let name = Path::new(body.filename.as_str())
                    .file_name()
                    .and_then(|v| v.to_str())
                    .context("Invalid image filename")?;
                let bytes = crate::vision::decode_data_base64(body.data_base64.as_str())?;
                let ws = self.attachment_workspace()?;
                let stored = crate::vision::store_attachment(&ws, name, &bytes)?;
                Ok(
                    json!({"path":stored.path,"mime":stored.mime,"bytes":stored.bytes,"kind":"image"}),
                )
            }
            _ => Err(call.unavailable()),
        }
    }
    /// POST /api/workspace/exec: the terminal Run button. The exact command
    /// was supplied by the user; agent commands use ApprovalHub instead.
    async fn exec(&self, call: &Call) -> Result<Value> {
        let body: ExecBody = call.body()?;
        let store = self.engine.store();
        let selection = self.snapshot_selection()?;
        let workspace = if body.workspace.is_empty() {
            selection.workspace.clone()
        } else {
            expand_path(body.workspace.as_str())?
        };
        let ws = self.mutable_workspace_at(&workspace)?;
        let cfg = Config::load(self.engine.paths(), Some(&ws.path))?;
        let command = body.command.as_str();
        ensure!(
            !command.trim().is_empty() && command.len() <= 64000,
            "Invalid command"
        );
        if let Decision::Deny(reason) =
            permissions::check(&cfg.permissions, "exec", &json!({"command":command}))
        {
            bail!(reason);
        }
        let session = body.session_id.0.clone().or_else(|| {
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
                body.timeout
                    .0
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
        Ok(result)
    }
}
