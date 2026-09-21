//! Independent navigation state for a desktop attached to an existing engine.
//! The lease keeps only the view alive; jobs belong to the persistent engine.
use super::*;
use std::{collections::HashMap, sync::RwLock};
use tokio::sync::Mutex;

#[derive(Clone, Default)]
pub(super) struct Registry(Arc<RwLock<HashMap<String, Service>>>);
impl Registry {
    pub(super) fn get(&self, id: &str) -> Result<Service> {
        self.0
            .read()
            .map_err(|_| anyhow::anyhow!("View registry poisoned"))?
            .get(id)
            .cloned()
            .context("Attached view is closed; reconnect to the engine")
    }
    fn insert(&self, service: Service) -> Result<Lease> {
        let mut views = self
            .0
            .write()
            .map_err(|_| anyhow::anyhow!("View registry poisoned"))?;
        // Leave connection slots for task owners and concurrent ordinary calls.
        ensure!(
            views.len() < 4,
            "At most four desktop views may attach to this engine"
        );
        let id = crate::id();
        views.insert(id.clone(), service);
        Ok(Lease {
            id,
            registry: self.clone(),
        })
    }
}
struct Lease {
    id: String,
    registry: Registry,
}
impl Drop for Lease {
    fn drop(&mut self) {
        if let Ok(mut views) = self.registry.0.write() {
            views.remove(&self.id);
        }
    }
}

/// A private navigation view. Closing it does not shut down the engine or
/// cancel its durable jobs. Requests use separate connections so a slow tool
/// cannot prevent status reads or cancellation from the same window.
pub struct ViewClient {
    client: Client,
    lease: Mutex<Option<UnixStream>>,
    closed: AtomicBool,
}
impl Client {
    pub async fn open_view(&self) -> Result<ViewClient> {
        ensure!(
            self.available().await?,
            "No running engine is available to attach"
        );
        let mut stream = self.connect().await?;
        send(
            &mut stream,
            &self.envelope(Request {
                method: "POST".into(),
                path: "/api/views".into(),
                body: Value::Null,
            }),
            REQUEST_LIMIT,
        )
        .await?;
        let ready = tokio::time::timeout(Duration::from_secs(10), self.response(&mut stream))
            .await
            .context("Desktop attachment timed out")??;
        let id = ready["view"]
            .as_str()
            .context("Engine returned no attached view")?;
        ensure!(
            id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "Invalid attached view identity"
        );
        let mut client = self.clone();
        client.view = Some(id.to_owned());
        Ok(ViewClient {
            client,
            lease: Mutex::new(Some(stream)),
            closed: AtomicBool::new(false),
        })
    }
}
impl ViewClient {
    pub async fn dispatch(&self, request: Request) -> Result<Value> {
        ensure!(
            !self.closed.load(Ordering::Acquire),
            "Attached view is closed"
        );
        self.client.dispatch(request).await
    }
    pub async fn close(&self) -> Result<()> {
        self.closed.store(true, Ordering::Release);
        let Some(mut stream) = self.lease.lock().await.take() else {
            return Ok(());
        };
        stream.write_all(&[1]).await?;
        let result =
            tokio::time::timeout(Duration::from_secs(10), self.client.response(&mut stream))
                .await
                .context("Desktop detach timed out")??;
        ensure!(
            result["closed"] == true,
            "Engine did not acknowledge desktop detach"
        );
        Ok(())
    }
}

pub(super) async fn serve(
    stream: &mut UnixStream,
    service: Service,
    profile: &str,
    registry: Registry,
) -> Result<Value> {
    let lease = registry.insert(service)?;
    send(
        stream,
        &json!({"protocol": PROTOCOL, "profile": profile, "result": {"view": lease.id}}),
        RESPONSE_LIMIT,
    )
    .await?;
    // EOF, an explicit close byte, or server task cancellation drops the lease.
    // No periodic client heartbeat is required while the window is inactive.
    let mut byte = [0];
    let count = stream.read(&mut byte).await?;
    ensure!(
        count == 0 || byte[0] == 1,
        "Invalid desktop view control message"
    );
    drop(lease);
    Ok(json!({"closed": true}))
}
