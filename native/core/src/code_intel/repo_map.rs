//! Ranked repository map (after Aider's repo map).
//!
//! Files are nodes; an edge runs from a file that uses a name to each file
//! that defines it, weighted by how often it is used. PageRank over that
//! graph, personalized toward focus files (recent edits, files named in the
//! task) and toward names the task mentions, ranks definitions. The best
//! ones are rendered as a compact outline that fits a token budget.
use super::chunks::split_words;
use anyhow::Result;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
    time::Duration,
};

const DAMPING: f64 = 0.85;
const MAX_LINES_PER_NAME: usize = 3;

/// Power-iteration PageRank on a weighted directed graph. `personalization`
/// may be all zeros (uniform teleport). Dangling nodes teleport too.
pub fn pagerank(nodes: usize, edges: &[(usize, usize, f64)], personalization: &[f64]) -> Vec<f64> {
    if nodes == 0 {
        return Vec::new();
    }
    let total: f64 = personalization.iter().sum();
    let teleport: Vec<f64> = if total > 0.0 {
        personalization.iter().map(|p| p / total).collect()
    } else {
        vec![1.0 / nodes as f64; nodes]
    };
    let mut out_weight = vec![0.0; nodes];
    for (src, _, w) in edges {
        out_weight[*src] += w;
    }
    let mut rank = teleport.clone();
    for _ in 0..100 {
        let dangling: f64 = (0..nodes)
            .filter(|i| out_weight[*i] <= 0.0)
            .map(|i| rank[i])
            .sum();
        let mut next: Vec<f64> = teleport
            .iter()
            .map(|t| (1.0 - DAMPING) * t + DAMPING * dangling * t)
            .collect();
        for (src, dst, w) in edges {
            if out_weight[*src] > 0.0 {
                next[*dst] += DAMPING * rank[*src] * w / out_weight[*src];
            }
        }
        let delta: f64 = next.iter().zip(&rank).map(|(a, b)| (a - b).abs()).sum();
        rank = next;
        if delta < 1e-10 {
            break;
        }
    }
    rank
}

fn is_long_identifier(name: &str) -> bool {
    name.len() >= 8
        && (name.contains('_')
            || name.contains('-')
            || name
                .chars()
                .zip(name.chars().skip(1))
                .any(|(a, b)| a.is_lowercase() && b.is_uppercase()))
}

/// Names in free text worth boosting (identifiers of three or more characters).
pub fn mentioned_names(text: &str) -> HashSet<String> {
    text.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|w| w.len() >= 3 && !w.chars().next().is_some_and(|c| c.is_numeric()))
        .map(str::to_owned)
        .collect()
}

struct Definition {
    line: i64,
    signature: String,
}

pub struct RepoMap {
    pub text: String,
    pub files: usize,
    pub symbols: usize,
    pub tokens: usize,
    pub focus: Vec<String>,
}

pub fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

/// Build the map from the index. `focus` are relative paths to favor;
/// `text` is the task or query whose names and file names get a boost.
pub fn build(root: &Path, focus: &[String], text: &str, max_tokens: usize) -> Result<RepoMap> {
    crate::symbol_index::ensure_index(root, &[], true)?;
    let conn = crate::symbol_index::open_index(root)?;
    let mut definitions: HashMap<(String, String), Vec<Definition>> = HashMap::new();
    let mut defines: HashMap<String, Vec<String>> = HashMap::new();
    {
        let mut stmt =
            conn.prepare("SELECT path, name, line, signature FROM symbols ORDER BY path, line")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        for row in rows {
            let (path, name, line, signature) = row?;
            let entry = definitions.entry((path.clone(), name.clone())).or_default();
            if entry.is_empty() {
                defines.entry(name).or_default().push(path);
            }
            entry.push(Definition { line, signature });
        }
    }
    let mut references: HashMap<String, Vec<(String, i64)>> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT path, name, count FROM refs WHERE name IN (SELECT name FROM symbols)",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?;
        for row in rows {
            let (path, name, count) = row?;
            // The identifier at a definition site is not a use of it.
            let own = definitions
                .get(&(path.clone(), name.clone()))
                .map_or(0, |d| d.len() as i64);
            if count - own > 0 {
                references
                    .entry(name)
                    .or_default()
                    .push((path, count - own));
            }
        }
    }
    let files: Vec<String> = conn
        .prepare("SELECT path FROM files ORDER BY path")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let index: HashMap<&str, usize> = files
        .iter()
        .enumerate()
        .map(|(i, p)| (p.as_str(), i))
        .collect();

    // Focus: explicit files plus files the text names by path or file name.
    let lower = text.to_lowercase();
    let words: HashSet<String> = lower
        .split(|c: char| c.is_whitespace() || matches!(c, '`' | '"' | '\'' | ',' | '(' | ')'))
        .map(|w| {
            w.trim_matches(|c: char| matches!(c, '.' | ':' | ';'))
                .to_owned()
        })
        .collect();
    let mut focus_set: HashSet<String> = focus
        .iter()
        .filter(|p| index.contains_key(p.as_str()))
        .cloned()
        .collect();
    for path in &files {
        let name = Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if (path.contains('/') && lower.contains(&path.to_lowercase()))
            || (name.contains('.') && words.contains(&name))
        {
            focus_set.insert(path.clone());
        }
    }
    let mentioned = mentioned_names(text);
    let mentioned_words: HashSet<String> = split_words(text).into_iter().collect();

    let mut personalization = vec![0.0; files.len()];
    for path in &focus_set {
        personalization[index[path.as_str()]] = 1.0;
    }
    let mut edges: Vec<(usize, usize, f64)> = Vec::new();
    let mut edge_names: Vec<&str> = Vec::new();
    for (name, definers) in &defines {
        let mut mul = 1.0;
        if mentioned.contains(name) {
            mul *= 10.0;
        } else if split_words(name)
            .iter()
            .all(|w| mentioned_words.contains(w))
            && name.len() >= 4
        {
            mul *= 3.0;
        }
        if is_long_identifier(name) {
            mul *= 10.0;
        }
        if name.starts_with('_') {
            mul *= 0.1;
        }
        if definers.len() > 5 {
            mul *= 0.1;
        }
        let Some(users) = references.get(name) else {
            // Keep never-used definitions reachable with a token weight.
            for definer in definers {
                let node = index[definer.as_str()];
                edges.push((node, node, 0.1 * mul));
                edge_names.push(name);
            }
            continue;
        };
        for (user, count) in users {
            let Some(&src) = index.get(user.as_str()) else {
                continue;
            };
            let use_mul = if focus_set.contains(user) {
                mul * 50.0
            } else {
                mul
            };
            for definer in definers {
                edges.push((
                    src,
                    index[definer.as_str()],
                    use_mul * (*count as f64).sqrt(),
                ));
                edge_names.push(name);
            }
        }
    }
    let rank = pagerank(files.len(), &edges, &personalization);
    let mut out_weight = vec![0.0; files.len()];
    for (src, _, w) in &edges {
        out_weight[*src] += w;
    }
    let mut ranked: HashMap<(usize, &str), f64> = HashMap::new();
    for ((src, dst, w), name) in edges.iter().zip(&edge_names) {
        *ranked.entry((*dst, name)).or_default() += rank[*src] * w / out_weight[*src];
    }
    let mut ranked: Vec<((usize, &str), f64)> = ranked.into_iter().collect();
    ranked.sort_by(|a, b| {
        b.1.total_cmp(&a.1)
            .then_with(|| files[a.0 .0].cmp(&files[b.0 .0]))
            .then_with(|| a.0 .1.cmp(b.0 .1))
    });
    let tags: Vec<(&str, &str)> = ranked
        .iter()
        .map(|((file, name), _)| (files[*file].as_str(), *name))
        .collect();

    let render = |count: usize| -> String {
        let mut order: Vec<&str> = Vec::new();
        let mut chosen: BTreeMap<&str, Vec<&Definition>> = BTreeMap::new();
        for (path, name) in tags.iter().take(count) {
            if !chosen.contains_key(path) {
                order.push(path);
            }
            let list = chosen.entry(path).or_default();
            if let Some(defs) = definitions.get(&(path.to_string(), name.to_string())) {
                list.extend(defs.iter().take(MAX_LINES_PER_NAME));
            }
        }
        let mut out = String::new();
        for path in order {
            let mut defs = chosen.remove(path).unwrap_or_default();
            defs.sort_by_key(|d| d.line);
            defs.dedup_by_key(|d| d.line);
            out.push_str(path);
            out.push_str(":\n");
            for def in defs {
                out.push_str(&format!("{:>5}│ {}\n", def.line, def.signature));
            }
        }
        out
    };
    // Largest prefix of the ranking that fits the budget.
    let (mut low, mut high) = (0usize, tags.len());
    while low < high {
        let mid = (low + high).div_ceil(2);
        if estimate_tokens(&render(mid)) <= max_tokens {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    let text = render(low);
    let mut focus: Vec<String> = focus_set.into_iter().collect();
    focus.sort();
    Ok(RepoMap {
        files: text.lines().filter(|l| !l.starts_with(' ')).count(),
        symbols: low,
        tokens: estimate_tokens(&text),
        text,
        focus,
    })
}

pub fn build_json(root: &Path, focus: &[String], text: &str, max_tokens: usize) -> Result<Value> {
    let map = build(root, focus, text, max_tokens.clamp(64, 16_384))?;
    Ok(json!({
        "ok": true,
        "map": map.text,
        "files": map.files,
        "symbols": map.symbols,
        "tokens_estimate": map.tokens,
        "focus": map.focus,
        "note": if map.symbols == 0 {
            "The index has no definitions for this project (no supported source files found)."
        } else {
            "Definitions ranked by PageRank over which files use which names, favoring focus files and names in the query. Line numbers come from the last index; read files before editing."
        }
    }))
}

/// The repo map section for a native task's system prompt, or None when it
/// is off, the model's context is small, the project has no code, or the
/// index takes too long.
pub async fn system_note(
    root: PathBuf,
    task: String,
    config: &crate::config::Config,
) -> Option<String> {
    let intel = super::CodeIntelConfig::lenient(config);
    let budget = intel.repo_map_tokens.min(config.model.context_limit / 32);
    if budget < 256 {
        return None;
    }
    let focus = crate::symbol_index::recent_edits(&root, 12);
    let work = tokio::task::spawn_blocking(move || build(&root, &focus, &task, budget));
    let map = tokio::time::timeout(Duration::from_secs(4), work)
        .await
        .ok()?
        .ok()?
        .ok()?;
    if map.symbols == 0 {
        return None;
    }
    Some(format!(
        "Repository map (definitions ranked by how the code uses them, from the local index; line numbers may be stale, so read files before editing; call repo_map or search_code for more):\n{}",
        map.text
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn pagerank_sums_to_one_and_follows_links() {
        // 0 -> 2, 1 -> 2, 2 -> 0
        let rank = pagerank(3, &[(0, 2, 1.0), (1, 2, 1.0), (2, 0, 1.0)], &[0.0; 3]);
        assert!((rank.iter().sum::<f64>() - 1.0).abs() < 1e-9);
        assert!(rank[2] > rank[0] && rank[0] > rank[1]);
        // Personalization pulls rank toward the chosen node.
        let biased = pagerank(
            3,
            &[(0, 2, 1.0), (1, 2, 1.0), (2, 0, 1.0)],
            &[0.0, 1.0, 0.0],
        );
        assert!(biased[1] > rank[1]);
        // Dangling nodes do not leak rank.
        let dangling = pagerank(2, &[(0, 1, 1.0)], &[0.0; 2]);
        assert!((dangling.iter().sum::<f64>() - 1.0).abs() < 1e-9);
        assert!(pagerank(0, &[], &[]).is_empty());
    }

    fn fixture() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("src");
        fs::create_dir_all(&src).unwrap();
        // `core.py` defines what everything else uses; `leaf.py` is used by no one.
        fs::write(
            src.join("core.py"),
            "class DatabaseConnection:\n    def execute_query(self, sql):\n        return sql\n\ndef open_database_connection():\n    return DatabaseConnection()\n",
        )
        .unwrap();
        for name in ["api", "jobs", "cli"] {
            fs::write(
                src.join(format!("{name}.py")),
                format!(
                    "from core import open_database_connection\n\ndef {name}_handler():\n    conn = open_database_connection()\n    return conn.execute_query('select 1')\n"
                ),
            )
            .unwrap();
        }
        fs::write(
            src.join("leaf.py"),
            "def render_leaf_template():\n    return format_leaf_title()\n\ndef format_leaf_title():\n    return 'leaf'\n",
        )
        .unwrap();
        root
    }

    #[test]
    fn ranks_widely_used_definitions_first() {
        let root = fixture();
        let map = build(root.path(), &[], "", 4_000).unwrap();
        let first_file = map.text.lines().next().unwrap();
        assert_eq!(first_file, "src/core.py:", "{}", map.text);
        assert!(map.text.contains("def open_database_connection():"));
        assert!(
            map.text.contains("    5│ def open_database_connection():"),
            "{}",
            map.text
        );
        assert!(map.symbols >= 3);
    }

    #[test]
    fn personalization_and_mentions_move_files_up() {
        let root = fixture();
        let neutral = build(root.path(), &[], "", 4_000).unwrap();
        let position = |text: &str, file: &str| {
            text.lines()
                .filter(|l| !l.starts_with(' '))
                .position(|l| l == file)
                .unwrap()
        };
        let focused = build(root.path(), &["src/leaf.py".into()], "", 4_000).unwrap();
        assert!(
            position(&focused.text, "src/leaf.py:") < position(&neutral.text, "src/leaf.py:"),
            "{}\n---\n{}",
            neutral.text,
            focused.text
        );
        assert_eq!(focused.focus, ["src/leaf.py"]);
        let named = build(
            root.path(),
            &[],
            "fix a bug in leaf.py around render_leaf_template",
            4_000,
        )
        .unwrap();
        assert_eq!(named.focus, ["src/leaf.py"]);
        assert_eq!(position(&named.text, "src/leaf.py:"), 0, "{}", named.text);
    }

    #[test]
    fn respects_the_token_budget() {
        let root = fixture();
        let small = build(root.path(), &[], "", 40).unwrap();
        assert!(small.tokens <= 40, "{}", small.tokens);
        assert!(small.symbols >= 1);
        let large = build(root.path(), &[], "", 4_000).unwrap();
        assert!(large.symbols > small.symbols);
        let empty = tempfile::tempdir().unwrap();
        let none = build(empty.path(), &[], "", 1_000).unwrap();
        assert_eq!(none.symbols, 0);
        assert!(none.text.is_empty());
    }
}
