//! Native application engine shared by the desktop, CLI, and MCP transports.
pub mod config;
pub mod models;
pub mod paths;
pub mod process;
pub mod store;
pub mod workspace;

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
