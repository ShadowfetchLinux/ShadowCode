//! `/api/worktrees…`, `/api/parallel…` and `/api/sandbox…` routes: managed
//! Git worktrees and the prepared parallel checkouts. The engine side lives
//! in `crate::worktrees` and `crate::parallel`.
use super::*;
use crate::worktrees;

#[derive(Default, Deserialize)]
#[serde(default)]
struct WorktreeBody {
    id: Text,
    hash: Text,
    reference: Text,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ParallelBody {
    goal: Text,
    worker_id: Text,
    status: Text,
}

impl Service {
    /// A worktree change names the project it was reviewed in; refuse it
    /// once the selection has moved to another project.
    pub(super) fn check_worktree_project(&self, call: &Call) -> Result<()> {
        if call.path.starts_with("/api/worktrees")
            && call.method == "POST"
            && !call.text("workspace").is_empty()
        {
            ensure!(
                Workspace::open(Path::new(call.text("workspace")))?.path == self.workspace()?,
                "Project changed; refresh worktrees before continuing"
            );
        }
        Ok(())
    }
    /// Source-project reads (inspect, review) require trust.
    fn trusted_workspace(&self, action: &str) -> Result<PathBuf> {
        let workspace = self.workspace()?;
        ensure!(
            Config::load(self.engine.paths(), Some(&workspace))?.is_trusted(&workspace),
            "Trust the source project before {action}"
        );
        Ok(workspace)
    }
    pub(super) async fn worktree_routes(&self, call: &Arc<Call>) -> Result<Value> {
        let body: WorktreeBody = call.body()?;
        let (id, hash) = (body.id.as_str(), body.hash.as_str());
        let store = self.engine.store();
        let paths = self.engine.paths();
        match (call.method.as_str(), call.path.as_str()) {
            ("GET", "/api/worktrees") => Ok(
                json!({"workspace":self.workspace()?,"worktrees":worktrees::list(paths,&self.workspace()?)?}),
            ),
            ("POST", "/api/worktrees/inspect") => {
                let workspace = self.trusted_workspace("inspecting its worktrees")?;
                Ok(json!(
                    worktrees::inspect(paths, &workspace, id, CancellationToken::new()).await?
                ))
            }
            ("POST", "/api/worktrees/review-changes") => {
                let workspace = self.trusted_workspace("reviewing its changes")?;
                Ok(json!(
                    worktrees::changes::review(&workspace, CancellationToken::new()).await?
                ))
            }
            ("POST", "/api/worktrees/copy-changes") => {
                let ws = self.mutable_workspace()?;
                let _background = self.engine.background().reserve_idle_workspace(&ws.path)?;
                let record = worktrees::changes::copy(
                    paths,
                    &ws.path,
                    hash,
                    ws.reservation.cancellation(),
                    |path| self.engine.reserve_workspace(path),
                )
                .await?;
                store.add_event("worktree.changes_copied", &json!(record), None, None)?;
                Ok(json!(record))
            }
            ("POST", "/api/worktrees/review-return") => {
                let workspace = self.trusted_workspace("reviewing returned changes")?;
                Ok(json!(
                    worktrees::review_return(paths, &workspace, id, CancellationToken::new())
                        .await?
                ))
            }
            ("POST", "/api/worktrees/return") => {
                let ws = self.mutable_workspace()?;
                let reviewed =
                    worktrees::review_return(paths, &ws.path, id, ws.reservation.cancellation())
                        .await?;
                let _target = self.engine.reserve_workspace(&reviewed.record.path)?;
                let _source_background =
                    self.engine.background().reserve_idle_workspace(&ws.path)?;
                let _target_background = self
                    .engine
                    .background()
                    .reserve_idle_workspace(&reviewed.record.path)?;
                let record = worktrees::return_changes(
                    paths,
                    &ws.path,
                    id,
                    hash,
                    ws.reservation.cancellation(),
                )
                .await?;
                store.add_event("worktree.returned", &json!(record), None, None)?;
                Ok(json!(record))
            }
            ("POST", "/api/worktrees/review-repair") => {
                let workspace = self.trusted_workspace("reviewing repair")?;
                Ok(json!(
                    worktrees::repair::review(paths, &workspace, id, CancellationToken::new())
                        .await?
                ))
            }
            ("POST", "/api/worktrees/repair") => {
                let ws = self.mutable_workspace()?;
                let reviewed =
                    worktrees::repair::review(paths, &ws.path, id, ws.reservation.cancellation())
                        .await?;
                let _target = self.engine.reserve_workspace(&reviewed.record.path)?;
                let _source_background =
                    self.engine.background().reserve_idle_workspace(&ws.path)?;
                let _target_background = self
                    .engine
                    .background()
                    .reserve_idle_workspace(&reviewed.record.path)?;
                let record = worktrees::repair::apply(
                    paths,
                    &ws.path,
                    id,
                    hash,
                    ws.reservation.cancellation(),
                )
                .await?;
                store.add_event("worktree.repaired", &json!(record), None, None)?;
                Ok(json!(record))
            }
            ("POST", "/api/worktrees/recovery") => {
                let workspace = self.trusted_workspace("inspecting recovery")?;
                Ok(json!(
                    worktrees::recovery(paths, &workspace, id, CancellationToken::new()).await?
                ))
            }
            ("POST", "/api/worktrees/restore") => {
                let ws = self.mutable_workspace()?;
                let record =
                    worktrees::restore(paths, &ws.path, id, hash, ws.reservation.cancellation())
                        .await?;
                store.add_event(
                    "worktree.restored",
                    &json!({"original_id":id,"checkout":record}),
                    None,
                    None,
                )?;
                Ok(json!(record))
            }
            ("POST", "/api/worktrees") => {
                let ws = self.mutable_workspace()?;
                let reference = body.reference.0.as_deref().unwrap_or("HEAD");
                let record =
                    worktrees::create(paths, &ws.path, reference, ws.reservation.cancellation())
                        .await?;
                store.add_event("worktree.created", &json!(record), None, None)?;
                Ok(json!(record))
            }
            ("POST", "/api/worktrees/remove") => {
                let ws = self.mutable_workspace()?;
                let inspected =
                    worktrees::inspect(paths, &ws.path, id, ws.reservation.cancellation()).await?;
                let _target = self.engine.reserve_workspace(&inspected.record.path)?;
                let _background = self
                    .engine
                    .background()
                    .reserve_idle_workspace(&inspected.record.path)?;
                let record =
                    worktrees::remove(paths, &ws.path, id, hash, ws.reservation.cancellation())
                        .await?;
                store.add_event("worktree.removed", &json!(record), None, None)?;
                Ok(json!(record))
            }
            ("GET", "/api/parallel") => Ok(json!({"max_workers":crate::parallel::MAX_WORKERS,
                "plan":crate::parallel::active_plan(&self.workspace()?, &paths.data.join("parallel-worktrees"))?,
                "note":"Prepared checkouts require explicit tasks. No automatic dispatch or integration."})),
            (
                "POST",
                "/api/parallel/prepare"
                | "/api/parallel/worker-status"
                | "/api/parallel/verify"
                | "/api/parallel/cleanup",
            ) => self.parallel_action(call).await,
            ("POST", "/api/sandbox/discard-scratch") => {
                crate::sandbox::discard_scratch(&PathBuf::from(call.text("path")))
            }
            _ => Err(call.unavailable()),
        }
    }
    async fn parallel_action(&self, call: &Call) -> Result<Value> {
        let workspace = self.mutable_workspace()?;
        let root = self.engine.paths().data.join("parallel-worktrees");
        let mut worker_reservations = Vec::new();
        if let Some(plan) = crate::parallel::active_plan(&workspace.path, &root)? {
            for worker in plan.workers.iter().filter(|w| w.status != "removed") {
                worker_reservations.push(self.engine.reserve_workspace(&worker.worktree_path)?);
            }
        }
        let action = call.path.clone();
        let ParallelBody {
            goal,
            worker_id,
            status,
        } = call.body()?;
        tokio::task::spawn_blocking(move || {
            let _workers = worker_reservations;
            let result = match action.as_str() {
                "/api/parallel/prepare" => {
                    crate::parallel::prepare(&workspace.path, goal.as_str(), &root)
                }
                "/api/parallel/worker-status" => crate::parallel::mark_worker_status(
                    &workspace.path,
                    &root,
                    worker_id.as_str(),
                    status.as_str(),
                ),
                "/api/parallel/verify" => crate::parallel::verify(&workspace.path, &root),
                _ => crate::parallel::cleanup(&workspace.path, &root),
            };
            drop(workspace);
            result
        })
        .await?
    }
}
