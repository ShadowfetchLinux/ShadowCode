//! Native application engine shared by the desktop, CLI, and MCP transports.
pub mod allowance;
pub mod approvals;
pub mod autonomy;
pub mod background;
pub mod checkpoint;
#[cfg(unix)]
pub mod cli;
pub mod cli_agent;
pub mod compaction;
pub mod compare;
pub mod config;
pub mod context;
#[cfg(unix)]
pub mod control;
pub mod engine;
pub mod events;
pub mod gguf;
pub mod guardian;
pub mod hooks;
pub mod intelligence;
#[cfg(unix)]
pub mod lifecycle;
pub mod local_engine;
pub mod local_runtime;
#[cfg(unix)]
pub mod mcp;
pub mod memory;
pub mod model_registry;
pub mod models;
pub mod ollama_store;
pub mod openrouter;
pub mod parallel;
pub mod patch;
pub mod paths;
pub mod permissions;
#[cfg(unix)]
pub mod plugins;
pub mod process;
pub mod project;
pub mod prompt_cache;
pub mod redaction;
pub mod retry;
pub mod routing;
pub mod runtime;
pub mod sandbox;
pub mod service;
#[cfg(unix)]
pub mod sqlite;
pub mod steering;
pub mod store;
pub mod symbol_index;
pub mod system_info;
pub mod tools;
pub mod usage;
pub mod vision;
pub mod web;
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
