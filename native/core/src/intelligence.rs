//! Local tree-sitter symbol lookup. No vector DB, no startup world index.
//! Results are bounded. Diagnostics run only when rust-analyzer is already installed.
use anyhow::{bail, Result};
use serde_json::{json, Value};
use streaming_iterator::StreamingIterator;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};
use tree_sitter::{Parser, Query, QueryCursor};

const MAX_FILES: usize = 200;
const MAX_HITS: usize = 80;
const MAX_FILE_BYTES: usize = 512_000;

#[derive(Clone, Copy)]
enum Lang {
    Rust,
    TypeScript,
    Tsx,
}

fn lang_for(path: &Path) -> Option<Lang> {
    match path.extension().and_then(|e| e.to_str())? {
        "rs" => Some(Lang::Rust),
        "ts" | "mts" | "cts" => Some(Lang::TypeScript),
        "tsx" => Some(Lang::Tsx),
        _ => None,
    }
}

fn language(lang: Lang) -> tree_sitter::Language {
    match lang {
        Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
        Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
    }
}

fn definition_query(lang: Lang) -> &'static str {
    match lang {
        Lang::Rust => r#"
            (function_item name: (identifier) @name)
            (struct_item name: (type_identifier) @name)
            (enum_item name: (type_identifier) @name)
            (trait_item name: (type_identifier) @name)
            (impl_item type: (type_identifier) @name)
            (mod_item name: (identifier) @name)
            (const_item name: (identifier) @name)
            (static_item name: (identifier) @name)
            (type_item name: (type_identifier) @name)
        "#,
        Lang::TypeScript | Lang::Tsx => r#"
            (function_declaration name: (identifier) @name)
            (class_declaration name: (type_identifier) @name)
            (interface_declaration name: (type_identifier) @name)
            (type_alias_declaration name: (type_identifier) @name)
            (enum_declaration name: (identifier) @name)
            (lexical_declaration (variable_declarator name: (identifier) @name))
            (method_definition name: (property_identifier) @name)
        "#,
    }
}

fn reference_query(lang: Lang) -> &'static str {
    match lang {
        Lang::Rust => "(identifier) @id (type_identifier) @id",
        Lang::TypeScript | Lang::Tsx => {
            "(identifier) @id (type_identifier) @id (property_identifier) @id"
        }
    }
}

struct ParserPool {
    rust: Mutex<Parser>,
    typescript: Mutex<Parser>,
    tsx: Mutex<Parser>,
}

impl ParserPool {
    fn new() -> Result<Self> {
        let mut rust = Parser::new();
        rust.set_language(&language(Lang::Rust))?;
        let mut typescript = Parser::new();
        typescript.set_language(&language(Lang::TypeScript))?;
        let mut tsx = Parser::new();
        tsx.set_language(&language(Lang::Tsx))?;
        Ok(Self {
            rust: Mutex::new(rust),
            typescript: Mutex::new(typescript),
            tsx: Mutex::new(tsx),
        })
    }

    fn with_parser<R>(&self, lang: Lang, f: impl FnOnce(&mut Parser) -> R) -> Result<R> {
        let lock = match lang {
            Lang::Rust => &self.rust,
            Lang::TypeScript => &self.typescript,
            Lang::Tsx => &self.tsx,
        };
        let mut parser = lock
            .lock()
            .map_err(|_| anyhow::anyhow!("Parser lock poisoned"))?;
        Ok(f(&mut parser))
    }
}

fn pool() -> Result<&'static ParserPool> {
    static POOL: std::sync::OnceLock<ParserPool> = std::sync::OnceLock::new();
    if let Some(existing) = POOL.get() {
        return Ok(existing);
    }
    let created = ParserPool::new()?;
    let _ = POOL.set(created);
    Ok(POOL.get().expect("parser pool"))
}

#[derive(Clone, Debug)]
struct SymbolHit {
    name: String,
    kind: String,
    path: String,
    line: usize,
    column: usize,
}

fn walk_sources(root: &Path, limit: usize) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(v) => v,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            if out.len() >= limit {
                return Ok(out);
            }
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.')
                || name == "target"
                || name == "node_modules"
                || name == "dist"
                || name == ".git"
            {
                continue;
            }
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            if meta.is_dir() {
                stack.push(path);
            } else if lang_for(&path).is_some() && meta.len() <= MAX_FILE_BYTES as u64 {
                out.push(path);
            }
        }
    }
    Ok(out)
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

fn collect_definitions(root: &Path, query_name: Option<&str>, max_hits: usize) -> Result<Vec<SymbolHit>> {
    let files = walk_sources(root, MAX_FILES)?;
    let mut hits = Vec::new();
    let pool = pool()?;
    for path in files {
        let Some(lang) = lang_for(&path) else {
            continue;
        };
        let source = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let tree = pool.with_parser(lang, |parser| parser.parse(&source, None))?;
        let Some(tree) = tree else {
            continue;
        };
        let query = Query::new(&language(lang), definition_query(lang))?;
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
        while let Some(m) = matches.next() {
            for capture in m.captures {
                let name = capture.node.utf8_text(source.as_bytes()).unwrap_or("");
                if let Some(want) = query_name {
                    if !name.eq_ignore_ascii_case(want) && !name.contains(want) {
                        continue;
                    }
                }
                let kind = capture
                    .node
                    .parent()
                    .map(|n| n.kind())
                    .unwrap_or("symbol")
                    .to_owned();
                hits.push(SymbolHit {
                    name: name.to_owned(),
                    kind,
                    path: relative(root, &path),
                    line: capture.node.start_position().row + 1,
                    column: capture.node.start_position().column + 1,
                });
                if hits.len() >= max_hits {
                    return Ok(hits);
                }
            }
        }
    }
    Ok(hits)
}

fn collect_references(root: &Path, symbol: &str, max_hits: usize) -> Result<Vec<SymbolHit>> {
    anyhow::ensure!(!symbol.is_empty(), "symbol required");
    let files = walk_sources(root, MAX_FILES)?;
    let mut hits = Vec::new();
    let pool = pool()?;
    for path in files {
        let Some(lang) = lang_for(&path) else {
            continue;
        };
        let source = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let tree = pool.with_parser(lang, |parser| parser.parse(&source, None))?;
        let Some(tree) = tree else {
            continue;
        };
        let query = Query::new(&language(lang), reference_query(lang))?;
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
        while let Some(m) = matches.next() {
            for capture in m.captures {
                let name = capture.node.utf8_text(source.as_bytes()).unwrap_or("");
                if name != symbol {
                    continue;
                }
                hits.push(SymbolHit {
                    name: name.to_owned(),
                    kind: "reference".into(),
                    path: relative(root, &path),
                    line: capture.node.start_position().row + 1,
                    column: capture.node.start_position().column + 1,
                });
                if hits.len() >= max_hits {
                    return Ok(hits);
                }
            }
        }
    }
    Ok(hits)
}

fn hit_json(hit: &SymbolHit) -> Value {
    json!({
        "name": hit.name,
        "kind": hit.kind,
        "path": hit.path,
        "line": hit.line,
        "column": hit.column
    })
}

pub fn workspace_symbols(root: &Path, query: &str, max_hits: usize) -> Result<Value> {
    let max_hits = max_hits.clamp(1, MAX_HITS);
    let hits = collect_definitions(root, Some(query).filter(|q| !q.is_empty()), max_hits)?;
    Ok(json!({
        "ok": true,
        "query": query,
        "count": hits.len(),
        "bounded": true,
        "symbols": hits.iter().map(hit_json).collect::<Vec<_>>(),
        "note": "Tree-sitter local parse; not a whole-world index or LSP."
    }))
}

pub fn goto_definition(root: &Path, symbol: &str, max_hits: usize) -> Result<Value> {
    anyhow::ensure!(!symbol.is_empty(), "symbol required");
    let max_hits = max_hits.clamp(1, MAX_HITS);
    let hits = collect_definitions(root, Some(symbol), max_hits)?;
    let exact: Vec<_> = hits
        .iter()
        .filter(|h| h.name == symbol)
        .cloned()
        .collect();
    let chosen = if exact.is_empty() { hits } else { exact };
    Ok(json!({
        "ok": !chosen.is_empty(),
        "symbol": symbol,
        "definitions": chosen.iter().map(hit_json).collect::<Vec<_>>(),
        "note": if chosen.is_empty() {
            "No definition found by local tree-sitter parse."
        } else {
            "Best-effort tree-sitter definition match; verify before editing."
        }
    }))
}

pub fn find_references(root: &Path, symbol: &str, max_hits: usize) -> Result<Value> {
    let max_hits = max_hits.clamp(1, MAX_HITS);
    let hits = collect_references(root, symbol, max_hits)?;
    Ok(json!({
        "ok": true,
        "symbol": symbol,
        "count": hits.len(),
        "references": hits.iter().map(hit_json).collect::<Vec<_>>(),
        "note": "Identifier matches from local tree-sitter parse; not a full type-aware reference graph."
    }))
}

pub fn rust_analyzer_available() -> bool {
    crate::sandbox::which("rust-analyzer").is_some()
}

pub async fn get_diagnostics(root: &Path, path: &str) -> Result<Value> {
    if !rust_analyzer_available() {
        bail!("rust-analyzer is not installed on PATH; diagnostics are unavailable");
    }
    let rel = Path::new(path);
    anyhow::ensure!(!rel.is_absolute(), "path must be workspace-relative");
    let full = root.join(rel);
    anyhow::ensure!(full.starts_with(root), "path escapes workspace");
    anyhow::ensure!(full.is_file(), "file not found");
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
    fn finds_rust_symbols_in_tiny_fixture() {
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
    }
}
