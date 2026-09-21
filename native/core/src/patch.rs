//! Parse and preflight text patches without invoking a shell or mutating files.
use crate::workspace::{Snapshot, Workspace, MAX_FILE_BYTES};
use anyhow::{bail, ensure, Context, Result};
use std::collections::HashSet;

pub struct Change {
    pub path: String,
    pub before: Snapshot,
    pub after: Option<Vec<u8>>,
    pub mode: Option<u32>,
}
struct FilePatch {
    source: Option<String>,
    destination: Option<String>,
    hunks: Vec<Hunk>,
    added: Option<String>,
}
#[derive(Default)]
struct Hunk {
    old: String,
    new: String,
    line: Option<usize>,
    anchor: Option<String>,
    eof: bool,
    preserve_final_newline: bool,
}

pub fn prepare(workspace: &Workspace, patch: &str) -> Result<Vec<Change>> {
    ensure!(patch.len() <= 8_000_000, "Patch exceeds the 8 MB limit");
    let files = if patch.starts_with("*** Begin Patch") {
        parse_codex(patch)?
    } else {
        parse_unified(patch)?
    };
    ensure!(
        !files.is_empty() && files.len() <= 128,
        "Patch must change between 1 and 128 files"
    );
    let mut changes = Vec::new();
    let mut paths = HashSet::new();
    let mut bytes = 0;
    for file in files {
        let source = file
            .source
            .as_deref()
            .map(|p| normalize(workspace, p))
            .transpose()?;
        let destination = file
            .destination
            .as_deref()
            .map(|p| normalize(workspace, p))
            .transpose()?;
        let before = match &source {
            Some(path) => workspace.snapshot(path)?,
            None => Snapshot {
                bytes: None,
                mode: None,
                hash: None,
            },
        };
        let after = if let Some(added) = file.added {
            Some(added.into_bytes())
        } else if source.is_some() {
            let original = before
                .bytes
                .as_deref()
                .context("Patch source does not exist")?;
            ensure!(!original.contains(&0), "Cannot patch a binary file");
            let original = std::str::from_utf8(original).context("Cannot patch non-UTF-8 text")?;
            if file.hunks.is_empty() && destination.is_none() {
                None
            } else {
                ensure!(!file.hunks.is_empty(), "Patch has no hunks");
                let updated = apply_hunks(original, &file.hunks)?;
                if destination.is_none() {
                    ensure!(
                        updated.is_empty(),
                        "Deletion patch does not remove the complete file"
                    );
                    None
                } else {
                    Some(updated.into_bytes())
                }
            }
        } else {
            Some(apply_hunks("", &file.hunks)?.into_bytes())
        };
        if let Some(after) = &after {
            ensure!(after.len() <= MAX_FILE_BYTES, "Patched file exceeds 4 MB");
            bytes += after.len();
        }
        bytes += before.bytes.as_ref().map_or(0, Vec::len);
        ensure!(
            bytes <= 32_000_000,
            "Patch exceeds the 32 MB checkpoint budget"
        );
        if source == destination {
            let path = source.context("Patch has no source or destination")?;
            ensure!(
                paths.insert(path.clone()),
                "Duplicate path in patch: {path}"
            );
            changes.push(Change {
                path,
                mode: before.mode,
                before,
                after,
            });
        } else {
            if let Some(destination) = destination {
                ensure!(
                    paths.insert(destination.clone()),
                    "Duplicate path in patch: {destination}"
                );
                let target = workspace.snapshot(&destination)?;
                ensure!(
                    target.bytes.is_none(),
                    "Patch destination already exists: {destination}"
                );
                changes.push(Change {
                    path: destination,
                    before: target,
                    mode: before.mode,
                    after,
                });
            }
            if let Some(source) = source {
                ensure!(
                    paths.insert(source.clone()),
                    "Duplicate path in patch: {source}"
                );
                changes.push(Change {
                    path: source,
                    mode: before.mode,
                    before,
                    after: None,
                });
            }
        }
    }
    Ok(changes)
}
fn normalize(workspace: &Workspace, path: &str) -> Result<String> {
    ensure!(!path.is_empty(), "Empty patch path");
    Ok(workspace.writable(path)?.to_string_lossy().into_owned())
}
fn parse_codex(patch: &str) -> Result<Vec<FilePatch>> {
    let lines: Vec<_> = patch.lines().collect();
    ensure!(
        lines.first() == Some(&"*** Begin Patch") && lines.last() == Some(&"*** End Patch"),
        "Patch must have complete Begin/End markers"
    );
    let mut result = Vec::new();
    let mut i = 1;
    while i + 1 < lines.len() {
        let header = lines[i];
        i += 1;
        if let Some(path) = header.strip_prefix("*** Add File: ") {
            let mut content = String::new();
            while i < lines.len() && !lines[i].starts_with("*** ") {
                content.push_str(
                    lines[i]
                        .strip_prefix('+')
                        .context("Added file lines must begin with +")?,
                );
                content.push('\n');
                i += 1;
            }
            result.push(FilePatch {
                source: None,
                destination: Some(path.into()),
                hunks: Vec::new(),
                added: Some(content),
            });
        } else if let Some(path) = header.strip_prefix("*** Delete File: ") {
            result.push(FilePatch {
                source: Some(path.into()),
                destination: None,
                hunks: Vec::new(),
                added: None,
            });
        } else if let Some(path) = header.strip_prefix("*** Update File: ") {
            let mut destination = path.to_owned();
            if let Some(moved) = lines.get(i).and_then(|l| l.strip_prefix("*** Move to: ")) {
                destination = moved.into();
                i += 1;
            }
            let mut hunks = Vec::new();
            while i < lines.len() && !lines[i].starts_with("*** ") {
                let mut hunk = Hunk {
                    preserve_final_newline: true,
                    ..Default::default()
                };
                if lines[i] == "@@" {
                    i += 1;
                } else if let Some(anchor) = lines[i].strip_prefix("@@ ") {
                    hunk.anchor = Some(anchor.into());
                    i += 1;
                }
                while i < lines.len()
                    && !lines[i].starts_with("@@")
                    && !lines[i].starts_with("*** ")
                {
                    add_line(&mut hunk, lines[i])?;
                    i += 1;
                }
                if lines.get(i) == Some(&"*** End of File") {
                    hunk.eof = true;
                    i += 1;
                }
                ensure!(
                    !hunk.old.is_empty() || !hunk.new.is_empty(),
                    "Empty patch hunk"
                );
                hunks.push(hunk);
            }
            result.push(FilePatch {
                source: Some(path.into()),
                destination: Some(destination),
                hunks,
                added: None,
            });
        } else {
            bail!("Unrecognized patch header: {header}");
        }
    }
    Ok(result)
}
fn unified_path(line: &str, prefix: &str) -> Result<Option<String>> {
    let path = line
        .strip_prefix(prefix)
        .context("Missing unified diff file header")?
        .split('\t')
        .next()
        .unwrap_or("");
    if path == "/dev/null" {
        return Ok(None);
    }
    ensure!(
        !path.starts_with('"'),
        "Quoted Git paths are not supported; use a Begin Patch block with the literal path"
    );
    let path = path
        .strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .unwrap_or(path);
    ensure!(!path.is_empty(), "Empty patch path");
    Ok(Some(path.into()))
}
fn parse_unified(patch: &str) -> Result<Vec<FilePatch>> {
    let lines: Vec<_> = patch.lines().collect();
    let header = regex::Regex::new(r"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@(?:.*)$")?;
    let mut result = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].starts_with("diff --git ")
            || lines[i].starts_with("index ")
            || lines[i].is_empty()
        {
            i += 1;
            continue;
        }
        // File creation/deletion metadata is represented by /dev/null below.
        if lines[i].starts_with("new file mode 100644")
            || lines[i].starts_with("deleted file mode ")
        {
            i += 1;
            continue;
        }
        let source = unified_path(lines[i], "--- ")?;
        i += 1;
        let destination =
            unified_path(lines.get(i).context("Missing destination header")?, "+++ ")?;
        i += 1;
        let mut hunks = Vec::new();
        while i < lines.len() && lines[i].starts_with("@@ ") {
            let caps = header
                .captures(lines[i])
                .context("Invalid unified hunk header")?;
            let old_start = caps[1].parse::<usize>()?;
            let old_count = caps.get(2).map_or("1", |v| v.as_str()).parse::<usize>()?;
            let new_count = caps.get(4).map_or("1", |v| v.as_str()).parse::<usize>()?;
            let mut hunk = Hunk {
                line: Some(if old_count == 0 {
                    old_start
                } else {
                    old_start.saturating_sub(1)
                }),
                ..Default::default()
            };
            i += 1;
            let (mut removed, mut added) = (0, 0);
            let mut previous = ' ';
            while removed < old_count
                || added < new_count
                || lines.get(i).is_some_and(|l| l.starts_with("\\ No newline"))
            {
                let line = *lines.get(i).context("Truncated unified patch")?;
                if line == "\\ No newline at end of file" {
                    ensure!(previous != '\\', "Repeated newline marker");
                    if previous != '+' {
                        ensure!(hunk.old.ends_with('\n'), "Invalid newline marker");
                        hunk.old.pop();
                    }
                    if previous != '-' {
                        ensure!(hunk.new.ends_with('\n'), "Invalid newline marker");
                        hunk.new.pop();
                    }
                    previous = '\\';
                    i += 1;
                    continue;
                }
                let kind = line
                    .chars()
                    .next()
                    .context("Unified patch lines require a prefix")?;
                add_line(&mut hunk, line)?;
                if kind != '+' {
                    removed += 1;
                }
                if kind != '-' {
                    added += 1;
                }
                ensure!(
                    removed <= old_count && added <= new_count,
                    "Unified hunk line counts do not match"
                );
                previous = kind;
                i += 1;
            }
            hunks.push(hunk);
        }
        ensure!(!hunks.is_empty(), "Unified patch has no hunks");
        result.push(FilePatch {
            source,
            destination,
            hunks,
            added: None,
        });
    }
    Ok(result)
}
fn add_line(hunk: &mut Hunk, line: &str) -> Result<()> {
    let (kind, text) = line
        .split_at_checked(1)
        .context("Patch line requires a space, +, or - prefix")?;
    ensure!(matches!(kind, " " | "+" | "-"), "Invalid patch line prefix");
    if kind != "+" {
        hunk.old.push_str(text);
        hunk.old.push('\n');
    }
    if kind != "-" {
        hunk.new.push_str(text);
        hunk.new.push('\n');
    }
    Ok(())
}
fn apply_hunks(original: &str, hunks: &[Hunk]) -> Result<String> {
    let mut text = original.to_owned();
    let mut cursor = 0;
    let mut line_delta = 0isize;
    let crlf = original.contains("\r\n") && !original.replace("\r\n", "").contains('\n');
    for hunk in hunks {
        let mut old = if crlf {
            hunk.old.replace('\n', "\r\n")
        } else {
            hunk.old.clone()
        };
        let mut new = if crlf {
            hunk.new.replace('\n', "\r\n")
        } else {
            hunk.new.clone()
        };
        let mut start = cursor;
        if let Some(anchor) = &hunk.anchor {
            if let Some(found) = text[start..].find(anchor) {
                start += found + anchor.len();
                if text.as_bytes().get(start) == Some(&b'\r') {
                    start += 1;
                }
                if text.as_bytes().get(start) == Some(&b'\n') {
                    start += 1;
                }
            } else {
                bail!("Patch anchor not found");
            }
        }
        let preferred = hunk
            .line
            .and_then(|line| line.checked_add_signed(line_delta))
            .and_then(|line| line_offset(&text, line));
        let mut candidates = matches_at_lines(&text, &old, start, hunk.eof);
        if candidates.is_empty()
            && hunk.preserve_final_newline
            && !text.ends_with('\n')
            && old.ends_with('\n')
        {
            old.truncate(old.len() - if crlf { 2 } else { 1 });
            if new.ends_with('\n') {
                new.truncate(new.len() - if crlf { 2 } else { 1 });
            }
            candidates = matches_at_lines(&text, &old, start, true);
        }
        let position = if old.is_empty() {
            let position = preferred
                .or_else(|| text.is_empty().then_some(0))
                .context("Insertion requires surrounding context or a unified line number")?;
            ensure!(
                position >= start && position <= text.len(),
                "Invalid insertion position"
            );
            position
        } else if let Some(position) = preferred.filter(|p| {
            *p >= start
                && text[*p..].starts_with(&old)
                && (!hunk.eof || *p + old.len() == text.len())
        }) {
            position
        } else {
            ensure!(
                candidates.len() == 1,
                if candidates.is_empty() {
                    "Patch context was not found"
                } else {
                    "Patch context is ambiguous; include more surrounding lines"
                }
            );
            candidates[0]
        };
        text.replace_range(position..position + old.len(), &new);
        cursor = position + new.len();
        line_delta += new.bytes().filter(|b| *b == b'\n').count() as isize
            - old.bytes().filter(|b| *b == b'\n').count() as isize;
        ensure!(text.len() <= MAX_FILE_BYTES, "Patched file exceeds 4 MB");
    }
    Ok(text)
}
fn line_offset(text: &str, line: usize) -> Option<usize> {
    if line == 0 {
        return Some(0);
    }
    text.match_indices('\n').nth(line - 1).map(|(i, _)| i + 1)
}
fn matches_at_lines(text: &str, needle: &str, start: usize, eof: bool) -> Vec<usize> {
    if needle.is_empty() {
        return Vec::new();
    }
    text[start..]
        .match_indices(needle)
        .map(|(i, _)| i + start)
        .filter(|i| {
            (*i == 0 || text.as_bytes()[i - 1] == b'\n') && (!eof || i + needle.len() == text.len())
        })
        .take(3)
        .collect()
}
