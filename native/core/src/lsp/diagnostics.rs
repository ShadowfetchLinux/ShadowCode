//! Which diagnostics an edit introduced.
//!
//! Diagnostics are compared as multisets keyed by (message, code, source),
//! ignoring positions: an edit above an old error moves it without making it
//! new, while a second copy of the same error is new.
use serde_json::{json, Value};
use std::collections::HashMap;

pub const MAX_REPORTED: usize = 12;
const MAX_MESSAGE: usize = 300;

/// Errors (severity 1, or no severity) only.
pub fn is_error(diagnostic: &Value) -> bool {
    matches!(diagnostic["severity"].as_i64(), None | Some(1))
}

fn key(diagnostic: &Value) -> (String, String, String) {
    (
        diagnostic["message"]
            .as_str()
            .unwrap_or("")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "),
        match &diagnostic["code"] {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => String::new(),
        },
        diagnostic["source"].as_str().unwrap_or("").to_owned(),
    )
}

/// Errors in `after` that `before` does not account for.
pub fn new_errors<'a>(before: &[Value], after: &'a [Value]) -> Vec<&'a Value> {
    let mut remaining: HashMap<(String, String, String), usize> = HashMap::new();
    for diagnostic in before.iter().filter(|d| is_error(d)) {
        *remaining.entry(key(diagnostic)).or_default() += 1;
    }
    after
        .iter()
        .filter(|d| is_error(d))
        .filter(|diagnostic| match remaining.get_mut(&key(diagnostic)) {
            Some(count) if *count > 0 => {
                *count -= 1;
                false
            }
            _ => true,
        })
        .collect()
}

pub fn severity_name(diagnostic: &Value) -> &'static str {
    match diagnostic["severity"].as_i64() {
        Some(2) => "warning",
        Some(3) => "information",
        Some(4) => "hint",
        _ => "error",
    }
}

/// A compact, 1-based view of one diagnostic for the model.
pub fn render(path: &str, diagnostic: &Value, line_text: Option<&str>) -> Value {
    let line = diagnostic["range"]["start"]["line"].as_u64().unwrap_or(0) as usize;
    let utf16 = diagnostic["range"]["start"]["character"]
        .as_u64()
        .unwrap_or(0) as usize;
    let column = line_text
        .map(|text| super::client::char_column(text, utf16))
        .unwrap_or(utf16);
    let message = diagnostic["message"].as_str().unwrap_or("").trim();
    let mut out = json!({
        "path": path,
        "line": line + 1,
        "column": column + 1,
        "severity": severity_name(diagnostic),
        "message": crate::tools::truncate(message, MAX_MESSAGE),
    });
    if let Some(source) = diagnostic["source"].as_str() {
        out["source"] = json!(source);
    }
    match &diagnostic["code"] {
        Value::String(code) => out["code"] = json!(code),
        Value::Number(code) => out["code"] = json!(code),
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diag(line: u64, message: &str, severity: i64) -> Value {
        json!({
            "range": {"start": {"line": line, "character": 2}, "end": {"line": line, "character": 5}},
            "severity": severity,
            "message": message,
            "source": "fake",
        })
    }

    #[test]
    fn only_errors_the_edit_introduced_are_new() {
        let before = vec![
            diag(3, "undefined name 'x'", 1),
            diag(9, "unused import", 2),
        ];
        let after = vec![
            // The old error moved down two lines: not new.
            diag(5, "undefined name 'x'", 1),
            // A second copy of the same error: new.
            diag(12, "undefined name 'x'", 1),
            diag(7, "expected ';'", 1),
            // New warnings are not errors.
            diag(8, "shadowed variable", 2),
        ];
        let new: Vec<_> = new_errors(&before, &after)
            .into_iter()
            .map(|d| {
                (
                    d["range"]["start"]["line"].as_u64().unwrap(),
                    d["message"].as_str().unwrap(),
                )
            })
            .collect();
        assert_eq!(new, vec![(12, "undefined name 'x'"), (7, "expected ';'")]);
        assert!(new_errors(&after, &before).is_empty());
        assert_eq!(new_errors(&[], &after).len(), 3);
    }

    #[test]
    fn renders_one_based_positions() {
        let rendered = render("a.py", &diag(0, "  bad\n", 1), Some("  😀x"));
        assert_eq!(rendered["line"], 1);
        assert_eq!(rendered["column"], 3);
        assert_eq!(rendered["message"], "bad");
        assert_eq!(rendered["severity"], "error");
    }
}
