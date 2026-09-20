//! One private connection owns all jobs submitted through it. EOF is an engine
//! lifecycle event, so killing a gateway cannot detach its jobs accidentally.
use super::*;
use crate::engine::JobOwner;
use tokio::sync::{Mutex, OwnedSemaphorePermit};

pub struct OwnedJobs {
    client: Client,
    stream: Mutex<Option<UnixStream>>,
}
impl Client {
    pub async fn own_jobs(&self) -> Result<OwnedJobs> {
        let mut stream = self.connect().await?;
        send(
            &mut stream,
            &self.envelope(Request {
                method: "POST".into(),
                path: "/api/owned-jobs".into(),
                body: Value::Null,
            }),
            REQUEST_LIMIT,
        )
        .await?;
        let ready = tokio::time::timeout(Duration::from_secs(10), self.response(&mut stream))
            .await
            .context("Task ownership handshake timed out")??;
        ensure!(
            ready["owned_jobs"] == true,
            "Engine does not support task ownership; use the matching build"
        );
        Ok(OwnedJobs {
            client: self.clone(),
            stream: Mutex::new(Some(stream)),
        })
    }
}
impl OwnedJobs {
    /// Submissions are serialized. Taking the stream before awaiting means an
    /// aborted/failed exchange closes ownership instead of reusing a partial reply.
    pub async fn submit(&self, body: Value) -> Result<Value> {
        let mut guard = self.stream.lock().await;
        let mut stream = guard
            .take()
            .context("Task ownership connection is closed")?;
        send(&mut stream, &json!({"job":body}), REQUEST_LIMIT).await?;
        let response = tokio::time::timeout(
            Duration::from_secs(30),
            receive(&mut stream, RESPONSE_LIMIT),
        )
        .await
        .context("Owned task submission timed out")??;
        let response = self.client.validate_response(&response)?;
        *guard = Some(stream);
        self.client.result(response)
    }
    pub async fn close(&self) -> Result<()> {
        let Some(mut stream) = self.stream.lock().await.take() else {
            return Ok(());
        };
        send(&mut stream, &json!({"close":true}), REQUEST_LIMIT).await?;
        let result =
            tokio::time::timeout(Duration::from_secs(30), self.client.response(&mut stream))
                .await
                .context("Owned task cleanup timed out")??;
        ensure!(
            result["closed"] == true,
            "Task ownership close was not acknowledged"
        );
        Ok(())
    }
}

pub(super) async fn serve(
    stream: &mut UnixStream,
    service: Service,
    profile: &str,
    _permit: OwnedSemaphorePermit,
) -> Result<Value> {
    let workspace = service.workspace()?;
    let owner = JobOwner::new(service.engine.clone());
    let service = service.with_job_owner(owner.clone());
    let result = async {
        send(
            stream,
            &json!({"protocol":PROTOCOL,"profile":profile,"result":{"owned_jobs":true}}),
            RESPONSE_LIMIT,
        )
        .await?;
        loop {
            // Idle connections deliberately remain live. Once a frame begins,
            // partial headers or bodies must arrive within ten seconds.
            let mut header = [0; 4];
            match stream.read_exact(&mut header[..1]).await {
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(error) => return Err(error.into()),
            }
            let bytes = tokio::time::timeout(Duration::from_secs(10), async {
                stream.read_exact(&mut header[1..]).await?;
                let size = u32::from_be_bytes(header) as usize;
                ensure!(
                    size > 0 && size <= REQUEST_LIMIT,
                    "Owned task message exceeds its size limit"
                );
                let mut bytes = vec![0; size];
                stream.read_exact(&mut bytes).await?;
                Ok::<_, anyhow::Error>(bytes)
            })
            .await
            .context("Owned task frame timed out")??;
            let message: Value =
                serde_json::from_slice(&bytes).context("Invalid task owner message")?;
            let object = message
                .as_object()
                .context("Task owner message must be an object")?;
            ensure!(object.len() == 1, "Choose one task owner operation");
            if message["close"] == true {
                break;
            }
            let mut body = message["job"]
                .as_object()
                .context("A job object is required")?
                .clone();
            if let Some(path) = body.get("workspace") {
                ensure!(
                    Workspace::open(Path::new(path.as_str().context("Invalid workspace")?))?.path
                        == workspace,
                    "Task owner is confined to its selected workspace"
                );
            }
            body.insert("workspace".into(), json!(workspace));
            let result = service
                .dispatch(Request {
                    method: "POST".into(),
                    path: "/api/jobs".into(),
                    body: Value::Object(body),
                })
                .await;
            // Ownership is registered in Engine::start before scheduling, so
            // even a failed response or an aborted handler cannot lose a job.
            let response = match result {
                Ok(job) => json!({"protocol":PROTOCOL,"profile":profile,"result":job}),
                Err(error) => {
                    json!({"protocol":PROTOCOL,"profile":profile,"error":error.to_string()})
                }
            };
            send(stream, &response, RESPONSE_LIMIT).await?;
        }
        Ok(json!({"closed":true}))
    }
    .await;
    owner.close().await?;
    result
}
