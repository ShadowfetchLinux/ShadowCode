//! Parsers for the model lists printed by official vendor CLIs
//! (`cursor-agent --list-models`, `agy models`). Output is parsed as a list of
//! names; no pretend catalog is added. The live catalog is `catalog.rs`.

#[derive(Clone, Debug, PartialEq)]
pub struct DiscoveredModel {
    pub id: String,
    pub label: String,
    pub auto: bool,
}

pub fn parse_cursor_models(text: &str) -> Vec<DiscoveredModel> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.to_ascii_lowercase().starts_with("available models") {
            continue;
        }
        let (id, label) = match line.split_once(" - ") {
            Some((id, label)) => (id.trim(), label.trim()),
            None => continue,
        };
        if id.is_empty() || !seen.insert(id.to_owned()) {
            continue;
        }
        out.push(DiscoveredModel {
            id: id.to_owned(),
            label: if label.is_empty() {
                id.to_owned()
            } else {
                label.to_owned()
            },
            auto: id == "auto",
        });
    }
    out
}

pub fn parse_agy_models(text: &str) -> Vec<DiscoveredModel> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.to_ascii_lowercase().starts_with("fetching") {
            continue;
        }
        let (id, label) = match line.split_once('\t') {
            Some((id, label)) => (id.trim(), label.trim()),
            None => match line.split_once("  ") {
                Some((id, label)) => (id.trim(), label.trim()),
                None => continue,
            },
        };
        if id.is_empty() || id.contains(' ') || !seen.insert(id.to_owned()) {
            continue;
        }
        out.push(DiscoveredModel {
            id: id.to_owned(),
            label: if label.is_empty() {
                id.to_owned()
            } else {
                label.to_owned()
            },
            auto: false,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cursor_and_agy_lists_without_inventing() {
        let cursor = parse_cursor_models(
            "Available models\n\nauto - Auto (current, default)\ngpt-5.3-codex - Codex 5.3\n",
        );
        assert_eq!(cursor.len(), 2);
        assert!(cursor[0].auto);
        assert_eq!(cursor[1].id, "gpt-5.3-codex");
        let agy = parse_agy_models(
            "Fetching available models...\ngemini-3.8-flash-high\tGemini 3.8 Flash (High)\n",
        );
        assert_eq!(agy.len(), 1);
        assert_eq!(agy[0].id, "gemini-3.8-flash-high");
        assert!(parse_cursor_models("not a list").is_empty());
    }
}
