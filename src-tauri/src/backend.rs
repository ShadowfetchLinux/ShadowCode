use anyhow::{bail, Result};
use serde_json::Value;
use shadowcode_core::{
    control,
    paths::AppPaths,
    service::{Request, Service},
};
use std::{path::PathBuf, time::Duration};

pub enum Backend {
    Owned {
        service: Service,
        server: control::Server,
    },
    Attached(control::ViewClient),
}
impl Backend {
    pub async fn open(paths: AppPaths, workspace: Option<PathBuf>) -> Result<Self> {
        let workspace = workspace
            .or_else(|| paths.remembered_workspace())
            .unwrap_or(std::env::current_dir()?);
        let client = control::Endpoint::for_paths(&paths)?.client(workspace.clone(), None);
        for attempt in 0..30 {
            if client.available().await? {
                return Ok(Self::Attached(client.open_view().await?));
            }
            match Service::open(paths.clone(), Some(workspace.clone())) {
                Ok(service) => {
                    let server = control::Server::start(service.clone())?;
                    return Ok(Self::Owned { service, server });
                }
                Err(error) if attempt < 29 && error.to_string().contains("already running") => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Err(error) => return Err(error),
            }
        }
        bail!("The active engine has not opened its local connection; wait for startup or close the other version")
    }
    pub async fn dispatch(&self, request: Request) -> Result<Value> {
        let describe = request.method == "GET"
            && matches!(request.path.as_str(), "/api/health" | "/api/version");
        let mut result = match self {
            Self::Owned { service, .. } => service.dispatch(request).await?,
            Self::Attached(view) => view.dispatch(request).await?,
        };
        if describe {
            result["desktop_attached"] = Value::Bool(matches!(self, Self::Attached(_)));
            result["desktop_pid"] = serde_json::json!(std::process::id());
        }
        Ok(result)
    }
    pub async fn close(&self) -> Result<()> {
        match self {
            Self::Owned { service, server } => {
                server.close();
                server.wait_closed().await;
                service.engine.shutdown().await
            }
            Self::Attached(view) => {
                // The shared owner may already have exited. Detaching drops
                // the lease even when its acknowledgement cannot arrive; that
                // must not trap the user in an uncloseable window.
                if let Err(error) = view.close().await {
                    eprintln!("Detached desktop without engine acknowledgement: {error:#}");
                }
                Ok(())
            }
        }
    }
}
