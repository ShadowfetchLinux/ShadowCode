//! Code chunks for full-text (FTS5/BM25) and semantic search.
//!
//! Files are cut into windows of about 24–64 lines, preferring to cut where a
//! definition starts (or at a blank line), so a hit usually covers one or a few
//! whole functions. Each chunk is stored twice: a plain row in `chunks` (line
//! range and a content digest that keys cached embeddings) and a row in the
//! `chunks_fts` virtual table with the same rowid.
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS chunks (
  id INTEGER PRIMARY KEY,
  path TEXT NOT NULL,
  start_line INTEGER NOT NULL,
  end_line INTEGER NOT NULL,
  digest TEXT NOT NULL,
  FOREIGN KEY(path) REFERENCES files(path) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_chunks_path ON chunks(path);
CREATE INDEX IF NOT EXISTS idx_chunks_digest ON chunks(digest);
CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
  path, symbols, body, words, tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER IF NOT EXISTS chunks_after_delete AFTER DELETE ON chunks BEGIN
  DELETE FROM chunks_fts WHERE rowid = old.id;
END;
"#;

const MIN_LINES: usize = 24;
const MAX_LINES: usize = 64;
const MAX_CHUNK_BYTES: usize = 8_000;
const MAX_CHUNKS_PER_FILE: usize = 400;

#[derive(Clone, Debug, PartialEq)]
pub struct Chunk {
    /// 1-based, inclusive.
    pub start_line: usize,
    pub end_line: usize,
    pub body: String,
    pub symbols: Vec<String>,
}

/// Split a file. `definitions` holds (1-based line, name) of symbols in it.
pub fn split(source: &str, definitions: &[(usize, String)]) -> Vec<Chunk> {
    let lines: Vec<&str> = source.lines().collect();
    let starts: BTreeSet<usize> = definitions.iter().map(|(line, _)| *line).collect();
    let mut chunks = Vec::new();
    let mut start = 0usize;
    while start < lines.len() && chunks.len() < MAX_CHUNKS_PER_FILE {
        let mut end = (start + MAX_LINES).min(lines.len());
        // Never let one chunk grow past the byte budget (minified lines).
        let mut bytes = 0usize;
        for (offset, line) in lines[start..end].iter().enumerate() {
            bytes += line.len() + 1;
            if bytes > MAX_CHUNK_BYTES && offset > 0 {
                end = start + offset;
                break;
            }
        }
        if end < lines.len() && end - start > MIN_LINES {
            // Cut before the last definition that starts inside the window,
            // else after the last blank line, else at the hard limit.
            let window = start + MIN_LINES..end;
            let at_definition = window
                .clone()
                .rev()
                .find(|index| starts.contains(&(index + 1)));
            let at_blank = window
                .rev()
                .find(|index| lines[*index].trim().is_empty())
                .map(|index| index + 1);
            if let Some(cut) = at_definition.or(at_blank) {
                end = cut;
            }
        }
        let body = lines[start..end].join("\n");
        if !body.trim().is_empty() {
            let symbols = definitions
                .iter()
                .filter(|(line, _)| (start + 1..=end).contains(line))
                .map(|(_, name)| name.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            chunks.push(Chunk {
                start_line: start + 1,
                end_line: end,
                body,
                symbols,
            });
        }
        start = end.max(start + 1);
    }
    chunks
}

/// Lower-case words of an identifier-ish text, splitting camelCase and
/// PascalCase (`parseHTTPConfig` → `parse http config`) as well as
/// `snake_case`, `kebab-case` and punctuation.
pub fn split_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    for token in text.split(|c: char| !c.is_alphanumeric()) {
        if token.is_empty() {
            continue;
        }
        let chars: Vec<char> = token.chars().collect();
        let mut current = String::new();
        for (index, ch) in chars.iter().enumerate() {
            let boundary = index > 0 && {
                let previous = chars[index - 1];
                let next_lower = chars.get(index + 1).is_some_and(|c| c.is_lowercase());
                (ch.is_uppercase() && (previous.is_lowercase() || previous.is_numeric()))
                    || (ch.is_uppercase() && previous.is_uppercase() && next_lower)
            };
            if boundary && !current.is_empty() {
                words.push(std::mem::take(&mut current).to_lowercase());
            }
            current.push(*ch);
        }
        if !current.is_empty() {
            words.push(current.to_lowercase());
        }
    }
    words
}

/// Extra tokens for identifiers whose case carries word boundaries, so a
/// query for "parse config" finds `parseConfig`. `snake_case` needs no help:
/// the FTS tokenizer already splits at underscores.
fn camel_words(body: &str) -> String {
    let mut seen = BTreeSet::new();
    for token in body.split(|c: char| !c.is_alphanumeric()) {
        if token.len() < 4 || token.len() > 80 {
            continue;
        }
        let has_case_change = token
            .chars()
            .zip(token.chars().skip(1))
            .any(|(a, b)| a.is_lowercase() && b.is_uppercase());
        if has_case_change {
            for word in split_words(token) {
                if word.len() > 1 {
                    seen.insert(word);
                }
            }
        }
        if seen.len() > 400 {
            break;
        }
    }
    seen.into_iter().collect::<Vec<_>>().join(" ")
}

/// The text a semantic model sees for a chunk; its digest keys the vector cache.
pub fn embed_text(path: &str, symbols: &str, body: &str) -> String {
    format!("{path}\n{symbols}\n{body}")
}

pub fn digest(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

/// Replace the chunks of one file (the caller deletes old rows first or the
/// file row cascade does).
pub fn store(conn: &Connection, path: &str, chunks: &[Chunk]) -> rusqlite::Result<usize> {
    let mut insert_chunk = conn
        .prepare_cached("INSERT INTO chunks(path, start_line, end_line, digest) VALUES(?,?,?,?)")?;
    let mut insert_fts = conn.prepare_cached(
        "INSERT INTO chunks_fts(rowid, path, symbols, body, words) VALUES(?,?,?,?,?)",
    )?;
    for chunk in chunks {
        let symbols = chunk.symbols.join(" ");
        let digest = digest(&embed_text(path, &symbols, &chunk.body));
        insert_chunk.execute(params![
            path,
            chunk.start_line as i64,
            chunk.end_line as i64,
            digest
        ])?;
        let id = conn.last_insert_rowid();
        insert_fts.execute(params![
            id,
            path,
            symbols,
            chunk.body,
            camel_words(&chunk.body)
        ])?;
    }
    Ok(chunks.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_words_like_identifiers() {
        assert_eq!(
            split_words("parseHTTPConfig snake_case kebab-case X1Y"),
            vec!["parse", "http", "config", "snake", "case", "kebab", "case", "x1", "y"]
        );
    }

    #[test]
    fn chunks_prefer_definition_boundaries_and_cover_every_line() {
        let mut source = String::new();
        let mut defs = Vec::new();
        for f in 0..10 {
            defs.push((source.lines().count() + 1, format!("f{f}")));
            source.push_str(&format!("fn f{f}() {{\n"));
            for i in 0..14 {
                source.push_str(&format!("    let v{i} = {i};\n"));
            }
            source.push_str("}\n");
        }
        let chunks = split(&source, &defs);
        assert!(chunks.len() >= 3, "{}", chunks.len());
        assert_eq!(chunks[0].start_line, 1);
        assert_eq!(chunks.last().unwrap().end_line, source.lines().count());
        for pair in chunks.windows(2) {
            assert_eq!(pair[0].end_line + 1, pair[1].start_line);
            // Every later chunk begins exactly at a definition.
            assert!(defs.iter().any(|(line, _)| *line == pair[1].start_line));
        }
        assert!(chunks[0].symbols.contains(&"f0".to_owned()));
    }

    #[test]
    fn camel_case_words_are_indexed() {
        assert_eq!(camel_words("let x = parseConfig(y);"), "config parse");
        assert_eq!(camel_words("snake_case only"), "");
    }
}
