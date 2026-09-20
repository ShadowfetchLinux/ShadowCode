use serde_json::Value;
use std::collections::VecDeque;
use unicode_segmentation::UnicodeSegmentation;

pub(super) fn display(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if (c.is_control() && !matches!(c, '\n' | '\t'))
                || matches!(c,'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}')
            {
                format!("\\u{{{:x}}}", c as u32)
            } else if c == '\t' {
                "    ".into()
            } else {
                c.to_string()
            }
        })
        .collect()
}
fn bounded(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.into();
    }
    let mut end = max;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\n[Display shortened; saved history retains the full event]",
        &value[..end]
    )
}
#[derive(Clone, Debug)]
pub(super) struct Card {
    pub key: String,
    pub event_id: i64,
    pub title: String,
    pub body: String,
    pub tool: bool,
    pub failed: bool,
}
#[derive(Clone, Debug, Default)]
pub(super) struct Transcript {
    pub cards: VecDeque<Card>,
    pub cursor: i64,
    pub first: i64,
    pub trimmed: bool,
}
impl Transcript {
    pub fn note(&mut self, title: &str, text: &str) {
        self.put(
            Card {
                key: format!("note-{}", crate::id()),
                event_id: 0,
                title: title.into(),
                body: text.into(),
                tool: false,
                failed: false,
            },
            false,
        );
    }
    fn put(&mut self, value: Card, append: bool) {
        let Card {
            key,
            event_id,
            title,
            body,
            tool,
            failed,
        } = value;
        let body = display(&body);
        if let Some(card) = self.cards.iter_mut().find(|c| c.key == key) {
            card.title = display(&title);
            card.failed = failed;
            card.body = bounded(
                &if append {
                    format!("{}{}", card.body, body)
                } else {
                    body
                },
                24_000,
            );
        } else {
            self.cards.push_back(Card {
                key,
                event_id,
                title: display(&title),
                body: bounded(&body, 24_000),
                tool,
                failed,
            });
        }
        while self.cards.len() > 128
            || self
                .cards
                .iter()
                .map(|c| c.body.len() + c.title.len())
                .sum::<usize>()
                > 384_000
        {
            self.cards.pop_front();
            self.trimmed = true;
            if let Some(id) = self
                .cards
                .iter()
                .find_map(|c| (c.event_id > 0).then_some(c.event_id))
            {
                self.first = id;
            }
        }
    }
    pub fn ingest(&mut self, event: &Value) {
        let id = event["id"].as_i64().unwrap_or(0);
        if id <= self.cursor {
            return;
        }
        if self.first == 0 {
            self.first = id;
        }
        self.cursor = id;
        let p = &event["payload"];
        let task = event["task_id"].as_str().unwrap_or("");
        let kind = event["type"].as_str().unwrap_or("");
        let text = |name: &str| p[name].as_str().unwrap_or("");
        let mut title = String::new();
        let mut body = String::new();
        let mut tool = false;
        let mut failed = false;
        let mut append = false;
        let mut key = format!("{task}:{id}");
        match kind {
            "user.message" => {
                title = "You".into();
                body = text("text").into();
            }
            "model.stream" | "model.delta" => {
                key = format!("{task}:message:{}", text("message_id"));
                title = "ShadowCode".into();
                body = text("text").into();
                append = kind == "model.stream";
            }
            "model.stream_end" => {
                key = format!("{task}:message:{}", text("message_id"));
                if let Some(card) = self.cards.iter_mut().find(|c| c.key == key) {
                    card.title = "Interrupted response".into();
                }
                return;
            }
            "tool.started" => {
                key = format!("{task}:tool:{}", text("call_id"));
                title = format!("Running · {}", text("tool"));
                body = p["arguments"].to_string();
                tool = true;
            }
            "tool.completed" => {
                key = format!("{task}:tool:{}", text("call_id"));
                failed = p["success"] == false;
                title = format!(
                    "{} · {}",
                    if failed { "Failed" } else { "Done" },
                    text("tool")
                );
                body = serde_json::to_string_pretty(&p["output"]).unwrap_or_default();
                if !text("error").is_empty() {
                    body.push_str(text("error"));
                }
                tool = true;
            }
            "agent.completed" => {
                failed = p["success"] != true;
                title = if p["cancelled"] == true {
                    "Stopped"
                } else if failed {
                    "Task failed"
                } else {
                    "Task completed"
                }
                .into();
                body = format!(
                    "Usage: {}{}\nVerification: {}",
                    p["usage"],
                    if p["usage_is_estimated"] == true {
                        " (estimated)"
                    } else {
                        ""
                    },
                    p["verification"]["status"]
                        .as_str()
                        .unwrap_or("not recorded")
                );
                let summary = text("summary");
                if !self.cards.iter().any(|c| {
                    c.key.starts_with(&format!("{task}:message:")) && c.body == display(summary)
                }) {
                    body = format!("{summary}\n{body}");
                }
            }
            "routing.selected" | "routing.fallback" => {
                title = "Model".into();
                body = format!("{} · {}", text("model_name"), text("purpose"));
            }
            "workflow.selected" => {
                title = "Workflow".into();
                body = format!("{} · {} · {}", text("name"), text("path"), text("mode"));
            }
            "hook.started" | "hook.completed" => {
                key = format!("{task}:hook:{}", text("id"));
                title = format!("Hook · {} · {}", text("name"), text("status"));
                body = p.to_string();
                tool = true;
                failed = p["success"] == false;
            }
            "plan.updated" | "verification.summary" | "command.completed" | "context.compacted" => {
                title = kind.into();
                body = serde_json::to_string_pretty(p).unwrap_or_default();
                tool = true;
            }
            _ => {}
        }
        if !title.is_empty() {
            self.put(
                Card {
                    key,
                    event_id: id,
                    title,
                    body,
                    tool,
                    failed,
                },
                append,
            );
        }
    }
}
#[derive(Default)]
pub(super) struct Editor {
    pub text: String,
    pub cursor: usize,
}
impl Editor {
    pub fn insert(&mut self, value: &str) {
        let value: String = value
            .chars()
            .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
            .collect();
        if self.text.len() + value.len() <= 64_000 {
            self.text.insert_str(self.cursor, &value);
            self.cursor += value.len();
        }
    }
    pub fn left(&mut self) {
        self.cursor = self.text[..self.cursor]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(i, _)| i);
    }
    pub fn right(&mut self) {
        if let Some(g) = self.text[self.cursor..].graphemes(true).next() {
            self.cursor += g.len();
        }
    }
    pub fn backspace(&mut self) {
        let end = self.cursor;
        self.left();
        self.text.replace_range(self.cursor..end, "");
    }
    pub fn delete(&mut self) {
        let start = self.cursor;
        self.right();
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
    }
    pub fn replace(&mut self, text: String) {
        self.text = text;
        self.cursor = self.text.len();
    }
    pub fn take(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.text)
    }
    pub fn home(&mut self) {
        self.cursor = self.text[..self.cursor].rfind('\n').map_or(0, |i| i + 1);
    }
    pub fn end(&mut self) {
        self.cursor += self.text[self.cursor..]
            .find('\n')
            .unwrap_or(self.text.len() - self.cursor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn terminal_output_cannot_inject_controls_and_editing_keeps_graphemes() {
        assert_eq!(
            display("a\x1b]52;c;secret\x07\u{202e}"),
            "a\\u{1b}]52;c;secret\\u{7}\\u{202e}"
        );
        let mut e = Editor::default();
        e.insert("a👩‍💻e\u{301}界");
        e.left();
        e.backspace();
        assert_eq!(e.text, "a👩‍💻界");
        e.backspace();
        assert_eq!(e.text, "a界");
        e.home();
        e.delete();
        assert_eq!(e.text, "界");
        e.end();
        e.insert("\nline\x1b");
        assert_eq!(e.text, "界\nline");
        e.insert(&"x".repeat(64_001));
        assert_eq!(e.text, "界\nline");
    }
    #[test]
    fn replay_deduplicates_streams_and_bounds_noisy_history() {
        let mut t = Transcript::default();
        for (id, kind, text) in [
            (1, "model.stream", "hel"),
            (2, "model.stream", "lo"),
            (3, "model.delta", "hello"),
        ] {
            t.ingest(&json!({"id":id,"type":kind,"task_id":"a","payload":{"message_id":"m","text":text}}));
        }
        assert_eq!(t.cards.len(), 1);
        assert_eq!(t.cards[0].body, "hello");
        t.ingest(&json!({"id":2,"type":"model.stream","task_id":"a","payload":{"message_id":"m","text":"duplicate"}}));
        assert_eq!(t.cards[0].body, "hello");
        for id in 4..1004 {
            t.ingest(&json!({"id":id,"type":"tool.completed","task_id":"a","payload":{"call_id":id.to_string(),"tool":"read_file","success":true,"output":"界".repeat(10000)}}));
        }
        assert!(
            t.cards.len() <= 128
                && t.cards
                    .iter()
                    .map(|c| c.body.len() + c.title.len())
                    .sum::<usize>()
                    <= 384_000
        );
        assert!(t.trimmed);
        assert!(
            t.first > 4,
            "Older paging must include evicted events rather than skip them"
        );
    }
}
