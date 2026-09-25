//! `/api/preview…`: the in-app preview of the project's dev servers. Detection
//! and the loopback proxies live in `crate::preview`.
use super::*;
use crate::preview::{ports, Target};
use std::collections::{BTreeMap, HashSet};

/// POST /api/preview/open
#[derive(Default, Deserialize)]
#[serde(default)]
struct OpenBody {
    url: Text,
    app_origin: Text,
}

impl Service {
    pub(super) async fn preview_routes(&self, call: &Arc<Call>) -> Result<Value> {
        match (call.method.as_str(), call.path.as_str()) {
            ("GET", "/api/preview/servers") => self.blocking(call, Self::preview_servers).await,
            ("POST", "/api/preview/open") => {
                let body: OpenBody = call.body()?;
                let (target, path) = Target::parse(body.url.as_str())?;
                let mut forbidden = tokio::task::spawn_blocking(|| {
                    ports::own_ports(Path::new("/proc"), std::process::id())
                })
                .await?;
                forbidden.extend(self.previews.ports().await);
                let opened = self
                    .previews
                    .open(target, body.app_origin.as_str(), &forbidden)
                    .await?;
                let mut value = json!(opened);
                value["url"] = json!(format!("{}{path}", opened.proxy_origin));
                value["target_url"] = json!(format!("{}{path}", opened.target_origin));
                Ok(value)
            }
            _ => Err(call.unavailable()),
        }
    }

    /// GET /api/preview/servers: URLs printed by the project's running
    /// background processes and loopback ports its processes listen on,
    /// merged by port. ShadowCode's own ports are never listed.
    fn preview_servers(&self, _call: &Call) -> Result<Value> {
        let workspace = self.workspace()?;
        let proc_root = Path::new("/proc");
        let own = std::process::id();
        let own_ports: HashSet<u16> = ports::own_ports(proc_root, own);
        let listening: HashSet<u16> = ports::listening(proc_root).iter().map(|l| l.port).collect();
        let mut servers: BTreeMap<u16, Value> = BTreeMap::new();
        for found in ports::project_servers(proc_root, &workspace, own) {
            if own_ports.contains(&found.port) {
                continue;
            }
            let host = match found.ip {
                std::net::IpAddr::V4(ip) if ip.is_loopback() && ip.octets() != [127, 0, 0, 1] => {
                    ip.to_string()
                }
                _ => "localhost".to_owned(),
            };
            servers.insert(
                found.port,
                json!({
                    "port": found.port,
                    "url": format!("http://{host}:{}/", found.port),
                    "source": "process",
                    "listening": true,
                    "pid": found.pid,
                    "process": found.process,
                    "command": found.command,
                    "background_id": null,
                    "background_name": null,
                }),
            );
        }
        for task in self.engine.background().list(&workspace)? {
            if !matches!(task.status.as_str(), "STARTING" | "RUNNING") {
                continue;
            }
            let mut cut = task.output.len().saturating_sub(64 * 1024);
            while !task.output.is_char_boundary(cut) {
                cut += 1;
            }
            for printed in ports::printed_urls(&task.output[cut..]) {
                if own_ports.contains(&printed.port) {
                    continue;
                }
                let entry = servers.entry(printed.port).or_insert_with(|| {
                    json!({
                        "port": printed.port,
                        "pid": null,
                        "process": null,
                        "command": null,
                    })
                });
                entry["url"] = json!(printed.url);
                entry["source"] = json!("background");
                entry["listening"] = json!(listening.contains(&printed.port));
                entry["background_id"] = json!(task.id);
                entry["background_name"] = json!(task.name);
            }
        }
        Ok(json!({
            "workspace": workspace,
            "servers": servers.into_values().collect::<Vec<_>>(),
        }))
    }
}
