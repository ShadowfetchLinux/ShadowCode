//! `search_code`: BM25 over FTS5 code chunks, fused with embedding similarity
//! by reciprocal rank fusion (RRF) when a semantic model is installed. Falls
//! back to BM25 alone when there is no model or the embedding server fails.
use super::{chunks, embeddings};
use anyhow::{ensure, Context, Result};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::Duration,
};

const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "code", "do", "does", "for", "from", "how",
    "i", "in", "is", "it", "of", "on", "or", "that", "the", "this", "to", "what", "where", "which",
    "with",
];
/// Standard RRF constant; larger values flatten the rank curve.
pub const RRF_K: f64 = 60.0;
const MAX_HITS_PER_FILE: usize = 3;
const PREVIEW_LINES: usize = 14;
/// How long one search may spend embedding chunks that have no vector yet.
const FILL_BUDGET: Duration = Duration::from_secs(6);

/// Query words (camelCase and snake_case split, lower-cased, stopwords removed).
pub fn query_words(query: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let all: Vec<String> = chunks::split_words(query)
        .into_iter()
        .filter(|w| seen.insert(w.clone()))
        .collect();
    let kept: Vec<String> = all
        .iter()
        .filter(|w| !STOPWORDS.contains(&w.as_str()))
        .cloned()
        .collect();
    if kept.is_empty() {
        all
    } else {
        kept
    }
}

/// An FTS5 MATCH expression: every word as a quoted prefix term, OR-ed, so
/// BM25 rewards chunks that contain more of the words.
pub fn fts_expression(query: &str) -> Option<String> {
    let words = query_words(query);
    if words.is_empty() {
        return None;
    }
    Some(
        words
            .iter()
            .take(24)
            .map(|w| {
                if w.chars().count() >= 3 {
                    format!("\"{w}\"*")
                } else {
                    format!("\"{w}\"")
                }
            })
            .collect::<Vec<_>>()
            .join(" OR "),
    )
}

/// Chunk ids by BM25 (best first) with the raw score (lower is better).
pub fn bm25(
    conn: &Connection,
    query: &str,
    path_prefix: &str,
    limit: usize,
) -> Result<Vec<(i64, f64)>> {
    let Some(expression) = fts_expression(query) else {
        return Ok(Vec::new());
    };
    // Column weights: path, symbols, body, camelCase words.
    let mut stmt = conn.prepare_cached(
        "SELECT f.rowid, bm25(chunks_fts, 2.0, 6.0, 1.0, 1.0) AS score
         FROM chunks_fts f JOIN chunks c ON c.id = f.rowid
         WHERE chunks_fts MATCH ?1 AND substr(c.path, 1, length(?2)) = ?2
         ORDER BY score LIMIT ?3",
    )?;
    let rows = stmt
        .query_map(params![expression, path_prefix, limit as i64], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Reciprocal rank fusion: score(d) = Σ 1 / (k + rank_i(d)), ranks from 1.
/// Ties keep the order of first appearance.
pub fn rrf<K: Clone + Eq + std::hash::Hash>(lists: &[Vec<K>], k: f64) -> Vec<(K, f64)> {
    let mut scores: HashMap<K, (f64, usize)> = HashMap::new();
    let mut order = 0usize;
    for list in lists {
        for (rank, key) in list.iter().enumerate() {
            let entry = scores.entry(key.clone()).or_insert_with(|| {
                order += 1;
                (0.0, order)
            });
            entry.0 += 1.0 / (k + rank as f64 + 1.0);
        }
    }
    let mut fused: Vec<(K, f64, usize)> = scores.into_iter().map(|(k, (s, o))| (k, s, o)).collect();
    fused.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.2.cmp(&b.2)));
    fused.into_iter().map(|(k, s, _)| (k, s)).collect()
}

struct Row {
    path: String,
    start_line: i64,
    end_line: i64,
    symbols: String,
    body: String,
}

fn load(conn: &Connection, id: i64) -> Result<Row> {
    Ok(conn.query_row(
        "SELECT c.path, c.start_line, c.end_line, f.symbols, f.body FROM chunks c JOIN chunks_fts f ON f.rowid = c.id WHERE c.id = ?",
        [id],
        |r| {
            Ok(Row {
                path: r.get(0)?,
                start_line: r.get(1)?,
                end_line: r.get(2)?,
                symbols: r.get(3)?,
                body: r.get(4)?,
            })
        },
    )?)
}

/// A numbered excerpt starting near the first line that mentions a query word.
fn preview(row: &Row, words: &[String]) -> String {
    let lines: Vec<&str> = row.body.lines().collect();
    let first = lines
        .iter()
        .position(|line| {
            let lower = line.to_lowercase();
            words.iter().any(|w| lower.contains(w.as_str()))
        })
        .unwrap_or(0);
    let start = first.saturating_sub(2);
    lines
        .iter()
        .enumerate()
        .skip(start)
        .take(PREVIEW_LINES)
        .map(|(index, line)| {
            format!(
                "{}: {}",
                row.start_line + index as i64,
                crate::tools::truncate(line, 200)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Clone, Debug)]
pub struct Request {
    pub query: String,
    pub path: String,
    pub max_hits: usize,
}

fn normalize_prefix(root: &Path, path: &str) -> Result<String> {
    let path = path.trim();
    if path.is_empty() || path == "." {
        return Ok(String::new());
    }
    let workspace = crate::workspace::Workspace::open(root)?;
    let rel = workspace.relative(path)?;
    Ok(rel.to_string_lossy().replace('\\', "/"))
}

/// Full search: index, BM25, optional vectors, fusion, excerpts.
pub async fn search_code(
    root: PathBuf,
    request: Request,
    semantic: Option<embeddings::Semantic>,
) -> Result<Value> {
    ensure!(!request.query.trim().is_empty(), "query must not be empty");
    ensure!(request.query.len() <= 2_000, "query is too long");
    let max_hits = request.max_hits.clamp(1, 50);
    let prefix = normalize_prefix(&root, &request.path)?;
    let words = query_words(&request.query);
    ensure!(!words.is_empty(), "The query has no searchable words");
    let pool = max_hits * 4;
    let (index_root, query, bm_prefix) = (root.clone(), request.query.clone(), prefix.clone());
    let lexical = tokio::task::spawn_blocking(move || -> Result<Vec<(i64, f64)>> {
        crate::symbol_index::ensure_index(&index_root, &[], true)?;
        let conn = crate::symbol_index::open_index(&index_root)?;
        bm25(&conn, &query, &bm_prefix, pool)
    })
    .await
    .context("Search worker stopped")??;
    let mut semantic_note = Value::Null;
    let mut vector: Vec<(i64, f32)> = Vec::new();
    if let Some(semantic) = &semantic {
        match semantic_rank(&root, semantic, &request.query, &prefix, pool).await {
            Ok((ranked, filled, covered, total)) => {
                vector = ranked;
                semantic_note = json!({
                    "model": semantic.entry.id,
                    "embedded_now": filled,
                    "chunks_with_vectors": covered,
                    "chunks": total,
                    "note": if covered < total {
                        "Some chunks have no vector yet; they are found by keywords only until indexing finishes."
                    } else {
                        "All indexed chunks have vectors."
                    }
                });
            }
            Err(error) => {
                semantic_note = json!({
                    "model": semantic.entry.id,
                    "error": format!("{error:#}"),
                    "note": "Semantic ranking failed; results are keyword (BM25) only."
                });
            }
        }
    }
    let hybrid = !vector.is_empty();
    let lists = vec![
        lexical.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        vector.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
    ];
    let bm25_scores: HashMap<i64, f64> = lexical.iter().copied().collect();
    let similarity: HashMap<i64, f32> = vector.iter().copied().collect();
    let fused = rrf(&lists, RRF_K);
    let hits = tokio::task::spawn_blocking(move || -> Result<Vec<Value>> {
        let conn = crate::symbol_index::open_index(&root)?;
        let mut per_file: HashMap<String, usize> = HashMap::new();
        let mut hits = Vec::new();
        for (id, score) in fused {
            if hits.len() >= max_hits {
                break;
            }
            let Ok(row) = load(&conn, id) else {
                continue;
            };
            let seen = per_file.entry(row.path.clone()).or_default();
            if *seen >= MAX_HITS_PER_FILE {
                continue;
            }
            *seen += 1;
            let mut hit = json!({
                "path": row.path,
                "start_line": row.start_line,
                "end_line": row.end_line,
                "score": (score * 10_000.0).round() / 10_000.0,
                "preview": preview(&row, &words),
            });
            if !row.symbols.is_empty() {
                hit["symbols"] = json!(row.symbols);
            }
            if let Some(bm) = bm25_scores.get(&id) {
                hit["bm25"] = json!((bm * 1000.0).round() / 1000.0);
            }
            if let Some(sim) = similarity.get(&id) {
                hit["similarity"] = json!((*sim as f64 * 1000.0).round() / 1000.0);
            }
            hits.push(hit);
        }
        Ok(hits)
    })
    .await
    .context("Search worker stopped")??;
    Ok(json!({
        "ok": true,
        "query": request.query,
        "mode": if hybrid { "hybrid" } else { "bm25" },
        "count": hits.len(),
        "hits": hits,
        "semantic": semantic_note,
        "note": if hybrid {
            "Keyword (BM25) and embedding rankings fused with reciprocal rank fusion. Read the file before editing."
        } else {
            "Keyword (BM25) ranking over indexed code chunks. Read the file before editing."
        }
    }))
}

async fn semantic_rank(
    root: &Path,
    semantic: &embeddings::Semantic,
    query: &str,
    prefix: &str,
    limit: usize,
) -> Result<(Vec<(i64, f32)>, usize, i64, i64)> {
    let filled = embeddings::fill(root, semantic, FILL_BUDGET).await?;
    let query_vector = embeddings::embed(semantic, &[query.to_owned()], true)
        .await?
        .pop()
        .context("No query vector")?;
    let (root, data_dir, entry, prefix) = (
        root.to_owned(),
        semantic.data_dir.clone(),
        semantic.entry,
        prefix.to_owned(),
    );
    tokio::task::spawn_blocking(move || {
        let ranked = embeddings::rank(&root, &data_dir, entry, &query_vector, &prefix, limit)?;
        let (covered, total) = embeddings::coverage(&root, &data_dir, entry)?;
        Ok((ranked, filled, covered, total))
    })
    .await
    .context("Search worker stopped")?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn rrf_rewards_agreement_between_lists() {
        let fused = rrf(&[vec!["a", "b", "c"], vec!["c", "b", "d"]], RRF_K);
        let order: Vec<_> = fused.iter().map(|(k, _)| *k).collect();
        // b and c appear in both lists; c (ranks 3 and 1) edges out b (2 and 2),
        // and both beat a, which ranked first in only one list.
        assert_eq!(order[..3].to_vec(), vec!["c", "b", "a"]);
        assert!((fused[0].1 - (1.0 / 63.0 + 1.0 / 61.0)).abs() < 1e-12);
        assert!((fused[1].1 - 2.0 / 62.0).abs() < 1e-12);
        assert_eq!(order.len(), 4);
        // A single list keeps its order.
        let single = rrf(&[vec![3, 1, 2]], RRF_K);
        assert_eq!(
            single.iter().map(|(k, _)| *k).collect::<Vec<_>>(),
            [3, 1, 2]
        );
        assert!(rrf::<u8>(&[vec![], vec![]], RRF_K).is_empty());
    }

    #[test]
    fn query_words_drop_stopwords_and_split_identifiers() {
        assert_eq!(
            query_words("where is the parseConfig for http_client?"),
            ["parse", "config", "http", "client"]
        );
        assert_eq!(query_words("the"), ["the"]);
        assert_eq!(fts_expression("load db").unwrap(), "\"load\"* OR \"db\"");
        assert!(fts_expression("!!!").is_none());
    }

    #[test]
    fn bm25_finds_camel_case_and_symbol_matches_first() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("config.ts"),
            "export function parseConfig(text: string) {\n  return JSON.parse(text);\n}\n",
        )
        .unwrap();
        fs::write(
            src.join("net.py"),
            "def open_socket(host):\n    '''Open a TCP socket to the host.'''\n    return host\n",
        )
        .unwrap();
        fs::write(
            root.path().join("README.md"),
            "# Project\nNothing about sockets here, only docs.\n",
        )
        .unwrap();
        crate::symbol_index::ensure_index(root.path(), &[], true).unwrap();
        let conn = crate::symbol_index::open_index(root.path()).unwrap();
        let path_of = |id: i64| -> String {
            conn.query_row("SELECT path FROM chunks WHERE id=?", [id], |r| r.get(0))
                .unwrap()
        };
        let hits = bm25(&conn, "parse config", "", 5).unwrap();
        assert_eq!(path_of(hits[0].0), "src/config.ts");
        let hits = bm25(&conn, "open socket", "", 5).unwrap();
        assert_eq!(path_of(hits[0].0), "src/net.py");
        let scoped = bm25(&conn, "socket", "README", 5).unwrap();
        assert_eq!(scoped.len(), 1);
        assert_eq!(path_of(scoped[0].0), "README.md");
        assert!(bm25(&conn, "zebra", "", 5).unwrap().is_empty());
    }

    #[tokio::test]
    async fn search_code_reports_bm25_hits_with_previews_and_follows_edits() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("store.go"),
            "package store\n\n// SaveRecord writes one record to disk.\nfunc SaveRecord(path string) error {\n\treturn nil\n}\n",
        )
        .unwrap();
        let request = |q: &str| Request {
            query: q.into(),
            path: String::new(),
            max_hits: 5,
        };
        let out = search_code(root.path().to_owned(), request("save record"), None)
            .await
            .unwrap();
        assert_eq!(out["mode"], "bm25");
        assert_eq!(out["hits"][0]["path"], "store.go");
        assert!(out["hits"][0]["preview"]
            .as_str()
            .unwrap()
            .contains("4: func SaveRecord"));
        // An edit reaches the index through touch() without a rescan.
        fs::write(
            root.path().join("store.go"),
            "package store\n\nfunc LoadRecord(path string) error {\n\treturn nil\n}\n",
        )
        .unwrap();
        crate::symbol_index::touch(root.path(), "store.go").unwrap();
        let out = search_code(root.path().to_owned(), request("save record"), None)
            .await
            .unwrap();
        assert_eq!(out["count"], 1, "the prefix 'record' still matches: {out}");
        let out = search_code(root.path().to_owned(), request("SaveRecord"), None)
            .await
            .unwrap();
        assert!(!out.to_string().contains("SaveRecord writes"));
        assert!(search_code(root.path().to_owned(), request("   "), None)
            .await
            .is_err());
    }
}
