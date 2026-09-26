//! Session modes and config options: ShadowCode's code / plan / ask modes and
//! its unified model picker (subscription CLIs, OpenRouter, local GGUF and
//! configured endpoints) in ACP's `modes`, `configOptions` and the unstable
//! `models` shapes.
use serde_json::{json, Value};

/// Offered while a conversation has no remembered model of its own.
pub(crate) const DEFAULT_MODEL: &str = "default";
/// Rows beyond this are left out of the editor's list (OpenRouter alone can
/// list hundreds); `session/set_model` still accepts any picker id.
const MAX_MODELS: usize = 150;

/// (id, name, description, task purpose).
const MODES: [(&str, &str, &str, &str); 3] = [
    (
        "code",
        "Code",
        "Edit files and run commands; risky steps ask first",
        "coder",
    ),
    (
        "plan",
        "Plan",
        "Read-only: investigate the project and write a plan",
        "planner",
    ),
    (
        "ask",
        "Ask",
        "Read-only: answer questions about the project",
        "reviewer",
    ),
];

pub(crate) fn valid_mode(mode: &str) -> bool {
    MODES.iter().any(|(id, ..)| *id == mode)
}

/// The engine purpose for a mode (the engine makes plan and review read-only).
pub(crate) fn purpose(mode: &str) -> &'static str {
    MODES
        .iter()
        .find(|(id, ..)| *id == mode)
        .map_or("coder", |(.., purpose)| purpose)
}

/// The ACP mode of a stored job (`plan` / `review` / `code`).
pub(crate) fn mode_of_job(job_mode: &str) -> &'static str {
    match job_mode {
        "plan" => "plan",
        "review" => "ask",
        _ => "code",
    }
}

pub(crate) fn modes_state(current: &str) -> Value {
    json!({
        "currentModeId": current,
        "availableModes": MODES
            .iter()
            .map(|(id, name, description, _)| json!({"id":id,"name":name,"description":description}))
            .collect::<Vec<_>>(),
    })
}

fn group_name(group: &str) -> String {
    match group {
        "subscriptions" => "Subscriptions".into(),
        "local" => "This computer".into(),
        "api" => "OpenRouter".into(),
        "" => "Models".into(),
        other => {
            let mut chars = other.chars();
            chars
                .next()
                .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        }
    }
}

/// One selectable model: (group id, value, name, description).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ModelChoice {
    pub group: String,
    pub value: String,
    pub name: String,
    pub description: String,
}

/// Ready rows of `/api/picker`'s `targets`, featured OpenRouter rows first.
pub(crate) fn model_choices(picker: &Value) -> Vec<ModelChoice> {
    let rows: Vec<&Value> = picker["targets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| row["availability"] == "ready" && row["id"].as_str().is_some())
        .collect();
    let mut ordered: Vec<&Value> = rows
        .iter()
        .copied()
        .filter(|r| r["group"] != "api")
        .collect();
    ordered.extend(
        rows.iter()
            .copied()
            .filter(|r| r["group"] == "api" && r["featured"] == true),
    );
    ordered.extend(
        rows.iter()
            .copied()
            .filter(|r| r["group"] == "api" && r["featured"] != true),
    );
    let mut seen = std::collections::HashSet::new();
    ordered
        .into_iter()
        .filter(|row| seen.insert(row["id"].as_str().unwrap_or("").to_owned()))
        .take(MAX_MODELS)
        .map(|row| ModelChoice {
            group: row["group"].as_str().unwrap_or("").to_owned(),
            value: row["id"].as_str().unwrap_or("").to_owned(),
            name: row["name"]
                .as_str()
                .filter(|n| !n.is_empty())
                .or_else(|| row["id"].as_str())
                .unwrap_or("")
                .to_owned(),
            description: row["subtitle"].as_str().unwrap_or("").to_owned(),
        })
        .collect()
}

/// The choices shown for a session: the project default while the
/// conversation remembers no model, and the current model even if it is not
/// among the ready rows.
pub(crate) fn session_choices(choices: &[ModelChoice], current: &str) -> Vec<ModelChoice> {
    let mut list = Vec::new();
    if current.is_empty() || current == DEFAULT_MODEL {
        list.push(ModelChoice {
            group: String::new(),
            value: DEFAULT_MODEL.into(),
            name: "Project default".into(),
            description: "The model this project uses when none is chosen".into(),
        });
    } else if !choices.iter().any(|c| c.value == current) {
        list.push(ModelChoice {
            group: String::new(),
            value: current.into(),
            name: current.into(),
            description: "Remembered for this conversation".into(),
        });
    }
    list.extend(choices.iter().cloned());
    list
}

fn current_value(current: &str) -> &str {
    if current.is_empty() {
        DEFAULT_MODEL
    } else {
        current
    }
}

/// `configOptions`: mode first, then the model (grouped by picker section).
pub(crate) fn config_options(mode: &str, model: &str, choices: &[ModelChoice]) -> Value {
    let list = session_choices(choices, model);
    let mut groups: Vec<(String, Vec<Value>)> = Vec::new();
    for choice in &list {
        let option =
            json!({"value":choice.value,"name":choice.name,"description":choice.description});
        match groups.iter_mut().find(|(g, _)| *g == choice.group) {
            Some((_, options)) => options.push(option),
            None => groups.push((choice.group.clone(), vec![option])),
        }
    }
    let options: Value = if groups.len() <= 1 {
        json!(groups.into_iter().flat_map(|(_, o)| o).collect::<Vec<_>>())
    } else {
        json!(groups
            .into_iter()
            .map(|(group, options)| json!({
                "group": if group.is_empty() { "current".to_owned() } else { group.clone() },
                "name": if group.is_empty() { "Current".to_owned() } else { group_name(&group) },
                "options": options,
            }))
            .collect::<Vec<_>>())
    };
    json!([
        {
            "id": "mode",
            "name": "Mode",
            "description": "How much ShadowCode may change",
            "category": "mode",
            "type": "select",
            "currentValue": mode,
            "options": MODES
                .iter()
                .map(|(id, name, description, _)| json!({"value":id,"name":name,"description":description}))
                .collect::<Vec<_>>(),
        },
        {
            "id": "model",
            "name": "Model",
            "description": "Subscription CLIs, OpenRouter and models on this computer",
            "category": "model",
            "type": "select",
            "currentValue": current_value(model),
            "options": options,
        }
    ])
}

/// The unstable `models` field some clients still read.
pub(crate) fn models_state(model: &str, choices: &[ModelChoice]) -> Value {
    json!({
        "currentModelId": current_value(model),
        "availableModels": session_choices(choices, model)
            .into_iter()
            .map(|c| json!({"modelId":c.value,"name":c.name,"description":c.description}))
            .collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_map_to_purposes_and_back() {
        assert_eq!(purpose("plan"), "planner");
        assert_eq!(purpose("ask"), "reviewer");
        assert_eq!(purpose("code"), "coder");
        assert_eq!(mode_of_job("review"), "ask");
        assert!(valid_mode("ask") && !valid_mode("yolo"));
        assert_eq!(modes_state("plan")["availableModes"][1]["id"], "plan");
    }

    #[test]
    fn picker_rows_become_grouped_ready_options() {
        let picker = json!({"targets":[
            {"id":"cli:codex","group":"subscriptions","name":"Codex","availability":"ready"},
            {"id":"cli:claude","group":"subscriptions","name":"Claude","availability":"sign_in"},
            {"id":"local:gguf:a","group":"local","name":"A","availability":"ready"},
            {"id":"openrouter:x","group":"api","name":"X","availability":"ready"},
            {"id":"openrouter:y","group":"api","name":"Y","availability":"ready","featured":true},
        ]});
        let choices = model_choices(&picker);
        let ids: Vec<_> = choices.iter().map(|c| c.value.as_str()).collect();
        assert_eq!(
            ids,
            ["cli:codex", "local:gguf:a", "openrouter:y", "openrouter:x"]
        );
        let options = config_options("code", "", &choices);
        assert_eq!(options[1]["currentValue"], "default");
        assert_eq!(options[1]["options"][0]["options"][0]["value"], "default");
        assert_eq!(options[1]["options"][1]["name"], "Subscriptions");
        let chosen = config_options("ask", "local:gguf:a", &choices);
        assert_eq!(chosen[0]["currentValue"], "ask");
        assert_eq!(chosen[1]["currentValue"], "local:gguf:a");
        assert!(!chosen.to_string().contains("\"default\""));
        let flat = config_options("code", "", &[]);
        assert_eq!(flat[1]["options"][0]["value"], "default");
        assert_eq!(
            models_state("", &choices)["availableModels"][0]["modelId"],
            "default"
        );
    }
}
