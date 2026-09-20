use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Approval {
    pub id: String,
    pub session_id: String,
    pub task_id: String,
    pub tool: String,
    pub arguments: Value,
    pub command: String,
    pub reason: String,
    pub pending: bool,
    pub created_at: f64,
    pub expires_at: f64,
}
struct Pending {
    record: Approval,
    answer: oneshot::Sender<bool>,
}
type PendingMap = Arc<Mutex<HashMap<String, Pending>>>;

#[derive(Clone, Default)]
pub struct ApprovalHub {
    pending: PendingMap,
}
struct Ticket {
    id: String,
    pending: PendingMap,
}
impl Drop for Ticket {
    fn drop(&mut self) {
        if let Ok(mut map) = self.pending.lock() {
            map.remove(&self.id);
        }
    }
}
impl ApprovalHub {
    pub fn list(&self, session: Option<&str>) -> Vec<Approval> {
        let mut records: Vec<_> = self
            .pending
            .lock()
            .map(|map| {
                map.values()
                    .filter(|p| session.is_none_or(|s| s == p.record.session_id))
                    .map(|p| p.record.clone())
                    .collect()
            })
            .unwrap_or_default();
        records.sort_by(|a, b| a.created_at.total_cmp(&b.created_at));
        records
    }
    pub fn decide(&self, id: &str, session: &str, approve: bool) -> Result<Approval> {
        let mut map = self
            .pending
            .lock()
            .map_err(|_| anyhow::anyhow!("Approval lock poisoned"))?;
        let pending = map
            .get(id)
            .context("Approval expired or was already answered")?;
        ensure!(
            pending.record.session_id == session,
            "Approval belongs to a different session"
        );
        ensure!(pending.record.expires_at > crate::now(), "Approval expired");
        let mut pending = map.remove(id).context("Approval no longer exists")?;
        pending.record.pending = false;
        pending
            .answer
            .send(approve)
            .map_err(|_| anyhow::anyhow!("Task is no longer waiting for this approval"))?;
        Ok(pending.record)
    }
    pub async fn request<F>(
        &self,
        mut record: Approval,
        timeout: Duration,
        cancel: CancellationToken,
        on_pending: F,
    ) -> Result<bool>
    where
        F: FnOnce(&Approval),
    {
        ensure!(!cancel.is_cancelled(), "Task cancelled before approval");
        record.id = crate::id();
        record.pending = true;
        record.created_at = crate::now();
        record.expires_at = record.created_at + timeout.as_secs_f64();
        let (answer, receiver) = oneshot::channel();
        let ticket = Ticket {
            id: record.id.clone(),
            pending: self.pending.clone(),
        };
        self.pending
            .lock()
            .map_err(|_| anyhow::anyhow!("Approval lock poisoned"))?
            .insert(
                record.id.clone(),
                Pending {
                    record: record.clone(),
                    answer,
                },
            );
        on_pending(&record);
        let result = tokio::select! {
            _=cancel.cancelled()=>false,
            _=tokio::time::sleep(timeout)=>false,
            answer=receiver=>answer.unwrap_or(false),
        };
        drop(ticket);
        Ok(result)
    }
    pub fn deny_task(&self, task_id: &str) {
        if let Ok(mut map) = self.pending.lock() {
            map.retain(|_, pending| pending.record.task_id != task_id);
        }
    }
}
