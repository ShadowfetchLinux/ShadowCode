//! Private local transport for command-line clients of the profile's one engine.
//! Filesystem permissions and peer credentials restrict access to this OS user;
//! it deliberately opens no TCP listener and does not accept browser requests.
use crate::{
    paths::AppPaths,
    service::{Request, Service},
    workspace::Workspace,
};
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::{Notify, Semaphore},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;
const PROTOCOL: u32 = 1;
const REQUEST_LIMIT: usize = 8_100_000;
const RESPONSE_LIMIT: usize = 68_000_000;
const MAX_CLIENTS: usize = 16;
mod owned;
mod view;
pub use owned::OwnedJobs;
pub use view::ViewClient;

fn uid() -> u32 {
    // SAFETY: geteuid has no arguments, dereferences no pointers, and cannot fail.
    unsafe { libc::geteuid() }
}
fn private_directory(path: &Path, create: bool) -> Result<()> {
    if create {
        match fs::DirBuilder::new().mode(0o700).create(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && metadata.uid() == uid()
            && metadata.mode() & 0o077 == 0,
        "Local control directory must be owned by this user and private: {}",
        path.display()
    );
    Ok(())
}
#[derive(Clone, Debug)]
pub struct Endpoint {
    path: PathBuf,
    profile: String,
}
impl Endpoint {
    pub fn for_paths(paths: &AppPaths) -> Result<Self> {
        let mut identity = Vec::new();
        for root in [&paths.config, &paths.data, &paths.state] {
            identity.extend_from_slice(root.canonicalize()?.as_os_str().as_encoded_bytes());
            identity.push(0);
        }
        let profile = format!("{:x}", Sha256::digest(&identity));
        // Derive this from UID, not each shell's environment: cron and desktop
        // sessions with different runtime/TMPDIR variables must find one engine.
        let runtime = PathBuf::from(format!("/run/user/{}", uid()));
        let directory = if private_directory(&runtime, false).is_ok() {
            runtime.join("shadowcode")
        } else {
            PathBuf::from(format!("/tmp/shadowcode-{}", uid()))
        };
        private_directory(&directory, true)?;
        let path = directory.join(format!("{}.sock", &profile[..32]));
        ensure!(
            path.as_os_str().as_encoded_bytes().len() < 104,
            "Local control socket path is too long"
        );
        Ok(Self { path, profile })
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn client(&self, workspace: PathBuf, session_id: Option<String>) -> Client {
        Client {
            endpoint: self.clone(),
            workspace,
            session_id,
            view: None,
        }
    }
}
#[derive(Serialize, Deserialize)]
struct Envelope {
    protocol: u32,
    profile: String,
    workspace: PathBuf,
    session_id: Option<String>,
    #[serde(default)]
    view: Option<String>,
    request: Request,
}
#[derive(Clone)]
pub struct Client {
    endpoint: Endpoint,
    workspace: PathBuf,
    session_id: Option<String>,
    view: Option<String>,
}
impl Client {
    /// Each request owns its connection. Dropping a pending request closes it;
    /// subsequent requests cannot consume an earlier abandoned response.
    pub async fn dispatch(&self, request: Request) -> Result<Value> {
        let mut stream = self.connect().await?;
        send(&mut stream, &self.envelope(request), REQUEST_LIMIT).await?;
        self.response(&mut stream).await
    }
    async fn connect(&self) -> Result<UnixStream> {
        let stream = tokio::time::timeout(
            Duration::from_secs(2),
            UnixStream::connect(&self.endpoint.path),
        )
        .await
        .context("Local engine connection timed out")??;
        ensure!(
            stream.peer_cred()?.uid() == uid(),
            "Local engine belongs to another OS user"
        );
        Ok(stream)
    }
    fn envelope(&self, request: Request) -> Envelope {
        Envelope {
            protocol: PROTOCOL,
            profile: self.endpoint.profile.clone(),
            workspace: self.workspace.clone(),
            session_id: self.session_id.clone(),
            view: self.view.clone(),
            request,
        }
    }
    async fn response(&self, stream: &mut UnixStream) -> Result<Value> {
        let bytes = receive(stream, RESPONSE_LIMIT).await?;
        self.result(self.validate_response(&bytes)?)
    }
    fn validate_response(&self, bytes: &[u8]) -> Result<Value> {
        let response: Value = serde_json::from_slice(bytes)?;
        ensure!(
            response["protocol"] == PROTOCOL && response["profile"] == self.endpoint.profile,
            "Local engine protocol/profile mismatch; use the matching ShadowCode build"
        );
        ensure!(
            response.get("result").is_some() || response["error"].is_string(),
            "Local engine returned no result"
        );
        Ok(response)
    }
    fn result(&self, mut response: Value) -> Result<Value> {
        if let Some(error) = response["error"].as_str() {
            bail!("{error}");
        }
        Ok(response["result"].take())
    }
    pub async fn available(&self) -> Result<bool> {
        match self
            .dispatch(Request {
                method: "GET".into(),
                path: "/api/version".into(),
                body: Value::Null,
            })
            .await
        {
            Ok(version) => {
                ensure!(version["version"] == crate::VERSION, "The active ShadowCode engine is a different version; close it before using this build");
                Ok(true)
            }
            Err(error)
                if error.downcast_ref::<std::io::Error>().is_some_and(|error| {
                    matches!(
                        error.kind(),
                        std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                    )
                }) =>
            {
                Ok(false)
            }
            Err(error) => Err(error),
        }
    }
}
struct SocketLease {
    path: PathBuf,
    dev: u64,
    ino: u64,
}
impl Drop for SocketLease {
    fn drop(&mut self) {
        if fs::symlink_metadata(&self.path)
            .is_ok_and(|m| m.dev() == self.dev && m.ino() == self.ino)
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}
struct State {
    cancel: CancellationToken,
    finished: AtomicBool,
    done: Notify,
}
struct Completion(Arc<State>);
impl Drop for Completion {
    fn drop(&mut self) {
        self.0.finished.store(true, Ordering::Release);
        self.0.done.notify_waiters();
    }
}
pub struct Server {
    state: Arc<State>,
    endpoint: Endpoint,
}
impl Server {
    /// The Service already holds the profile lock. A live endpoint is never
    /// unlinked, even if another caller tries to serve the same engine twice.
    pub fn start(service: Service) -> Result<Self> {
        Self::start_with_mode(service, "desktop")
    }
    pub fn start_with_mode(service: Service, mode: &str) -> Result<Self> {
        ensure!(
            matches!(mode, "desktop" | "server" | "command" | "tui"),
            "Invalid engine mode"
        );
        let mode = mode.to_owned();
        let endpoint = Endpoint::for_paths(service.engine.paths())?;
        if let Ok(metadata) = fs::symlink_metadata(&endpoint.path) {
            ensure!(
                metadata.file_type().is_socket() && metadata.uid() == uid(),
                "Refusing to replace a non-socket local control path"
            );
            ensure!(
                std::os::unix::net::UnixStream::connect(&endpoint.path).is_err(),
                "Local engine control endpoint is already running"
            );
            fs::remove_file(&endpoint.path)?;
        }
        let listener = UnixListener::bind(&endpoint.path)?;
        fs::set_permissions(&endpoint.path, fs::Permissions::from_mode(0o600))?;
        let metadata = fs::symlink_metadata(&endpoint.path)?;
        let lease = SocketLease {
            path: endpoint.path.clone(),
            dev: metadata.dev(),
            ino: metadata.ino(),
        };
        let state = Arc::new(State {
            cancel: CancellationToken::new(),
            finished: AtomicBool::new(false),
            done: Notify::new(),
        });
        let running = state.clone();
        let profile = endpoint.profile.clone();
        tokio::spawn(async move {
            let _completion = Completion(running.clone());
            let _lease = lease;
            let mut clients = JoinSet::new();
            let owners = Arc::new(Semaphore::new(8));
            let views = view::Registry::default();
            loop {
                tokio::select! {
                    _=running.cancel.cancelled()=>break,
                    Some(_)=clients.join_next(),if !clients.is_empty()=>{},
                    accepted=listener.accept(),if clients.len()<MAX_CLIENTS=>match accepted {
                        Ok((stream,_))=>{
                            let service=service.clone();let profile=profile.clone();let mode=mode.clone();let owners=owners.clone();let views=views.clone();
                            clients.spawn(async move {let _=handle(stream, service, profile, mode, owners, views).await;});
                        }
                        Err(_)=>break,
                    }
                }
            }
            clients.abort_all();
            while clients.join_next().await.is_some() {}
            drop(listener);
            drop(_lease);
            drop(service);
        });
        Ok(Self { state, endpoint })
    }
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }
    pub fn close(&self) {
        self.state.cancel.cancel();
    }
    pub async fn wait_closed(&self) {
        loop {
            let notified = self.state.done.notified();
            if self.state.finished.load(Ordering::Acquire) {
                break;
            }
            notified.await;
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.close();
    }
}
async fn handle(
    mut stream: UnixStream,
    service: Service,
    profile: String,
    mode: String,
    owners: Arc<Semaphore>,
    views: view::Registry,
) -> Result<()> {
    ensure!(stream.peer_cred()?.uid() == uid(), "Wrong peer user");
    let response = async {
        let bytes =
            tokio::time::timeout(Duration::from_secs(10), receive(&mut stream, REQUEST_LIMIT))
                .await
                .context("Local request timed out")??;
        let envelope: Envelope = serde_json::from_slice(&bytes).context("Invalid local request")?;
        ensure!(
            envelope.protocol == PROTOCOL && envelope.profile == profile,
            "Local engine protocol/profile mismatch"
        );
        if envelope.request.path == "/api/views" && envelope.request.method == "POST" {
            ensure!(mode != "command", "A foreground CLI task owns this profile; wait for it to finish or use shadowcode serve");
            ensure!(envelope.view.is_none(), "Cannot nest an attached view");
            let scoped = service.fork_selection(envelope.workspace, envelope.session_id)?;
            return view::serve(&mut stream, scoped, &profile, views).await;
        }
        if envelope.request.path == "/api/runtime" && envelope.request.method == "GET" {
            return Ok(json!({"mode":mode,"persistent":mode!="command","pid":std::process::id(),"version":crate::VERSION}));
        }
        if envelope.request.path == "/api/owned-jobs" && envelope.request.method == "POST" {
            ensure!(mode != "command" || stream.peer_cred()?.pid() == Some(std::process::id() as i32), "A foreground CLI task owns this profile; use the desktop or shadowcode serve for concurrent work");
            let permit = owners.try_acquire_owned().context("At most eight task ownership connections may be active")?;
            let scoped = service.fork_selection(envelope.workspace, envelope.session_id)?;
            return owned::serve(&mut stream, scoped, &profile, permit).await;
        }
        if mode == "command" && envelope.request.method != "GET" {
            ensure!(envelope.request.path.starts_with("/api/approvals/") || (envelope.request.path.starts_with("/api/jobs/") && envelope.request.path.ends_with("/cancel")), "A foreground CLI task owns this profile. Other clients can inspect it, approve tools, or cancel it; use the desktop or shadowcode serve for concurrent work.");
        }
        let scoped = if let Some(id) = envelope.view {
            views.get(&id)?
        } else {
            let workspace = Workspace::open(&envelope.workspace)?.path;
            service.fork_selection(workspace, envelope.session_id)?
        };
        // Pipelining is deliberately unsupported. EOF or extra bytes abandon
        // this request; ordinary job execution remains durable in the engine.
        let mut extra = [0u8; 1];
        tokio::select! {
            result=scoped.dispatch(envelope.request)=>result,
            _=stream.read(&mut extra)=>bail!("Client disconnected or pipelined another request"),
        }
    }
    .await;
    let value = match response {
        Ok(result) => json!({"protocol":PROTOCOL,"profile":profile,"result":result}),
        Err(error) => json!({"protocol":PROTOCOL,"profile":profile,"error":format!("{error:#}")}),
    };
    let bytes = encode(&value, RESPONSE_LIMIT).unwrap_or_else(|error| {
        // No bytes have been sent yet, so a bounded, structured error can still
        // explain a large result instead of appearing as an unexplained EOF.
        json!({"protocol":PROTOCOL,"profile":profile,"error":format!("Could not encode local response: {error}")}).to_string().into_bytes()
    });
    send_bytes(&mut stream, &bytes).await
}
async fn receive(stream: &mut UnixStream, limit: usize) -> Result<Vec<u8>> {
    let size = stream.read_u32().await? as usize;
    ensure!(
        size > 0 && size <= limit,
        "Local message exceeds its size limit"
    );
    let mut bytes = vec![0; size];
    stream.read_exact(&mut bytes).await?;
    Ok(bytes)
}
async fn send(stream: &mut UnixStream, value: &impl Serialize, limit: usize) -> Result<()> {
    send_bytes(stream, &encode(value, limit)?).await
}
fn encode(value: &impl Serialize, limit: usize) -> Result<Vec<u8>> {
    struct Bounded {
        bytes: Vec<u8>,
        limit: usize,
    }
    impl Write for Bounded {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if self.bytes.len().saturating_add(buf.len()) > self.limit {
                return Err(std::io::Error::other(
                    "Local response exceeds its size limit",
                ));
            }
            self.bytes.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut buffer = Bounded {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut buffer, value)?;
    Ok(buffer.bytes)
}
async fn send_bytes(stream: &mut UnixStream, bytes: &[u8]) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(15), async {
        stream.write_u32(bytes.len() as u32).await?;
        stream.write_all(bytes).await
    })
    .await
    .context("Local peer stopped reading")??;
    Ok(())
}
