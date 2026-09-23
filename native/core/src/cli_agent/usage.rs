//! Official-runtime usage snapshots. Unknown values stay unknown.
//! Never invent remaining percents, message counts, or "unlimited".
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UsageSnapshot {
    pub state: String,
    pub label: String,
    pub remaining_percent: Option<f64>,
    pub remaining_quantity: Option<u64>,
    pub window: Option<String>,
    pub reset: Option<String>,
    pub shared_pool: Option<String>,
    pub credits: Option<String>,
    pub last_refresh: Option<f64>,
    pub provider_usage_url: Option<String>,
}

impl UsageSnapshot {
    pub fn unavailable(provider: &str) -> Self {
        Self {
            state: "unavailable".into(),
            label: format!("Usage unavailable · Open {provider} usage"),
            remaining_percent: None,
            remaining_quantity: None,
            window: None,
            reset: None,
            shared_pool: None,
            credits: None,
            last_refresh: None,
            provider_usage_url: usage_url(provider),
        }
    }
    pub fn stale(provider: &str, last_refresh: f64) -> Self {
        let mut snap = Self::unavailable(provider);
        snap.state = "stale".into();
        snap.label = format!("Last checked {}", format_age(last_refresh));
        snap.last_refresh = Some(last_refresh);
        snap
    }
    pub fn local() -> Self {
        Self {
            state: "local".into(),
            label: "Runs on this computer · No subscription quota".into(),
            remaining_percent: None,
            remaining_quantity: None,
            window: None,
            reset: None,
            shared_pool: None,
            credits: None,
            last_refresh: None,
            provider_usage_url: None,
        }
    }
    pub fn shared_pool(provider: &str, pool: &str, remaining_percent: f64, last_refresh: f64) -> Self {
        Self {
            state: "ok".into(),
            label: format!("{remaining_percent:.0}% remaining · shared {pool}"),
            remaining_percent: Some(remaining_percent),
            remaining_quantity: None,
            window: None,
            reset: None,
            shared_pool: Some(pool.to_owned()),
            credits: None,
            last_refresh: Some(last_refresh),
            provider_usage_url: usage_url(provider),
        }
    }
    pub fn from_official(value: &Value, provider: &str, now: f64) -> Self {
        if value.get("remaining_percent").and_then(Value::as_f64).is_none()
            && value.get("remaining_quantity").and_then(Value::as_u64).is_none()
            && value.get("credits").and_then(Value::as_str).is_none()
        {
            let mut snap = Self::unavailable(provider);
            snap.last_refresh = Some(now);
            return snap;
        }
        let remaining_percent = value.get("remaining_percent").and_then(Value::as_f64);
        let remaining_quantity = value.get("remaining_quantity").and_then(Value::as_u64);
        let shared = value.get("shared_pool").and_then(Value::as_str);
        let mut label = match (remaining_percent, remaining_quantity) {
            (Some(p), _) => format!("{p:.0}% remaining"),
            (_, Some(q)) => format!("{q} remaining"),
            _ => "Usage reported".into(),
        };
        if let Some(pool) = shared {
            label.push_str(" · shared ");
            label.push_str(pool);
        }
        Self {
            state: "ok".into(),
            label,
            remaining_percent,
            remaining_quantity,
            window: value
                .get("window")
                .and_then(Value::as_str)
                .map(str::to_owned),
            reset: value.get("reset").and_then(Value::as_str).map(str::to_owned),
            shared_pool: shared.map(str::to_owned),
            credits: value
                .get("credits")
                .and_then(Value::as_str)
                .map(str::to_owned),
            last_refresh: Some(now),
            provider_usage_url: usage_url(provider),
        }
    }
    pub fn to_json(&self) -> Value {
        json!(self)
    }
}

fn usage_url(provider: &str) -> Option<String> {
    Some(
        match provider {
            "Codex" | "cli:codex" => "https://chatgpt.com/#settings",
            "Claude Code" | "cli:claude" => "https://claude.ai/settings/usage",
            "Cursor" | "cli:cursor" => "https://cursor.com/dashboard",
            "Antigravity" | "cli:antigravity" => "https://antigravity.google/docs/cli/commands/usage/",
            _ => return None,
        }
        .into(),
    )
}

fn format_age(ts: f64) -> String {
    let now = crate::now();
    let delta = (now - ts).max(0.0);
    if delta < 60.0 {
        "just now".into()
    } else if delta < 3600.0 {
        format!("{}m ago", (delta / 60.0) as u64)
    } else {
        format!("{}h ago", (delta / 3600.0) as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_usage_is_not_a_percent() {
        let snap = UsageSnapshot::unavailable("Cursor");
        assert_eq!(snap.state, "unavailable");
        assert!(snap.remaining_percent.is_none());
        assert!(snap.label.contains("Usage unavailable"));
        assert!(!snap.label.contains("72%"));
        assert!(!snap.label.contains("unlimited"));
    }

    #[test]
    fn stale_and_shared_are_explicit() {
        let stale = UsageSnapshot::stale("Codex", crate::now() - 120.0);
        assert_eq!(stale.state, "stale");
        assert!(stale.label.starts_with("Last checked"));
        let shared = UsageSnapshot::shared_pool("Cursor", "account", 40.0, crate::now());
        assert_eq!(shared.shared_pool.as_deref(), Some("account"));
        assert!(shared.label.contains("shared"));
        let empty = UsageSnapshot::from_official(&json!({"tier":"Free"}), "Cursor", crate::now());
        assert_eq!(empty.state, "unavailable");
        assert!(empty.remaining_percent.is_none());
    }

    #[test]
    fn local_rows_have_no_quota() {
        let snap = UsageSnapshot::local();
        assert_eq!(snap.state, "local");
        assert!(snap.label.contains("No subscription quota"));
    }
}
