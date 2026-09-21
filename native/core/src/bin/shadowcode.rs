//! Engine CLI used by `qualification_real`. Cargo builds this bin before those
//! integration tests, so `cargo test --workspace` does not need a prior desktop build.
fn main() {
    #[cfg(not(unix))]
    {
        eprintln!("ShadowCode CLI is only available on Unix");
        std::process::exit(1);
    }
    #[cfg(unix)]
    if let Err(error) = run() {
        eprintln!("ShadowCode: {error:#}");
        std::process::exit(1);
    }
}

#[cfg(unix)]
fn run() -> anyhow::Result<()> {
    use serde_json::json;
    use shadowcode_core::cli::{self, Options};
    let options = Options::parse_args();
    let json_output = options.json && !options.mcp_stdio();
    let events_output = options.events();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(cli::run(options));
    runtime.shutdown_timeout(std::time::Duration::from_secs(2));
    let code = match result {
        Ok(code) => code,
        Err(error) => {
            use std::io::Write;
            if json_output || events_output {
                let value = json!({"ok":false,"error":format!("{error:#}")});
                let value = if events_output {
                    json!({"type":"result","exit_code":1,"result":value})
                } else {
                    value
                };
                let _ = writeln!(std::io::stdout(), "{value}");
            } else {
                let _ = writeln!(std::io::stderr(), "ShadowCode: {error:#}");
            }
            1
        }
    };
    std::process::exit(code);
}
