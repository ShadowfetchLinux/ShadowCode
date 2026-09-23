//! Unified execution-target catalog. Rows are keyed by stable IDs
//! (`provider`, `account`, `model`, `route`), never by display name.
use super::{usage::UsageSnapshot, CliAgentsConfig, Vendor};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const GROUP_SUBSCRIPTIONS: &str = "subscriptions";
pub const GROUP_LOCAL: &str = "local";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Ready,
    SignIn,
    SetupRequired,
    Unavailable,
}

impl Availability {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::SignIn => "Sign in",
            Self::SetupRequired => "Setup required",
            Self::Unavailable => "Unavailable",
        }
    }
    pub fn from_doctor(state: &str) -> Self {
        match state {
            "ready" => Self::Ready,
            "not_logged_in" => Self::SignIn,
            "not_installed" | "disabled" => Self::SetupRequired,
            _ => Self::Unavailable,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PickerTarget {
    pub id: String,
    pub provider: String,
    pub account: String,
    pub model: String,
    pub route: String,
    pub group: String,
    pub name: String,
    pub subtitle: String,
    pub inference: String,
    pub availability: Availability,
    pub availability_label: String,
    pub reason: String,
    pub featured: bool,
    pub vision: bool,
    pub tools: bool,
    pub usage: UsageSnapshot,
}

impl PickerTarget {
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "provider": self.provider,
            "account": self.account,
            "model": self.model,
            "route": self.route,
            "group": self.group,
            "name": self.name,
            "subtitle": self.subtitle,
            "inference": self.inference,
            "availability": self.availability,
            "availability_label": self.availability_label,
            "reason": self.reason,
            "featured": self.featured,
            "vision": self.vision,
            "tools": self.tools,
            "usage": self.usage,
        })
    }
}

pub fn target_id(vendor: Vendor, model: &str) -> String {
    let model = model.trim();
    if model.is_empty() || model == "default" {
        vendor.provider()
    } else {
        format!("{}:{model}", vendor.provider())
    }
}

pub fn vendor_target(
    vendor: Vendor,
    model: &str,
    model_label: &str,
    availability: Availability,
    reason: &str,
    usage: UsageSnapshot,
    vision: bool,
) -> PickerTarget {
    let model = if model.trim().is_empty() {
        "default"
    } else {
        model.trim()
    };
    let auto = model == "default" || model == "auto";
    let name = if auto {
        format!("{} · Auto", vendor.product_label())
    } else {
        format!("{} · {model_label}", vendor.product_label())
    };
    PickerTarget {
        id: target_id(vendor, model),
        provider: vendor.provider(),
        account: format!("account:{}", vendor.id()),
        model: model.to_owned(),
        route: "vendor_cli".into(),
        group: GROUP_SUBSCRIPTIONS.into(),
        name,
        subtitle: "Cloud · subscription".into(),
        inference: "cloud".into(),
        availability,
        availability_label: availability.label().into(),
        reason: reason.to_owned(),
        featured: vendor.featured(),
        vision,
        tools: true,
        usage,
    }
}

pub fn local_target(
    id: &str,
    provider: &str,
    model: &str,
    name: &str,
    availability: Availability,
    reason: &str,
    vision: bool,
    tools: bool,
) -> PickerTarget {
    PickerTarget {
        id: id.to_owned(),
        provider: provider.to_owned(),
        account: "this-computer".into(),
        model: model.to_owned(),
        route: if provider == "llamacpp" {
            "local_llamacpp".into()
        } else {
            "local_compat".into()
        },
        group: GROUP_LOCAL.into(),
        name: format!("{name} · This computer"),
        subtitle: "Runs on this computer · No subscription quota".into(),
        inference: "local".into(),
        availability,
        availability_label: availability.label().into(),
        reason: reason.to_owned(),
        featured: true,
        vision,
        tools,
        usage: UsageSnapshot::local(),
    }
}

/// Fallback subscription rows from Doctor only (no pretend model catalog).
pub fn doctor_rows(config: &CliAgentsConfig, doctor: &Value) -> Vec<PickerTarget> {
    let mut rows = Vec::new();
    for vendor in Vendor::ALL {
        if !config.vendor_enabled(vendor) {
            continue;
        }
        let state = doctor
            .get(vendor.id())
            .and_then(|v| v["state"].as_str())
            .unwrap_or("unavailable");
        let detail = doctor
            .get(vendor.id())
            .and_then(|v| v["detail"].as_str())
            .unwrap_or("");
        let availability = Availability::from_doctor(state);
        let model = if vendor.supports_auto_model() {
            "auto"
        } else {
            "default"
        };
        if vendor.featured() || availability == Availability::Ready {
            rows.push(vendor_target(
                vendor,
                model,
                if vendor.supports_auto_model() {
                    "Auto"
                } else {
                    "Default"
                },
                availability,
                detail,
                UsageSnapshot::unavailable(vendor.product_label()),
                false,
            ));
        }
    }
    rows
}

pub fn merge_discovered(
    base: Vec<PickerTarget>,
    discovered: Vec<PickerTarget>,
) -> Vec<PickerTarget> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for row in discovered.into_iter().chain(base) {
        if seen.insert(row.id.clone()) {
            out.push(row);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_stable_and_not_display_names() {
        assert_eq!(target_id(Vendor::Codex, "default"), "cli:codex");
        assert_eq!(target_id(Vendor::Cursor, "auto"), "cli:cursor:auto");
        assert_eq!(
            target_id(Vendor::Cursor, "gpt-5.3-codex"),
            "cli:cursor:gpt-5.3-codex"
        );
        let a = vendor_target(
            Vendor::Cursor,
            "sonnet-4",
            "Sonnet 4",
            Availability::Ready,
            "Ready",
            UsageSnapshot::unavailable("Cursor"),
            false,
        );
        let b = vendor_target(
            Vendor::Claude,
            "sonnet-4",
            "Sonnet 4",
            Availability::Ready,
            "Ready",
            UsageSnapshot::unavailable("Claude Code"),
            false,
        );
        assert_ne!(a.id, b.id);
        assert!(!a.id.contains("Sonnet"));
    }
}
