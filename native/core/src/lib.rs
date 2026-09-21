//! Native application engine shared by the desktop, CLI, and MCP transports.
pub mod approvals;
pub mod autonomy;
pub mod background;
pub mod checkpoint;
#[cfg(unix)]
pub mod cli;
pub mod config;
pub mod context;
#[cfg(unix)]
pub mod control;
pub mod engine;
pub mod events;
pub mod hooks;
#[cfg(unix)]
pub mod lifecycle;
#[cfg(unix)]
pub mod mcp;
pub mod memory;
pub mod model_registry;
pub mod models;
pub mod patch;
pub mod paths;
pub mod permissions;
#[cfg(unix)]
pub mod plugins;
pub mod process;
pub mod project;
pub mod routing;
pub mod service;
#[cfg(unix)]
pub mod sqlite;
pub mod store;
pub mod tools;
pub mod workflows;
pub mod workspace;
pub mod worktrees;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

pub fn id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}
