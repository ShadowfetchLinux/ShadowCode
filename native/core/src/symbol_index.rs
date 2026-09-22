//! On-demand AST symbol index for Rust and TypeScript in ordinary SQLite tables.
//! Not sqlite-vec, not embeddings, not BM25. Files are indexed when touched or
//! during a bounded project scan — never the whole world at startup.
use anyhow::{ensure, Context, Result};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
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

fn db_path(root: &Path) -> PathBuf {
    root.join(".shadow").join("symbol-index.sqlite")
}

fn open_db(root: &Path) -> Result<Connection> {
    let path = db_path(root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(&path).with_context(|| format!("open {}", path.display()))?;
    conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;")?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}

fn extract_signature(source: &str, node: tree_sitter::Node) -> String {
    let parent = node.parent().unwrap_or(node);
    let start = parent.start_byte();
    let end = parent.end_byte().min(source.len());
    let slice = &source[start..end];
    let first = slice.lines().next().unwrap_or(slice).trim();
    let mut out = first.to_owned();
    if out.len() > 240 {
        out.truncate(240);
        out.push('…');
    }
    out
}

fn parse_symbols(_path: &Path, source: &str, lang: Lang) -> Result<Vec<(String, String, usize, usize, String)>> {
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
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if out.len() >= limit {
                return out;
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
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.is_dir() {
                stack.push(path);
            } else if lang_for(&path).is_some() && meta.len() <= MAX_FILE_BYTES as u64 {
                out.push(path);
            }
        }
    }
    out
}

fn index_file(conn: &Connection, root: &Path, path: &Path) -> Result<usize> {
    let Some(lang) = lang_for(path) else {
        return Ok(0);
    };
    let rel = relative(root, path);
    let Ok(source) = fs::read_to_string(path) else {
        return Ok(0);
    };
    if source.len() > MAX_FILE_BYTES {
        return Ok(0);
    }
    let mt = mtime_ns(path) as i64;
    let size = source.len() as i64;
    let existing: Option<(i64, i64)> = conn
        .query_row(
            "SELECT mtime_ns, size FROM files WHERE path=?",
            [&rel],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();
    if existing == Some((mt, size)) {
        return Ok(0);
    }
    let symbols = parse_symbols(path, &source, lang)?;
    conn.execute("DELETE FROM symbols WHERE path=?", [&rel])?;
    conn.execute("DELETE FROM files WHERE path=?", [&rel])?;
    conn.execute(
        "INSERT INTO files(path, lang, mtime_ns, size, indexed_at) VALUES(?,?,?,?,?)",
        params![rel, lang_name(lang), mt, size, now()],
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
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0))?;
    Ok(json!({
        "ok": true,
        "indexed_files": indexed_files,
        "symbols_written": symbol_count,
        "symbols_total": total,
        "scanned_files": scanned,
        "bounded": true,
        "db": relative(root, &db_path(root)),
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
    ensure!(!symbol.is_empty(), "symbol required");
    let _ = ensure_index(root, &[], true)?;
    let conn = open_db(root)?;
    let mut stmt = conn.prepare(
        "SELECT path, name, kind, line, column, signature FROM symbols WHERE name = ?1 OR name LIKE '%' || ?1 || '%' LIMIT ?2",
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
    ensure!(!symbol.is_empty(), "symbol required");
    let _ = ensure_index(root, &[], true)?;
    let files = walk_sources(root, MAX_SCAN_FILES);
    let mut hits = Vec::new();
    let mut truncated = false;
    for path in files {
        let Some(lang) = lang_for(&path) else {
            continue;
        };
        let Ok(source) = fs::read_to_string(&path) else {
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
    let refs = query_references(root, symbol, cap + 8)?;
    let defs = query_definitions(root, symbol, 16)?;
    let def_keys: Vec<(String, i64)> = defs["definitions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|d| {
            Some((
                d["path"].as_str()?.to_owned(),
                d["line"].as_i64().unwrap_or(0),
            ))
        })
        .collect();
    let mut callers = Vec::new();
    let mut truncated = refs["truncated"].as_bool().unwrap_or(false);
    for r in refs["references"].as_array().into_iter().flatten() {
        let path = r["path"].as_str().unwrap_or("");
        let line = r["line"].as_i64().unwrap_or(0);
        if def_keys.iter().any(|(p, l)| p == path && *l == line) {
            continue;
        }
        callers.push(r.clone());
        if callers.len() >= cap {
            truncated = true;
            break;
        }
    }
    Ok(json!({
        "symbol": symbol,
        "callers": callers,
        "count": callers.len(),
        "truncated": truncated,
        "cap": cap,
        "note": if truncated {
            format!("Caller list truncated at {cap}; more in-repo references may exist.")
        } else {
            "Bounded in-repo callers from the AST index.".into()
        }
    }))
}

/// Touch-index a relative path after an edit (no full scan).
pub fn touch(root: &Path, relative_path: &str) -> Result<()> {
    let _guard = INDEX_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("symbol index lock poisoned"))?;
    let conn = open_db(root)?;
    let full = root.join(relative_path);
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
    fn does_not_require_startup_world_index() {
        let root = tempfile::tempdir().unwrap();
        let status = ensure_index(root.path(), &[], false).unwrap();
        assert_eq!(status["ok"], true);
        assert!(status["db"].as_str().unwrap().contains("symbol-index"));
    }
}
