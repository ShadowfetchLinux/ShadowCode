//! Read-only ACP handshake probe for Cursor (`cursor-agent acp`) and Grok
//! (`grok agent stdio`): auth methods, image support, modes, and models.
//! No prompt is sent. Usage: cargo run --example probe_acp -- cursor|grok
use std::{path::PathBuf, time::Duration};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let which = std::env::args().nth(1).unwrap_or_else(|| "cursor".into());
    let (bin, args): (&str, &[&str]) = match which.as_str() {
        "grok" => ("grok", &["agent", "stdio"]),
        _ => ("cursor-agent", &["acp"]),
    };
    let binary = shadowcode_core::cli_agent::resolve_binary(bin)
        .ok_or_else(|| anyhow::anyhow!("{bin} is not on PATH"))?;
    let workspace = std::env::temp_dir();
    let probe = shadowcode_core::cli_agent::acp_probe::probe(&binary, args, &workspace, None, Duration::from_secs(40)).await?;
    println!(
        "protocol={:?} agent_version={:?} auth_methods={:?} authenticated={:?} session={} error={:?} images={} load_session={} modes={:?} current={:?}",
        probe.protocol_version, probe.agent_version, probe.auth_methods, probe.authenticated, probe.session_started, probe.session_error, probe.accepts_images, probe.load_session, probe.modes, probe.current_model
    );
    for m in probe.models.iter().take(12) {
        println!("model {} ({}) current={}", m.id, m.label, m.current);
    }
    println!("{} models total", probe.models.len());
    let _ = PathBuf::new();
    Ok(())
}
