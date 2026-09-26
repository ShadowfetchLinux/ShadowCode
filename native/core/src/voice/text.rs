//! Turn a raw transcript into text for the composer: drop whisper's
//! non-speech markers and, when enabled, apply spoken commands.
use regex::Regex;
use std::sync::LazyLock;

/// `[BLANK_AUDIO]`, `[Music]`, and the usual non-speech words in `( )` or
/// `* *` (`(silence)`, `*coughs*`); other parentheses are the user's words.
static MARKERS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\[[^\]]{1,40}\]|[(*][ \t]*(silence|inaudible|no speech|blank audio|(upbeat |soft |background )?music|laugh(s|ing|ter)?|applause|cough(s|ing)?|sigh(s|ing)?|(background )?noise|static|beep(s|ing)?|clears throat|breathing)[ \t]*[)*]",
    )
    .unwrap()
});
static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t]+").unwrap());
/// "… done. New line. Next …" / "new paragraph", with the punctuation whisper
/// tends to put around them (a full stop before the command stays: it ends
/// the sentence the user spoke).
static COMMANDS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)[ \t]*[,;:]?[ \t]*\b(new[ -]?line|new paragraph)\b[,.;:!]?[ \t]*").unwrap()
});

/// The text whisper heard, without markers or runs of spaces.
pub fn clean(raw: &str) -> String {
    let stripped = MARKERS.replace_all(raw, " ");
    let lines: Vec<String> = stripped
        .lines()
        .map(|line| SPACES.replace_all(line.trim(), " ").into_owned())
        .collect();
    lines.join(" ").trim().to_owned()
}

/// "new line" → a line break, "new paragraph" → a blank line.
pub fn apply_commands(text: &str) -> String {
    let replaced = COMMANDS.replace_all(text, |caps: &regex::Captures| {
        if caps[1].to_ascii_lowercase().contains("paragraph") {
            "\n\n".to_owned()
        } else {
            "\n".to_owned()
        }
    });
    // A command right after a sentence keeps that sentence's full stop; the
    // next line starts with a capital letter the way speech-to-text wrote it.
    let mut out = String::with_capacity(replaced.len());
    let mut capitalize = false;
    for ch in replaced.chars() {
        if ch == '\n' {
            capitalize = true;
            out.push(ch);
        } else if capitalize && ch.is_alphabetic() {
            out.extend(ch.to_uppercase());
            capitalize = false;
        } else {
            if !ch.is_whitespace() {
                capitalize = false;
            }
            out.push(ch);
        }
    }
    out.trim_matches(|c: char| c == ' ' || c == '\t').to_owned()
}

/// `clean`, then commands if enabled.
pub fn finish(raw: &str, commands: bool) -> String {
    let text = clean(raw);
    if commands {
        apply_commands(&text)
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_drops_markers_and_spaces() {
        assert_eq!(clean(" [BLANK_AUDIO] "), "");
        assert_eq!(clean("(silence)"), "");
        assert_eq!(
            clean(" Hello  there. [Music]\n How are *coughs* you?"),
            "Hello there. How are you?"
        );
        assert_eq!(clean("Fix the parser"), "Fix the parser");
        assert_eq!(clean("Call foo (the helper)"), "Call foo (the helper)");
    }

    #[test]
    fn commands_make_line_breaks() {
        assert_eq!(
            apply_commands("First item. New line. second item."),
            "First item.\nSecond item."
        );
        assert_eq!(
            apply_commands("Title, new paragraph, body text"),
            "Title\n\nBody text"
        );
        assert_eq!(apply_commands("one newline two"), "one\nTwo");
        assert_eq!(apply_commands("a new-line b"), "a\nB");
        // Only whole words.
        assert_eq!(apply_commands("renew lines here"), "renew lines here");
        assert_eq!(finish("Hi. New line. [BLANK_AUDIO]", true), "Hi.\n");
        assert_eq!(
            finish("Say new line literally", false),
            "Say new line literally"
        );
    }
}
