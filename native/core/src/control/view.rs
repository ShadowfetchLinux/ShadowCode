//! Independent navigation state for a desktop attached to an existing engine.
//! The lease keeps only the view alive; jobs belong to the persistent engine.
use super::*;
use std::{
    collections::HashMap,
    sync::{Mutex as StdMutex, RwLock},
};
use tokio::{
    net::unix::OwnedWriteHalf,
    sync::{broadcast, Mutex},
};
use tokio_util::task::AbortOnDropHandle;
const EVENT_LIMIT: usize = 16_384;

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
    client: StdMutex<Client>,
    writer: Mutex<Option<OwnedWriteHalf>>,
    reader: Mutex<Option<AbortOnDropHandle<Result<()>>>>,
    events: broadcast::Sender<Value>,
    closed: Arc<AtomicBool>,
    closing: Arc<AtomicBool>,
    attach: Mutex<()>,
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
        let (mut reader, writer) = stream.into_split();
        let (events, _) = broadcast::channel(64);
        let closed = Arc::new(AtomicBool::new(false));
        let event_sender = events.clone();
        let ended = closed.clone();
        let peer = client.clone();
        let task = tokio::spawn(async move {
            let result = async {
                loop {
                    let bytes = receive(&mut reader, EVENT_LIMIT).await?;
                    let message = peer.result(peer.validate_response(&bytes)?)?;
                    if message["closed"] == true {
                        return Ok(());
                    }
                    let event = message
                        .get("event")
                        .context("Invalid desktop event notification")?;
                    let _ = event_sender.send(event.clone());
                }
            }
            .await;
            ended.store(true, Ordering::Release);
            if result.is_err() {
                let _ = event_sender.send(json!({"type": "view.disconnected"}));
            }
            result
        });
        Ok(ViewClient {
            client: StdMutex::new(client),
            writer: Mutex::new(Some(writer)),
            reader: Mutex::new(Some(AbortOnDropHandle::new(task))),
            events,
            closed,
            closing: Arc::new(AtomicBool::new(false)),
            attach: Mutex::new(()),
        })
    }
}
impl ViewClient {
    pub fn disconnected(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
    pub fn mark_disconnected(&self) {
        self.closed.store(true, Ordering::Release);
    }
    fn client(&self) -> Result<Client> {
        self.client
            .lock()
            .map(|client| client.clone())
            .map_err(|_| anyhow::anyhow!("View client lock poisoned"))
    }
    pub async fn dispatch(&self, request: Request) -> Result<Value> {
        ensure!(
            !self.closed.load(Ordering::Acquire),
            "Attached view is closed"
        );
        self.client()?.dispatch(request).await
    }
    pub fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.events.subscribe()
    }
    /// After the owner process exits, wait for a new engine on the same
    /// endpoint and replace this view's lease. Does not start jobs, replay
    /// tools, or emit durable events. Existing subscribers keep this
    /// broadcast and receive `view.reattached`.
    pub async fn reattach(&self) -> Result<Value> {
        ensure!(
            !self.closing.load(Ordering::Acquire),
            "Attached view is closed"
        );
        ensure!(
            self.closed.load(Ordering::Acquire),
            "Attached view is still connected; detach before reattaching"
        );
        let mut base = self.client()?;
        base.view = None;
        // Do not hold `attach` while waiting: close() must stay able to
        // detach after the owner exits instead of blocking for 20s.
        self.wait_for_owner(&base, Duration::from_secs(20)).await?;
        let _gate = self.attach.lock().await;
        ensure!(
            !self.closing.load(Ordering::Acquire),
            "Attached view is closed"
        );
        ensure!(
            self.closed.load(Ordering::Acquire),
            "Attached view is still connected; detach before reattaching"
        );
        let _ = self.reader.lock().await.take();
        let _ = self.writer.lock().await.take();
        let fresh = base.open_view().await?;
        *self.writer.lock().await = fresh.writer.lock().await.take();
        *self.reader.lock().await = fresh.reader.lock().await.take();
        {
            let mut client = self
                .client
                .lock()
                .map_err(|_| anyhow::anyhow!("View client lock poisoned"))?;
            *client = fresh
                .client
                .lock()
                .map_err(|_| anyhow::anyhow!("View client lock poisoned"))?
                .clone();
        }
        self.closed.store(false, Ordering::Release);
        let notice = json!({
            "type": "view.reattached",
            "jobs_started": 0,
            "tools_replayed": 0,
        });
        let _ = self.events.send(notice.clone());
        Ok(json!({
            "reattached": true,
            "jobs_started": 0,
            "tools_replayed": 0,
        }))
    }
    async fn wait_for_owner(&self, client: &Client, timeout: Duration) -> Result<()> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            ensure!(
                !self.closing.load(Ordering::Acquire),
                "Attached view is closed"
            );
            match client.available().await {
                Ok(true) => return Ok(()),
                Ok(false) if tokio::time::Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Ok(false) => bail!("No running engine is available to attach"),
                Err(error)
                    if error.to_string().contains("different version")
                        || error.to_string().contains("protocol/profile mismatch") =>
                {
                    return Err(error);
                }
                Err(_error) if tokio::time::Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Err(error) => return Err(error),
            }
        }
    }
    pub async fn close(&self) -> Result<()> {
        self.closing.store(true, Ordering::Release);
        let _gate = self.attach.lock().await;
        self.closed.store(true, Ordering::Release);
        let Some(mut task) = self.reader.lock().await.take() else {
            return Ok(());
        };
        let writer = self.writer.lock().await.take();
        let result = async {
            if let Some(mut writer) = writer {
                tokio::time::timeout(Duration::from_secs(2), writer.write_all(&[1]))
                    .await
                    .context("Desktop detach write timed out")??;
                writer.shutdown().await?;
            }
            tokio::time::timeout(Duration::from_secs(10), &mut task)
                .await
                .context("Desktop detach timed out")??
        }
        .await;
        if !task.is_finished() {
            task.abort();
            let _ = task.await;
        }
        result
    }
}
impl Drop for ViewClient {
    fn drop(&mut self) {
        if let Some(reader) = self.reader.get_mut().take() {
            reader.abort();
        }
        self.writer.get_mut().take();
    }
}

// Notifications are hints, never transcript payloads. In particular, a large
// model answer must not fill each attached window's transport or event buffer.
fn notification(event: &Value) -> Value {
    let mut result = json!({
        "type": event["type"].as_str().unwrap_or("").chars().take(80).collect::<String>(),
        "session_id": event["session_id"].as_str().unwrap_or("").chars().take(128).collect::<String>(),
    });
    if event["type"] == "agent.completed" {
        result["payload"] = json!({"summary": event["payload"]["summary"].as_str().unwrap_or("Task finished").chars().take(180).collect::<String>()});
    }
    result
}

pub(super) async fn serve(
    stream: &mut UnixStream,
    service: Service,
    profile: &str,
    registry: Registry,
) -> Result<Value> {
    let mut events = service.engine.subscribe();
    let lease = registry.insert(service)?;
    send(
        stream,
        &json!({"protocol": PROTOCOL, "profile": profile, "result": {"view": lease.id}}),
        RESPONSE_LIMIT,
    )
    .await?;
    // The lease is also a bounded wakeup feed. A slow window cannot block the
    // engine: lag becomes one replay hint, and a blocked write releases the view.
    let mut byte = [0];
    loop {
        tokio::select! {
            count = stream.read(&mut byte) => {
                let count = count?;
                ensure!(count == 0 || byte[0] == 1, "Invalid desktop view control message");
                break;
            }
            event = events.recv() => {
                let event = match event {
                    Ok(event) => notification(&event),
                    Err(broadcast::error::RecvError::Lagged(_)) => json!({"type":"view.lagged"}),
                    Err(broadcast::error::RecvError::Closed) => break,
                };
                tokio::time::timeout(Duration::from_secs(5), send(stream, &json!({"protocol": PROTOCOL, "profile": profile, "result": {"event": event}}), EVENT_LIMIT))
                    .await.context("Attached desktop is not reading notifications")??;
            }
        }
    }
    drop(lease);
    Ok(json!({"closed": true}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn cancelled_detach_drops_its_reader_even_when_peer_never_acknowledges() {
        struct Dropped(Arc<AtomicBool>);
        impl Drop for Dropped {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }
        let (stream, mut peer) = UnixStream::pair().unwrap();
        let (mut reader, writer) = stream.into_split();
        let released = Arc::new(AtomicBool::new(false));
        let marker = Dropped(released.clone());
        let task = tokio::spawn(async move {
            let _marker = marker;
            let _ = receive(&mut reader, EVENT_LIMIT).await?;
            Ok(())
        });
        let (events, _) = broadcast::channel(64);
        let view = Arc::new(ViewClient {
            client: StdMutex::new(Client {
                endpoint: Endpoint {
                    path: PathBuf::new(),
                    profile: "fixture".into(),
                },
                workspace: PathBuf::new(),
                session_id: None,
                view: Some("fixture".into()),
            }),
            writer: Mutex::new(Some(writer)),
            reader: Mutex::new(Some(AbortOnDropHandle::new(task))),
            events,
            closed: Arc::new(AtomicBool::new(false)),
            closing: Arc::new(AtomicBool::new(false)),
            attach: Mutex::new(()),
        });
        let closing_view = view.clone();
        let closing = tokio::spawn(async move { closing_view.close().await });
        let mut close_byte = [0];
        tokio::time::timeout(Duration::from_secs(1), peer.read_exact(&mut close_byte))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(close_byte, [1]);
        closing.abort();
        assert!(closing.await.unwrap_err().is_cancelled());
        tokio::time::timeout(Duration::from_secs(1), async {
            while !released.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(view.closed.load(Ordering::Acquire));
        view.close().await.unwrap();
    }

    #[test]
    fn notifications_bound_unicode_summaries_and_omit_transcript_content() {
        let event = json!({"type":"agent.completed", "session_id":"session", "payload":{"summary":"😀".repeat(100_000), "private":"hidden"}});
        let compact = notification(&event);
        assert_eq!(
            compact["payload"]["summary"]
                .as_str()
                .unwrap()
                .chars()
                .count(),
            180
        );
        assert!(compact["payload"].get("private").is_none());
        assert!(serde_json::to_vec(&compact).unwrap().len() < EVENT_LIMIT);
        let text =
            notification(&json!({"type":"model.stream","payload":{"text":"x".repeat(1_000_000)}}));
        assert!(text.get("payload").is_none());
    }
}
