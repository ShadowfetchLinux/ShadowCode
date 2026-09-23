//! Narrow secret redaction before file contents or tool output enter model context.
//! Fake fixture strings only — never commit real secrets.
use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

const PLACEHOLDER: &str = "[redacted secret]";

fn patterns() -> &'static [Regex] {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        let sources = [
            r"(?i)-----BEGIN (?:RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----",
            r"(?i)\bgh[pousr]_[A-Za-z0-9_]{20,}",
            r"(?i)\bxox[baprs]-[A-Za-z0-9-]{10,}",
            r"(?i)\bAKIA[0-9A-Z]{16}\b",
            r"(?i)\bASIA[0-9A-Z]{16}\b",
            r"(?i)\bsk-(?:live|test|proj)?[A-Za-z0-9_-]{16,}",
            r"(?i)\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}",
            r"(?i)\b(?:api[_-]?key|secret[_-]?key|access[_-]?token|auth[_-]?token)\s*[:=]\s*\S{16,}",
            r"(?i)\bBearer\s+[A-Za-z0-9._~+/=-]{20,}",
        ];
        sources
            .into_iter()
            .map(|p| Regex::new(p).expect("redaction regex"))
            .collect()
    })
}

/// High-entropy token heuristic: long base64/hex-like runs outside common words.
fn high_entropy_tokens(text: &str) -> Vec<(usize, usize)> {
    static ENTROPY: OnceLock<Regex> = OnceLock::new();
    let re = ENTROPY.get_or_init(|| Regex::new(r"[A-Za-z0-9+/_=-]{32,}").expect("entropy regex"));
    re.find_iter(text)
        .filter(|m| {
            let s = m.as_str();
            if s.chars().all(|c| c.is_ascii_digit()) {
                return false;
            }
            let classes = [
                s.bytes().any(|b| b.is_ascii_lowercase()),
                s.bytes().any(|b| b.is_ascii_uppercase()),
                s.bytes().any(|b| b.is_ascii_digit()),
                s.bytes()
                    .any(|b| matches!(b, b'+' | b'/' | b'_' | b'=' | b'-')),
            ]
            .into_iter()
            .filter(|v| *v)
            .count();
            classes >= 3 && shannon(s) >= 3.5
        })
        .map(|m| (m.start(), m.end()))
        .collect()
}

fn shannon(s: &str) -> f64 {
    let mut counts = [0u32; 256];
    for b in s.bytes() {
        counts[b as usize] += 1;
    }
    let len = s.len() as f64;
    counts
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = f64::from(c) / len;
            -p * p.log2()
        })
        .sum()
}

pub fn is_secret_path(path: &str) -> bool {
    let name = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_ascii_lowercase();
    // Committed templates document variable names without values; refusing
    // them would hide the one file a model may legitimately need to read.
    if matches!(
        name.as_str(),
        ".env.example" | ".env.sample" | ".env.template" | ".env.dist" | ".env.defaults"
    ) {
        return false;
    }
    matches!(
        name.as_str(),
        ".env"
            | ".env.local"
            | ".env.development"
            | ".env.production"
            | ".env.test"
            | "secrets.env"
            | ".secrets"
            | "credentials.json"
            | "service-account.json"
    ) || name.starts_with(".env.")
        || name.ends_with(".pem")
        || name.ends_with(".p12")
}

pub struct Redaction {
    pub text: String,
    pub redacted: bool,
    pub count: usize,
}

pub fn redact_text(input: &str) -> Redaction {
    let mut text = input.to_owned();
    let mut count = 0usize;
    for pattern in patterns() {
        let found = pattern.find_iter(&text).count();
        if found == 0 {
            continue;
        }
        count += found;
        text = pattern.replace_all(&text, PLACEHOLDER).into_owned();
    }
    let mut spans = high_entropy_tokens(&text);
    spans.sort_by_key(|(start, _)| std::cmp::Reverse(*start));
    for (start, end) in spans {
        if text[start..end].contains("redacted") {
            continue;
        }
        text.replace_range(start..end, PLACEHOLDER);
        count += 1;
    }
    Redaction {
        redacted: count > 0,
        count,
        text,
    }
}

pub fn redact_value(value: &mut Value) -> usize {
    match value {
        Value::String(s) => {
            let result = redact_text(s);
            if result.redacted {
                *s = result.text;
                result.count
            } else {
                0
            }
        }
        Value::Array(items) => items.iter_mut().map(redact_value).sum(),
        Value::Object(map) => map.values_mut().map(redact_value).sum(),
        _ => 0,
    }
}

pub fn secret_file_refusal(path: &str) -> Value {
    serde_json::json!({
        "ok": false,
        "path": path,
        "redacted": true,
        "error": format!(
            "Refusing to send {path} to the model. Secret files (.env, secrets.env, credential JSON, private keys) are blocked; summarize keys present without values if the user needs structure."
        ),
        "note": "Raw secret file contents are never included in model context."
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_common_fixture_patterns() {
        // Fixture strings are assembled so scanners do not treat this file as
        // containing live credentials. Patterns still match production regexes.
        let jwt = format!(
            "Authorization: Bearer {}.{}.{}",
            "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9",
            "eyJzdWIiOiIxMjM0In0",
            "signaturepad_fixture_only"
        );
        let aws_key = format!("{}{}", "AKIA", "IOSFODNN7EXAMPLE");
        let slack = format!("{}-{}-{}", "xoxb", "000000000000", "fixturetoken0000");
        let github = format!("{}{}", "ghp_", "abcdefghijklmnopqrstuvwxyz012345");
        let pem = format!(
            "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----",
            "MIIEvQIBADANBgkqhkiG9w0BAQEFAASC"
        );
        let openai = format!("{}{}", "sk-test", "abcdefghijklmnopqrstuvwxyz0123");
        let samples = [
            jwt,
            format!("AWS_ACCESS_KEY_ID={aws_key}"),
            format!("slack={slack}"),
            format!("github={github}"),
            pem,
            format!("openai={openai}"),
        ];
        for sample in samples {
            let result = redact_text(&sample);
            assert!(result.redacted, "expected redaction in {sample}");
            assert!(result.text.contains(PLACEHOLDER));
        }
        let aws = redact_text(&format!("key={aws_key}"));
        assert!(!aws.text.contains(&aws_key));
        assert!(aws.text.contains(PLACEHOLDER));
    }

    #[test]
    fn redacts_anthropic_and_stripe_fixtures() {
        let anthropic = format!(
            "{}{}",
            "sk-ant-api03-", "abcdefghijklmnopqrstuvwxyz0123456789ABCDEF"
        );
        let stripe = format!("{}{}", "sk_live_", "abcdefghijklmnopqrstuvwxyz012345");
        let npm = format!("{}{}", "npm_", "abcdefghijklmnopqrstuvwxyz0123456789");
        for sample in [anthropic, stripe, npm] {
            let result = redact_text(&sample);
            assert!(result.redacted, "expected redaction in {sample}");
            assert!(result.text.contains(PLACEHOLDER));
            assert!(!result.text.contains("abcdefghijklmnopqrstuvwxyz"));
        }
    }

    #[test]
    fn secret_paths_are_detected() {
        assert!(is_secret_path(".env"));
        assert!(is_secret_path("app/.env.local"));
        assert!(is_secret_path("secrets.env"));
        assert!(is_secret_path("deploy/key.pem"));
        assert!(is_secret_path(".env.production"));
        // Templates with placeholder names are readable; values in them still
        // pass through redact_text like any other content.
        assert!(!is_secret_path(".env.example"));
        assert!(!is_secret_path("app/.env.sample"));
        assert!(!is_secret_path(".env.template"));
        assert!(!is_secret_path("src/config.rs"));
        assert!(!is_secret_path("README.md"));
    }

    #[test]
    fn plain_code_is_mostly_untouched() {
        let code = "fn main() { let x = 42; }";
        let result = redact_text(code);
        assert!(!result.redacted);
        assert_eq!(result.text, code);
    }
}
