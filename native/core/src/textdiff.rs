//! Line diffs between two versions of a text file, for approval previews and
//! the per-task review (`service/review.rs`).
//!
//! Lines keep their line endings, so reverting one hunk rebuilds the file
//! byte for byte. The diff is Myers' O(ND) algorithm after trimming the
//! common prefix and suffix; past `MAX_EDITS` differences the changed middle
//! is reported as one replacement instead of spending more time and memory.
use serde_json::{json, Value};

/// Beyond this many inserted plus deleted lines the middle of the file is
/// shown as one replacement.
const MAX_EDITS: usize = 2000;
/// Unchanged lines shown around each change.
pub const CONTEXT: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Context,
    Added,
    Removed,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Context => "ctx",
            Kind::Added => "add",
            Kind::Removed => "del",
        }
    }
    fn prefix(self) -> char {
        match self {
            Kind::Context => ' ',
            Kind::Added => '+',
            Kind::Removed => '-',
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line<'a> {
    pub kind: Kind,
    /// The line as stored, with its line ending when it has one.
    pub raw: &'a str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk<'a> {
    /// 0-based first line of the hunk in the old and new text.
    pub old_index: usize,
    pub old_len: usize,
    pub new_index: usize,
    pub new_len: usize,
    pub lines: Vec<Line<'a>>,
}

impl Hunk<'_> {
    /// `@@ -a,b +c,d @@` as unified diffs write it (1-based; an empty side
    /// names the line before it).
    pub fn header(&self) -> String {
        let side = |index: usize, len: usize| {
            let start = if len == 0 { index } else { index + 1 };
            format!("{start},{len}")
        };
        format!(
            "@@ -{} +{} @@",
            side(self.old_index, self.old_len),
            side(self.new_index, self.new_len)
        )
    }
    /// A short stable name for this hunk: its header and lines hashed.
    pub fn id(&self) -> String {
        let mut text = self.header();
        for line in &self.lines {
            text.push(line.kind.prefix());
            text.push_str(line.raw);
            text.push('\u{0}');
        }
        crate::workspace::hash(text.as_bytes())[..16].to_owned()
    }
    pub fn added(&self) -> usize {
        self.lines.iter().filter(|l| l.kind == Kind::Added).count()
    }
    pub fn removed(&self) -> usize {
        self.lines
            .iter()
            .filter(|l| l.kind == Kind::Removed)
            .count()
    }
    /// `{id, header, lines: [{kind, text}]}`; `eol: false` marks a last line
    /// without a line ending.
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id(),
            "header": self.header(),
            "old_start": self.old_index + 1,
            "old_len": self.old_len,
            "new_start": self.new_index + 1,
            "new_len": self.new_len,
            "lines": self.lines.iter().map(|line| {
                let text = display(line.raw);
                if line.raw.ends_with('\n') {
                    json!({"kind": line.kind.label(), "text": text})
                } else {
                    json!({"kind": line.kind.label(), "text": text, "eol": false})
                }
            }).collect::<Vec<_>>(),
        })
    }
}

fn display(raw: &str) -> &str {
    raw.strip_suffix('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .unwrap_or(raw)
}

/// Split into lines, each keeping its `\n`.
pub fn lines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

enum Op {
    Equal(usize, usize),
    Delete(usize),
    Insert(usize),
}

/// The hunks that turn `old` into `new`.
pub fn hunks<'a>(old: &'a str, new: &'a str) -> Vec<Hunk<'a>> {
    let a = lines(old);
    let b = lines(new);
    group(&a, &b, &edit_script(&a, &b), CONTEXT)
}

fn edit_script(a: &[&str], b: &[&str]) -> Vec<Op> {
    let prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (ma, mb) = (&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix]);
    let mut ops: Vec<Op> = (0..prefix).map(|i| Op::Equal(i, i)).collect();
    match myers(ma, mb) {
        Some(middle) => ops.extend(middle.into_iter().map(|op| match op {
            Op::Equal(i, j) => Op::Equal(i + prefix, j + prefix),
            Op::Delete(i) => Op::Delete(i + prefix),
            Op::Insert(j) => Op::Insert(j + prefix),
        })),
        None => {
            ops.extend((0..ma.len()).map(|i| Op::Delete(i + prefix)));
            ops.extend((0..mb.len()).map(|j| Op::Insert(j + prefix)));
        }
    }
    let (ta, tb) = (a.len() - suffix, b.len() - suffix);
    ops.extend((0..suffix).map(|k| Op::Equal(ta + k, tb + k)));
    ops
}

/// Myers' greedy shortest edit script, keeping only the diagonals each step
/// can reach (O(D²) memory). `None` past `MAX_EDITS`.
fn myers(a: &[&str], b: &[&str]) -> Option<Vec<Op>> {
    let (n, m) = (a.len() as isize, b.len() as isize);
    if n == 0 || m == 0 {
        let mut ops: Vec<Op> = (0..a.len()).map(Op::Delete).collect();
        ops.extend((0..b.len()).map(Op::Insert));
        return Some(ops);
    }
    let max = (n + m) as usize;
    let limit = max.min(MAX_EDITS) as isize;
    let offset = max as isize + 1;
    let mut v = vec![0isize; 2 * max + 3];
    let at = |k: isize| (k + offset) as usize;
    // trace[d] holds v[-d..=d] as it was before step d.
    let mut trace: Vec<Vec<isize>> = Vec::new();
    let mut found = None;
    'outer: for d in 0..=limit {
        trace.push(v[at(-d)..=at(d)].to_vec());
        let mut k = -d;
        while k <= d {
            let mut x = if k == -d || (k != d && v[at(k - 1)] < v[at(k + 1)]) {
                v[at(k + 1)]
            } else {
                v[at(k - 1)] + 1
            };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[at(k)] = x;
            if x >= n && y >= m {
                found = Some(d);
                break 'outer;
            }
            k += 2;
        }
    }
    let depth = found?;
    let mut ops = Vec::new();
    let (mut x, mut y) = (n, m);
    for d in (1..=depth).rev() {
        let saved = &trace[d as usize];
        let get = |k: isize| saved[(k + d) as usize];
        let k = x - y;
        let prev_k = if k == -d || (k != d && get(k - 1) < get(k + 1)) {
            k + 1
        } else {
            k - 1
        };
        let prev_x = get(prev_k);
        let prev_y = prev_x - prev_k;
        while x > prev_x && y > prev_y {
            x -= 1;
            y -= 1;
            ops.push(Op::Equal(x as usize, y as usize));
        }
        if x == prev_x {
            y -= 1;
            ops.push(Op::Insert(y as usize));
        } else {
            x -= 1;
            ops.push(Op::Delete(x as usize));
        }
    }
    while x > 0 && y > 0 {
        x -= 1;
        y -= 1;
        ops.push(Op::Equal(x as usize, y as usize));
    }
    ops.reverse();
    Some(ops)
}

fn group<'a>(a: &[&'a str], b: &[&'a str], ops: &[Op], context: usize) -> Vec<Hunk<'a>> {
    let changes: Vec<usize> = ops
        .iter()
        .enumerate()
        .filter(|(_, op)| !matches!(op, Op::Equal(..)))
        .map(|(i, _)| i)
        .collect();
    let mut hunks = Vec::new();
    let mut index = 0;
    while index < changes.len() {
        // Extend the group while the unchanged run between changes is short
        // enough that their context would touch.
        let first = changes[index];
        let mut last = first;
        index += 1;
        while index < changes.len() && changes[index] - last <= 2 * context + 1 {
            last = changes[index];
            index += 1;
        }
        let start = first.saturating_sub(context);
        let end = (last + context + 1).min(ops.len());
        // Where the hunk begins in each text: the first op's position, or for
        // a hunk that starts with an insertion/deletion, the other side's
        // position at that point.
        let (mut old_index, mut new_index) = position(ops, start, a.len(), b.len());
        let mut lines = Vec::new();
        let (mut old_len, mut new_len) = (0, 0);
        for op in &ops[start..end] {
            match *op {
                Op::Equal(i, _) => {
                    lines.push(Line {
                        kind: Kind::Context,
                        raw: a[i],
                    });
                    old_len += 1;
                    new_len += 1;
                }
                Op::Delete(i) => {
                    lines.push(Line {
                        kind: Kind::Removed,
                        raw: a[i],
                    });
                    old_len += 1;
                }
                Op::Insert(j) => {
                    lines.push(Line {
                        kind: Kind::Added,
                        raw: b[j],
                    });
                    new_len += 1;
                }
            }
        }
        old_index = old_index.min(a.len());
        new_index = new_index.min(b.len());
        hunks.push(Hunk {
            old_index,
            old_len,
            new_index,
            new_len,
            lines,
        });
    }
    hunks
}

/// The old and new line numbers (0-based) at op `index`.
fn position(ops: &[Op], index: usize, old_total: usize, new_total: usize) -> (usize, usize) {
    let (mut old, mut new) = (0, 0);
    for op in &ops[..index] {
        match op {
            Op::Equal(..) => {
                old += 1;
                new += 1;
            }
            Op::Delete(_) => old += 1,
            Op::Insert(_) => new += 1,
        }
    }
    (old.min(old_total), new.min(new_total))
}

/// `new` with one hunk (from `hunks(old, new)`) put back as it was in `old`.
pub fn revert(new: &str, hunk: &Hunk<'_>) -> Option<String> {
    let current = lines(new);
    let end = hunk.new_index.checked_add(hunk.new_len)?;
    if end > current.len() {
        return None;
    }
    let expected: Vec<&str> = hunk
        .lines
        .iter()
        .filter(|l| l.kind != Kind::Removed)
        .map(|l| l.raw)
        .collect();
    if current[hunk.new_index..end] != expected[..] {
        return None;
    }
    let mut out = String::with_capacity(new.len());
    for line in &current[..hunk.new_index] {
        out.push_str(line);
    }
    for line in hunk.lines.iter().filter(|l| l.kind != Kind::Added) {
        out.push_str(line.raw);
    }
    for line in &current[end..] {
        out.push_str(line);
    }
    Some(out)
}

/// A unified diff body (hunks only, no file headers), at most `max_lines`
/// lines. Returns the text, whether it was cut, and the +/- counts.
pub fn unified(old: &str, new: &str, max_lines: usize) -> (String, bool, usize, usize) {
    let hunks = hunks(old, new);
    let mut out = String::new();
    let mut shown = 0;
    let mut truncated = false;
    let (mut added, mut removed) = (0, 0);
    for hunk in &hunks {
        added += hunk.added();
        removed += hunk.removed();
        if truncated {
            continue;
        }
        if shown >= max_lines {
            truncated = true;
            continue;
        }
        out.push_str(&hunk.header());
        out.push('\n');
        shown += 1;
        for line in &hunk.lines {
            if shown >= max_lines {
                truncated = true;
                break;
            }
            out.push(line.kind.prefix());
            out.push_str(display(line.raw));
            out.push('\n');
            shown += 1;
        }
    }
    (out, truncated, added, removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply_all_reverts(old: &str, new: &str) -> String {
        // Reverting hunks from the last one keeps earlier positions valid.
        let mut text = new.to_owned();
        let owned_new = new.to_owned();
        let hunks = hunks(old, &owned_new);
        for hunk in hunks.iter().rev() {
            text = revert(&text, hunk).expect("hunk applies");
        }
        text
    }

    #[test]
    fn identical_texts_have_no_hunks() {
        assert!(hunks("a\nb\n", "a\nb\n").is_empty());
        assert!(hunks("", "").is_empty());
    }

    #[test]
    fn one_line_change_has_context_and_header() {
        let old = "1\n2\n3\n4\n5\n6\n7\n8\n";
        let new = "1\n2\n3\n4\nfive\n6\n7\n8\n";
        let hunks = hunks(old, new);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].header(), "@@ -2,7 +2,7 @@");
        assert_eq!(hunks[0].added(), 1);
        assert_eq!(hunks[0].removed(), 1);
        let json = hunks[0].to_json();
        assert_eq!(json["lines"][3], json!({"kind":"del","text":"5"}));
        assert_eq!(json["lines"][4], json!({"kind":"add","text":"five"}));
    }

    #[test]
    fn distant_changes_are_separate_hunks_and_revert_independently() {
        let old: String = (1..=30).map(|i| format!("line {i}\n")).collect();
        let new = old
            .replace("line 3\n", "line three\n")
            .replace("line 25\n", "line 25\nextra\n");
        let found = hunks(&old, &new);
        assert_eq!(found.len(), 2);
        // Undo only the second change.
        let partly = revert(&new, &found[1]).unwrap();
        assert!(partly.contains("line three\n"));
        assert!(!partly.contains("extra"));
        // Then the remaining change, recomputed against the new text.
        let rest = hunks(&old, &partly);
        assert_eq!(rest.len(), 1);
        assert_eq!(revert(&partly, &rest[0]).unwrap(), old);
        assert_eq!(apply_all_reverts(&old, &new), old);
    }

    #[test]
    fn new_and_emptied_files() {
        let added = hunks("", "a\nb\n");
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].header(), "@@ -0,0 +1,2 @@");
        assert_eq!(revert("a\nb\n", &added[0]).unwrap(), "");
        let emptied = hunks("a\nb\n", "");
        assert_eq!(emptied[0].header(), "@@ -1,2 +0,0 @@");
        assert_eq!(revert("", &emptied[0]).unwrap(), "a\nb\n");
    }

    #[test]
    fn missing_final_newline_is_kept_exactly() {
        let old = "a\nb";
        let new = "a\nb\nc";
        let found = hunks(old, new);
        assert_eq!(found.len(), 1);
        let json = found[0].to_json();
        assert!(json["lines"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l["eol"] == false));
        assert_eq!(apply_all_reverts(old, new), old);
    }

    #[test]
    fn large_rewrites_fall_back_to_one_replacement() {
        let old: String = (0..3000).map(|i| format!("old {i}\n")).collect();
        let new: String = (0..3000).map(|i| format!("new {i}\n")).collect();
        let found = hunks(&old, &new);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].removed(), 3000);
        assert_eq!(found[0].added(), 3000);
        assert_eq!(apply_all_reverts(&old, &new), old);
    }

    #[test]
    fn stale_hunks_do_not_apply() {
        let found = hunks("a\nb\nc\n", "a\nB\nc\n");
        assert!(revert("a\nX\nc\n", &found[0]).is_none());
    }

    #[test]
    fn unified_text_is_capped() {
        let old: String = (0..100).map(|i| format!("{i}\n")).collect();
        let new: String = (0..100).map(|i| format!("{}\n", i * 2)).collect();
        let (text, truncated, added, removed) = unified(&old, &new, 10);
        assert!(truncated);
        assert_eq!(text.lines().count(), 10);
        assert!(added > 0 && removed > 0);
        let (full, cut, _, _) = unified("a\n", "b\n", 100);
        assert!(!cut);
        assert_eq!(full, "@@ -1,1 +1,1 @@\n-a\n+b\n");
    }

    #[test]
    fn random_edits_round_trip() {
        // A small deterministic generator: every edit script must rebuild the
        // old text exactly when all hunks are reverted.
        let mut seed = 7u64;
        let mut next = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            (seed >> 33) as usize
        };
        for _ in 0..200 {
            let old: Vec<String> = (0..next() % 40).map(|i| format!("{}\n", i % 7)).collect();
            let mut new = old.clone();
            for _ in 0..next() % 6 {
                let at = if new.is_empty() {
                    0
                } else {
                    next() % new.len()
                };
                match next() % 3 {
                    0 if !new.is_empty() => {
                        new.remove(at);
                    }
                    1 => new.insert(at, format!("x{}\n", next() % 5)),
                    _ if !new.is_empty() => new[at] = format!("y{}\n", next() % 5),
                    _ => {}
                }
            }
            let (old, new) = (old.concat(), new.concat());
            assert_eq!(apply_all_reverts(&old, &new), old, "{old:?} -> {new:?}");
        }
    }
}
