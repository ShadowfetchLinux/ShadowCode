use crate::{
    control::{Client, Endpoint, Server},
    paths::AppPaths,
    service::{Request, Service},
};
use anyhow::{bail, Result};
use serde_json::Value;
use std::{path::PathBuf, time::Duration};
pub struct Backend {
    local: Option<(Service, Server)>,
    client: Client,
    pub persistent: bool,
    pub parent: Option<u32>,
}
impl Backend {
    pub async fn open(
        paths: AppPaths,
        workspace: PathBuf,
        serving: bool,
        parent: Option<u32>,
    ) -> Result<Self> {
        Self::open_mode(
            paths,
            workspace,
            if serving { "server" } else { "command" },
            serving,
            parent,
        )
        .await
    }
    pub(crate) async fn open_tui(
        paths: AppPaths,
        workspace: PathBuf,
        parent: Option<u32>,
    ) -> Result<Self> {
        Self::open_mode(paths, workspace, "tui", false, parent).await
    }
    async fn open_mode(
        paths: AppPaths,
        workspace: PathBuf,
        mode: &str,
        require_owner: bool,
        parent: Option<u32>,
    ) -> Result<Self> {
        let endpoint = Endpoint::for_paths(&paths)?;
        let client = endpoint.client(workspace.clone(), None);
        for attempt in 0..30 {
            if client.available().await? {
                if require_owner {
                    bail!("An engine already owns this profile; use its desktop or existing headless server");
                }
                let runtime = client
                    .dispatch(Request {
                        method: "GET".into(),
                        path: "/api/runtime".into(),
                        body: Value::Null,
                    })
                    .await?;
                return Ok(Self {
                    local: None,
                    client,
                    persistent: runtime["persistent"] == true,
                    parent,
                });
            }
            match Service::open(paths.clone(), Some(workspace.clone())) {
                Ok(service) => {
                    let server = Server::start_with_mode(service.clone(), mode)?;
                    return Ok(Self {
                        local: Some((service, server)),
                        client,
                        persistent: mode != "command",
                        parent,
                    });
                }
                Err(error) if attempt < 29 && error.to_string().contains("already running") => {
                    tokio::time::sleep(Duration::from_millis(100)).await
                }
                Err(error) => return Err(error.context("Could not open the native engine")),
            }
        }
        bail!("The active engine has not opened its local connection; wait for startup or close the other version")
    }
    pub async fn dispatch(&self, request: Request) -> Result<Value> {
        match &self.local {
            Some((service, _)) => service.dispatch(request).await,
            None => self.client.dispatch(request).await,
        }
    }
    pub async fn close(&self) -> Result<()> {
        if let Some((service, server)) = &self.local {
            server.close();
            server.wait_closed().await;
            service.engine.shutdown().await?;
        }
        Ok(())
    }
    pub async fn call(&self, method: &str, path: impl Into<String>, body: Value) -> Result<Value> {
        self.dispatch(Request {
            method: method.into(),
            path: path.into(),
            body,
        })
        .await
    }
    pub fn service(&self) -> Option<&Service> {
        self.local.as_ref().map(|(service, _)| service)
    }
    pub(crate) async fn own_jobs(&self) -> Result<crate::control::OwnedJobs> {
        self.client.own_jobs().await
    }
}
