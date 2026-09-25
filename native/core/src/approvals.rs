use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{broadcast, oneshot};
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
    /// Where `approval.expiring` warnings go (the engine's broadcast).
    notices: Option<broadcast::Sender<Value>>,
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
/// When a pending approval warns that it will soon be denied: after 80% of
/// its time (8 of 10 minutes). Timeouts under a minute get no warning.
pub fn warning_delay(timeout: Duration) -> Option<Duration> {
    (timeout >= Duration::from_secs(60)).then(|| timeout.mul_f64(0.8))
}
impl ApprovalHub {
    /// A hub that also broadcasts a transient `approval.expiring` event
    /// (never stored) shortly before a pending approval times out.
    pub fn with_notices(notices: broadcast::Sender<Value>) -> Self {
        Self {
            pending: PendingMap::default(),
            notices: Some(notices),
        }
    }
    fn warn(&self, record: &Approval) {
        let Some(notices) = &self.notices else {
            return;
        };
        let mut payload = json!({
            "approval_id": record.id,
            "session_id": record.session_id,
            "tool": record.tool,
            "command": record.command,
            "expires_at": record.expires_at,
            "seconds_left": (record.expires_at - crate::now()).max(0.0).round(),
        });
        crate::redaction::redact_value(&mut payload);
        let _ = notices.send(json!({
            "type": "approval.expiring",
            "session_id": record.session_id,
            "task_id": record.task_id,
            "payload": payload,
        }));
    }
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
        let expiry = tokio::time::sleep(timeout);
        tokio::pin!(expiry);
        let warning = tokio::time::sleep(warning_delay(timeout).unwrap_or(timeout));
        tokio::pin!(warning);
        let mut receiver = receiver;
        let mut warned = warning_delay(timeout).is_none();
        let result = loop {
            tokio::select! {
                _=cancel.cancelled()=>break false,
                _=&mut expiry=>break false,
                answer=&mut receiver=>break answer.unwrap_or(false),
                _=&mut warning, if !warned=>{
                    warned = true;
                    self.warn(&record);
                }
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendor_approvals_warn_two_minutes_before_they_expire() {
        assert_eq!(
            warning_delay(Duration::from_secs(600)),
            Some(Duration::from_secs(480))
        );
        assert_eq!(warning_delay(Duration::from_secs(30)), None);
        let (sender, mut receiver) = broadcast::channel(4);
        let hub = ApprovalHub::with_notices(sender);
        let now = crate::now();
        hub.warn(&Approval {
            id: "a1".into(),
            session_id: "s1".into(),
            task_id: "t1".into(),
            tool: "vendor".into(),
            arguments: Value::Null,
            command: "curl -H 'Authorization: Bearer sk-live-0123456789abcdef0123' x".into(),
            reason: String::new(),
            pending: true,
            created_at: now - 480.0,
            expires_at: now + 120.0,
        });
        let event = receiver.try_recv().unwrap();
        assert_eq!(event["type"], "approval.expiring");
        assert_eq!(event["session_id"], "s1");
        let left = event["payload"]["seconds_left"].as_f64().unwrap();
        assert!((118.0..=120.0).contains(&left), "{left}");
        assert!(!event["payload"]["command"]
            .as_str()
            .unwrap()
            .contains("sk-live-0123456789abcdef0123"));
        // A hub without a broadcast (tests, tools) stays quiet.
        ApprovalHub::default().warn(&Approval {
            id: String::new(),
            session_id: String::new(),
            task_id: String::new(),
            tool: String::new(),
            arguments: Value::Null,
            command: String::new(),
            reason: String::new(),
            pending: true,
            created_at: 0.0,
            expires_at: 0.0,
        });
    }
}
