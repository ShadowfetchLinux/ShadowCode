//! Connection-scoped task ownership. The engine records ownership before a job
//! can run, including jobs whose submission reply never reaches the client.
use super::*;

#[derive(Clone)]
pub(crate) struct JobOwner(Arc<Owned>);
struct Owned {
    engine: Engine,
    cancel: CancellationToken,
    jobs: Mutex<Vec<String>>,
}
impl JobOwner {
    pub(crate) fn new(engine: Engine) -> Self {
        Self(Arc::new(Owned {
            engine,
            cancel: CancellationToken::new(),
            jobs: Mutex::new(Vec::new()),
        }))
    }
    pub(super) fn register(&self, engine: &Engine, id: &str) -> Result<CancellationToken> {
        ensure!(
            Arc::ptr_eq(&engine.0, &self.0.engine.0),
            "Wrong task owner engine"
        );
        let mut jobs = self
            .0
            .jobs
            .lock()
            .map_err(|_| anyhow!("Task owner lock poisoned"))?;
        ensure!(!self.0.cancel.is_cancelled(), "Task owner is closing");
        ensure!(
            jobs.len() < 64,
            "A task owner may submit at most 64 jobs; reconnect to continue"
        );
        jobs.push(id.to_owned());
        Ok(self.0.cancel.child_token())
    }
    pub(super) fn forget(&self, id: &str) {
        if let Ok(mut jobs) = self.0.jobs.lock() {
            jobs.retain(|job| job != id);
        }
    }
    pub(crate) async fn close(&self) -> Result<()> {
        self.0.cancel.cancel();
        let ids = self
            .0
            .jobs
            .lock()
            .map_err(|_| anyhow!("Task owner lock poisoned"))?
            .clone();
        let results = futures_util::future::join_all(ids.iter().map(|id| async {
            // Completed conversations may have been deleted through the UI.
            // Missing history is not unfinished owned work.
            if self.0.engine.job(id)?.is_some() {
                self.0.engine.cancel(id).await?;
            }
            Ok::<(), anyhow::Error>(())
        }))
        .await;
        for result in results {
            result?;
        }
        Ok(())
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        self.cancel.cancel();
        let ids = self.jobs.get_mut().unwrap_or_else(|e| e.into_inner());
        // Signal synchronously even when the transport handler is aborted. The
        // engine's normal workers retain and reap all actual task resources.
        for id in ids.iter() {
            let _ = self.engine.request_cancel(id);
        }
    }
}
