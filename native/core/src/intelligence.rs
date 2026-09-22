//! Local code intelligence: on-demand SQLite AST index (tree-sitter) plus
//! optional rust-analyzer diagnostics. No vector DB, no startup world index.
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::{path::Path, time::Duration};

const MAX_HITS: usize = 80;

pub fn workspace_symbols(root: &Path, query: &str, max_hits: usize) -> Result<Value> {
    let max_hits = max_hits.clamp(1, MAX_HITS);
    let defs = crate::symbol_index::query_definitions(root, query, max_hits)?;
    Ok(json!({
        "ok": true,
        "query": query,
        "count": defs["definitions"].as_array().map(|a| a.len()).unwrap_or(0),
        "bounded": true,
        "symbols": defs["definitions"],
        "source": "sqlite-ast-index",
        "note": "Tree-sitter local parse stored in ordinary SQLite tables; not a whole-world index or LSP."
    }))
}

pub fn goto_definition(root: &Path, symbol: &str, max_hits: usize) -> Result<Value> {
    let max_hits = max_hits.clamp(1, MAX_HITS);
    crate::symbol_index::query_definitions(root, symbol, max_hits)
}

pub fn find_references(root: &Path, symbol: &str, max_hits: usize) -> Result<Value> {
    let max_hits = max_hits.clamp(1, MAX_HITS);
    crate::symbol_index::query_references(root, symbol, max_hits)
}

pub fn get_type_signature(root: &Path, symbol: &str) -> Result<Value> {
    crate::symbol_index::get_type_signature(root, symbol)
}

pub fn callers_for_patch(root: &Path, symbol: &str) -> Result<Value> {
    crate::symbol_index::callers_for(root, symbol, 16)
}

pub fn rust_analyzer_available() -> bool {
    crate::sandbox::which("rust-analyzer").is_some()
}

pub async fn get_diagnostics(root: &Path, path: &str) -> Result<Value> {
    if !rust_analyzer_available() {
        bail!("rust-analyzer is not installed on PATH; diagnostics are unavailable");
    }
    let workspace = crate::workspace::Workspace::open(root)?;
    let rel = workspace.relative(path)?;
    let full = root.join(&rel).canonicalize()?;
    anyhow::ensure!(full.starts_with(&workspace.path), "path escapes workspace");
    anyhow::ensure!(
        !crate::redaction::is_secret_path(&rel.to_string_lossy()),
        "Secret paths cannot be inspected"
    );
    workspace.read(&rel.to_string_lossy())?;
    let mut spec = crate::process::ProcessSpec::command(
        "rust-analyzer",
        &["diagnostics", &full.to_string_lossy()],
        root.to_path_buf(),
    );
    spec.timeout = Duration::from_secs(45);
    spec.output_limit = 64_000;
    let result = crate::process::run(spec, tokio_util::sync::CancellationToken::new(), None).await;
    match result {
        Ok(out) => Ok(json!({
            "ok": out.ok,
            "path": path,
            "exit_code": out.exit_code,
            "stdout": crate::tools::truncate(&out.stdout, 32_000),
            "stderr": crate::tools::truncate(&out.stderr, 8_000),
            "note": "One-shot rust-analyzer diagnostics; no persistent LSP was started."
        })),
        Err(error) => Ok(json!({
            "ok": false,
            "path": path,
            "error": format!("{error:#}"),
            "note": "rust-analyzer was present but failed to run (missing libs or user namespaces). Falling back without diagnostics."
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn finds_rust_symbols_via_index() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("lib.rs"),
            "pub fn greet(name: &str) -> String { format!(\"hi {name}\") }\npub struct Point { pub x: i32 }\n",
        )
        .unwrap();
        let symbols = workspace_symbols(root.path(), "greet", 20).unwrap();
        assert!(symbols["symbols"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["name"] == "greet"));
        let defs = goto_definition(root.path(), "Point", 20).unwrap();
        assert!(defs["definitions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["name"] == "Point"));
        let refs = find_references(root.path(), "greet", 20).unwrap();
        assert!(refs["count"].as_u64().unwrap() >= 1);
        let sig = get_type_signature(root.path(), "greet").unwrap();
        assert!(sig["ok"].as_bool().unwrap());
    }
}
