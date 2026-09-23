//! On-demand AST symbol index for Rust and TypeScript in ordinary SQLite tables.
//! Not sqlite-vec, not embeddings, not BM25. Files are indexed when touched or
//! during a bounded project scan — never the whole world at startup.
use crate::workspace::Workspace;
use anyhow::{ensure, Context, Result};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};
use streaming_iterator::StreamingIterator;
use tree_sitter::{Parser, Query, QueryCursor};

const MAX_SCAN_FILES: usize = 200;
const MAX_FILE_BYTES: usize = 512_000;
const MAX_CALLERS: usize = 24;
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS meta (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS files (
  path TEXT PRIMARY KEY,
  lang TEXT NOT NULL,
  mtime_ns INTEGER NOT NULL,
  size INTEGER NOT NULL,
  digest TEXT NOT NULL,
  indexed_at REAL NOT NULL
);
CREATE TABLE IF NOT EXISTS symbols (
  id INTEGER PRIMARY KEY,
  path TEXT NOT NULL,
  name TEXT NOT NULL,
  kind TEXT NOT NULL,
  line INTEGER NOT NULL,
  column INTEGER NOT NULL,
  signature TEXT NOT NULL,
  FOREIGN KEY(path) REFERENCES files(path) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
CREATE INDEX IF NOT EXISTS idx_symbols_path ON symbols(path);
"#;

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

fn lang_name(lang: Lang) -> &'static str {
    match lang {
        Lang::Rust => "rust",
        Lang::TypeScript => "typescript",
        Lang::Tsx => "tsx",
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
        Lang::Rust => {
            r#"
            (function_item name: (identifier) @name)
            (struct_item name: (type_identifier) @name)
            (enum_item name: (type_identifier) @name)
            (trait_item name: (type_identifier) @name)
            (impl_item type: (type_identifier) @name)
            (mod_item name: (identifier) @name)
            (const_item name: (identifier) @name)
            (static_item name: (identifier) @name)
            (type_item name: (type_identifier) @name)
        "#
        }
        Lang::TypeScript | Lang::Tsx => {
            r#"
            (function_declaration name: (identifier) @name)
            (class_declaration name: (type_identifier) @name)
            (interface_declaration name: (type_identifier) @name)
            (type_alias_declaration name: (type_identifier) @name)
            (enum_declaration name: (identifier) @name)
            (lexical_declaration (variable_declarator name: (identifier) @name))
            (method_definition name: (property_identifier) @name)
        "#
        }
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

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn mtime_ns(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

// Keep derived data out of the project (including read-only reviews). The
// private process cache cannot be redirected by a project's .shadow symlink.
static CACHE: Mutex<Option<tempfile::TempDir>> = Mutex::new(None);
fn db_path(root: &Path) -> Result<PathBuf> {
    let root = root.canonicalize()?;
    let key = format!("{:x}", Sha256::digest(root.as_os_str().as_encoded_bytes()));
    let mut cache = CACHE
        .lock()
        .map_err(|_| anyhow::anyhow!("Index cache lock poisoned"))?;
    if cache.is_none() {
        *cache = Some(
            tempfile::Builder::new()
                .prefix("shadowcode-symbols-")
                .tempdir()?,
        );
    }
    Ok(cache
        .as_ref()
        .context("Index cache unavailable")?
        .path()
        .join(format!("{key}.sqlite")))
}
fn open_db(root: &Path) -> Result<Connection> {
    let path = db_path(root)?;
    let conn = Connection::open(&path).with_context(|| format!("open {}", path.display()))?;
    conn.busy_timeout(std::time::Duration::from_secs(3))?;
    conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;")?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}
fn read_source(root: &Path, path: &Path) -> Result<String> {
    let workspace = Workspace::open(root)?;
    let rel = workspace.relative(path.to_str().context("Source path is not UTF-8")?)?;
    ensure!(
        !crate::redaction::is_secret_path(&rel.to_string_lossy()),
        "Secret paths are not indexed"
    );
    let file = workspace.read(&rel.to_string_lossy())?;
    ensure!(
        file.bytes <= MAX_FILE_BYTES,
        "Source exceeds the AST index byte limit"
    );
    Ok(file.content)
}
fn extract_signature(source: &str, node: tree_sitter::Node) -> String {
    let parent = node.parent().unwrap_or(node);
    let start = parent.start_byte();
    let end = parent.end_byte().min(source.len());
    let slice = &source[start..end];
    let first = slice.lines().next().unwrap_or(slice).trim();
    let mut out = first.to_owned();
    if out.len() > 240 {
        let mut end = 240;
        while !out.is_char_boundary(end) {
            end -= 1;
        }
        out.truncate(end);
        out.push('…');
    }
    out
}

type ParsedSymbol = (String, String, usize, usize, String);
fn parse_symbols(_path: &Path, source: &str, lang: Lang) -> Result<Vec<ParsedSymbol>> {
    let mut parser = Parser::new();
    parser.set_language(&language(lang))?;
    let Some(tree) = parser.parse(source, None) else {
        return Ok(Vec::new());
    };
    let query = Query::new(&language(lang), definition_query(lang))?;
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
    let mut out = Vec::new();
    while let Some(m) = matches.next() {
        for capture in m.captures {
            let name = capture
                .node
                .utf8_text(source.as_bytes())
                .unwrap_or("")
                .to_owned();
            if name.is_empty() {
                continue;
            }
            let kind = capture
                .node
                .parent()
                .map(|n| n.kind())
                .unwrap_or("symbol")
                .to_owned();
            let signature = extract_signature(source, capture.node);
            out.push((
                name,
                kind,
                capture.node.start_position().row + 1,
                capture.node.start_position().column + 1,
                signature,
            ));
        }
    }
    Ok(out)
}

fn walk_sources(root: &Path, limit: usize) -> Vec<PathBuf> {
    ignore::WalkBuilder::new(root)
        .follow_links(false)
        .max_depth(Some(32))
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            entry.depth() == 0
                || (!name.starts_with('.')
                    && !matches!(name.as_ref(), "node_modules" | "target" | "dist")
                    && !crate::redaction::is_secret_path(&entry.path().to_string_lossy()))
        })
        .build()
        .take(20_000)
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry.file_type().is_some_and(|kind| kind.is_file())
                && lang_for(entry.path()).is_some()
                && entry
                    .metadata()
                    .is_ok_and(|metadata| metadata.len() <= MAX_FILE_BYTES as u64)
        })
        .take(limit)
        .map(|entry| entry.into_path())
        .collect()
}

fn index_file(conn: &Connection, root: &Path, path: &Path) -> Result<usize> {
    let Some(lang) = lang_for(path) else {
        return Ok(0);
    };
    let rel = relative(root, path);
    let Ok(source) = read_source(root, path) else {
        return Ok(0);
    };
    if source.len() > MAX_FILE_BYTES {
        return Ok(0);
    }
    let mt = mtime_ns(path) as i64;
    let size = source.len() as i64;
    let digest = format!("{:x}", Sha256::digest(source.as_bytes()));
    let existing: Option<(i64, i64, String)> = conn
        .query_row(
            "SELECT mtime_ns, size, digest FROM files WHERE path=?",
            [&rel],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .ok();
    if existing == Some((mt, size, digest.clone())) {
        return Ok(0);
    }
    let symbols = parse_symbols(path, &source, lang)?;
    conn.execute("DELETE FROM symbols WHERE path=?", [&rel])?;
    conn.execute("DELETE FROM files WHERE path=?", [&rel])?;
    conn.execute(
        "INSERT INTO files(path, lang, mtime_ns, size, digest, indexed_at) VALUES(?,?,?,?,?,?)",
        params![rel, lang_name(lang), mt, size, digest, now()],
    )?;
    for (name, kind, line, column, signature) in &symbols {
        conn.execute(
            "INSERT INTO symbols(path, name, kind, line, column, signature) VALUES(?,?,?,?,?,?)",
            params![rel, name, kind, *line as i64, *column as i64, signature],
        )?;
    }
    Ok(symbols.len())
}

/// Index specific relative paths (touched files) and/or a bounded scan.
pub fn ensure_index(root: &Path, touched: &[String], scan: bool) -> Result<Value> {
    ensure!(root.is_dir(), "workspace root required");
    let _guard = INDEX_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("symbol index lock poisoned"))?;
    let conn = open_db(root)?;
    let transaction = conn.unchecked_transaction()?;
    let workspace = Workspace::open(root)?;
    for path in touched {
        let rel = workspace.relative(path)?;
        ensure!(
            !crate::redaction::is_secret_path(&rel.to_string_lossy()),
            "Secret paths are not indexed"
        );
        if root.join(&rel).exists() {
            read_source(root, &root.join(&rel))?;
        }
    }
    let mut indexed_files = 0usize;
    let mut symbol_count = 0usize;
    let mut paths: Vec<PathBuf> = touched
        .iter()
        .filter_map(|p| {
            let full = root.join(p);
            if full.is_file() {
                Some(full)
            } else {
                None
            }
        })
        .collect();
    let scanned = if scan || paths.is_empty() {
        let walked = walk_sources(root, MAX_SCAN_FILES);
        let n = walked.len();
        paths.extend(walked);
        n
    } else {
        0
    };
    paths.sort();
    paths.dedup();
    for path in &paths {
        let n = index_file(&conn, root, path)?;
        if n > 0
            || conn
                .query_row(
                    "SELECT 1 FROM files WHERE path=?",
                    [relative(root, path)],
                    |_| Ok(()),
                )
                .is_ok()
        {
            indexed_files += 1;
            symbol_count += n;
        }
    }
    // Remove stale entries for deleted, renamed, oversized, or inaccessible files.
    let stored: Vec<String> = conn
        .prepare("SELECT path FROM files")?
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for path in stored {
        if read_source(root, &root.join(&path)).is_err() {
            conn.execute("DELETE FROM files WHERE path=?", [path])?;
        }
    }
    transaction.commit()?;
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0))?;
    Ok(json!({
        "ok": true,
        "indexed_files": indexed_files,
        "symbols_written": symbol_count,
        "symbols_total": total,
        "scanned_files": scanned,
        "bounded": true,
        "storage": "private process cache; workspace is unchanged",
        "note": "Ordinary SQLite AST tables; not embeddings or a world index."
    }))
}

fn hit_json(path: &str, name: &str, kind: &str, line: i64, column: i64, signature: &str) -> Value {
    json!({
        "path": path,
        "name": name,
        "kind": kind,
        "line": line,
        "column": column,
        "signature": signature
    })
}

pub fn query_definitions(root: &Path, symbol: &str, max_hits: usize) -> Result<Value> {
    let max_hits = max_hits.clamp(1, 80);
    let _ = ensure_index(root, &[], true)?;
    let conn = open_db(root)?;
    let mut stmt = conn.prepare(
        "SELECT path, name, kind, line, column, signature FROM symbols WHERE instr(name, ?1)>0 OR ?1='' ORDER BY name = ?1 DESC,path,line,column LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![symbol, max_hits as i64], |r| {
            Ok(hit_json(
                &r.get::<_, String>(0)?,
                &r.get::<_, String>(1)?,
                &r.get::<_, String>(2)?,
                r.get(3)?,
                r.get(4)?,
                &r.get::<_, String>(5)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect::<Vec<_>>();
    let exact: Vec<_> = rows
        .iter()
        .filter(|h| h["name"].as_str() == Some(symbol))
        .cloned()
        .collect();
    let chosen = if exact.is_empty() { rows } else { exact };
    Ok(json!({
        "ok": !chosen.is_empty(),
        "symbol": symbol,
        "definitions": chosen,
        "source": "sqlite-ast-index",
        "note": if chosen.is_empty() {
            "No definition in the bounded AST index."
        } else {
            "Definitions from on-demand SQLite AST index (tree-sitter)."
        }
    }))
}

pub fn query_references(root: &Path, symbol: &str, max_hits: usize) -> Result<Value> {
    references(root, symbol, max_hits, false)
}
fn references(root: &Path, symbol: &str, max_hits: usize, calls_only: bool) -> Result<Value> {
    ensure!(!symbol.is_empty(), "symbol required");
    let max_hits = max_hits.clamp(1, 80);
    let _ = ensure_index(root, &[], true)?;
    let files = walk_sources(root, MAX_SCAN_FILES);
    let mut hits = Vec::new();
    let mut truncated = false;
    for path in files {
        let Some(lang) = lang_for(&path) else {
            continue;
        };
        let Ok(source) = read_source(root, &path) else {
            continue;
        };
        let mut parser = Parser::new();
        parser.set_language(&language(lang))?;
        let Some(tree) = parser.parse(&source, None) else {
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
                if calls_only {
                    let mut node = capture.node;
                    let mut called = false;
                    while let Some(parent) = node.parent() {
                        if parent.kind() == "call_expression" {
                            called = parent.child_by_field_name("function").is_some_and(|f| {
                                f.start_byte() <= capture.node.start_byte()
                                    && f.end_byte() >= capture.node.end_byte()
                            });
                            break;
                        }
                        if !matches!(
                            parent.kind(),
                            "scoped_identifier"
                                | "field_expression"
                                | "member_expression"
                                | "generic_function"
                                | "parenthesized_expression"
                        ) {
                            break;
                        }
                        node = parent;
                    }
                    if !called {
                        continue;
                    }
                }
                hits.push(hit_json(
                    &relative(root, &path),
                    name,
                    "reference",
                    (capture.node.start_position().row + 1) as i64,
                    (capture.node.start_position().column + 1) as i64,
                    "",
                ));
                if hits.len() >= max_hits {
                    truncated = true;
                    break;
                }
            }
            if truncated {
                break;
            }
        }
        if truncated {
            break;
        }
    }
    Ok(json!({
        "ok": true,
        "symbol": symbol,
        "count": hits.len(),
        "truncated": truncated,
        "references": hits,
        "source": "sqlite-ast-index",
        "note": if truncated {
            format!("Reference list capped at {max_hits}; more may exist.")
        } else {
            "Identifier matches from bounded tree-sitter parse over indexed sources.".into()
        }
    }))
}

pub fn get_type_signature(root: &Path, symbol: &str) -> Result<Value> {
    ensure!(!symbol.is_empty(), "symbol required");
    let defs = query_definitions(root, symbol, 8)?;
    let arr = defs["definitions"].as_array().cloned().unwrap_or_default();
    let signatures: Vec<_> = arr
        .iter()
        .filter_map(|d| {
            let sig = d["signature"].as_str().unwrap_or("").trim();
            if sig.is_empty() {
                None
            } else {
                Some(json!({
                    "name": d["name"],
                    "path": d["path"],
                    "line": d["line"],
                    "signature": sig,
                    "kind": d["kind"]
                }))
            }
        })
        .collect();
    Ok(json!({
        "ok": !signatures.is_empty(),
        "symbol": symbol,
        "signatures": signatures,
        "source": "parser",
        "note": if signatures.is_empty() {
            "No parser signature available; rust-analyzer LSP is not required for this tool."
        } else {
            "Signature extracted from the local tree-sitter AST when LSP is absent."
        }
    }))
}

/// Bounded in-repo callers for a function name, for patch preparation context.
pub fn callers_for(root: &Path, symbol: &str, cap: usize) -> Result<Value> {
    let cap = cap.clamp(1, MAX_CALLERS);
    let refs = references(root, symbol, cap, true)?;
    let callers = refs["references"].as_array().cloned().unwrap_or_default();
    let truncated = refs["truncated"].as_bool().unwrap_or(false);
    Ok(json!({
        "symbol": symbol,
        "callers": callers,
        "count": callers.len(),
        "truncated": truncated,
        "cap": cap,
        "note": if truncated {
            format!("Caller list truncated at {cap}; more in-repo references may exist.")
        } else {
            "Syntactic call sites in bounded project sources; receiver types are not resolved.".into()
        }
    }))
}

/// Touch-index a relative path after an edit (no full scan).
pub fn touch(root: &Path, relative_path: &str) -> Result<()> {
    let _guard = INDEX_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("symbol index lock poisoned"))?;
    let conn = open_db(root)?;
    let workspace = Workspace::open(root)?;
    let rel = workspace.relative(relative_path)?;
    let full = root.join(&rel);
    if !full.exists() {
        // Delete by the normalized key the index stores, not the caller's
        // spelling ("./src/a.rs" must remove "src/a.rs").
        conn.execute("DELETE FROM files WHERE path=?", [relative(root, &full)])?;
        return Ok(());
    }
    read_source(root, &full)?;
    let _ = index_file(&conn, root, &full)?;
    Ok(())
}

static INDEX_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn indexes_rust_and_returns_signature_and_callers() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("lib.rs"),
            "pub fn greet(name: &str) -> String { format!(\"hi {name}\") }\npub fn run() { let _ = greet(\"x\"); }\n",
        )
        .unwrap();
        let status = ensure_index(root.path(), &["src/lib.rs".into()], false).unwrap();
        assert!(status["symbols_total"].as_i64().unwrap() >= 2);
        let sig = get_type_signature(root.path(), "greet").unwrap();
        assert!(sig["ok"].as_bool().unwrap());
        assert!(sig["signatures"][0]["signature"]
            .as_str()
            .unwrap()
            .contains("fn greet"));
        let callers = callers_for(root.path(), "greet", 8).unwrap();
        assert!(callers["count"].as_u64().unwrap() >= 1);
        let defs = query_definitions(root.path(), "greet", 10).unwrap();
        assert!(!defs["definitions"].as_array().unwrap().is_empty());
    }

    #[test]
    fn touch_removes_deleted_files_by_normalized_path() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("gone.rs"), "pub fn vanish() {}\n").unwrap();
        ensure_index(root.path(), &["src/gone.rs".into()], false).unwrap();
        assert!(query_definitions(root.path(), "vanish", 4).unwrap()["ok"] == true);
        fs::remove_file(src.join("gone.rs")).unwrap();
        // A differently spelled path for the same file must still drop the record.
        touch(root.path(), "./src/gone.rs").unwrap();
        let conn = open_db(root.path()).unwrap();
        let remaining: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM symbols WHERE name='vanish'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(remaining, 0);
    }

    #[test]
    fn does_not_require_startup_world_index() {
        let root = tempfile::tempdir().unwrap();
        let status = ensure_index(root.path(), &[], false).unwrap();
        assert_eq!(status["ok"], true);
        assert!(status["storage"].as_str().unwrap().contains("private"));
        assert!(!root.path().join(".shadow").exists());
    }

    #[test]
    fn indexes_typescript_definitions_references_and_signatures() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("math.ts"),
            "export function add(a: number, b: number): number { return a + b; }\nexport function useAdd() { return add(1, 2); }\n",
        )
        .unwrap();
        let status = ensure_index(root.path(), &["src/math.ts".into()], false).unwrap();
        assert!(status["symbols_total"].as_i64().unwrap() >= 2);
        let defs = query_definitions(root.path(), "add", 10).unwrap();
        assert!(!defs["definitions"].as_array().unwrap().is_empty());
        let refs = query_references(root.path(), "add", 16).unwrap();
        assert!(!refs["references"].as_array().unwrap().is_empty());
        let sig = get_type_signature(root.path(), "add").unwrap();
        assert!(sig["ok"].as_bool().unwrap());
        let callers = callers_for(root.path(), "add", 8).unwrap();
        assert!(callers["count"].as_u64().unwrap() >= 1);
    }
}
