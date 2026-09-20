use rmcp::model::Tool;
use serde_json::{json, Value};

pub(super) fn tools() -> Vec<Tool> {
    let s = json!({"type":"string"});
    let b = json!({"type":"boolean"});
    let id = json!({"type":"string","format":"uuid"});
    let definitions = vec![
        ("shadow_status","Read this server's workspace, model, permissions and active jobs.",json!({}),vec![],true),
        ("shadow_models","List saved models; detect=true also probes configured/local providers.",json!({"detect":b}),vec![],false),
        ("shadow_sessions","List recent conversations in this workspace only.",json!({"limit":{"type":"integer","minimum":1,"maximum":100}}),vec![],true),
        ("shadow_review","Read Git status and pending/staged diffs in this workspace.",json!({"path":s}),vec![],true),
        ("shadow_memory","Read project notes or append a note. Append requires --allow-write and project trust.",json!({"action":{"type":"string","enum":["read","append"]},"note":s}),vec!["action"],false),
        ("shadow_goal","Create, list, inspect, advance or abandon a durable goal in this workspace. Changes require --allow-write; creation does not run milestones.",json!({"action":{"type":"string","enum":["create","list","get","advance","abandon"]},"instruction":s,"goal_id":id,"milestone_id":id,"status":s,"detail":s}),vec!["action"],false),
        ("shadow_run","Start one owned agent task using the native engine. Returns its job ID; use shadow_jobs to observe/cancel it. Default access is read-only. Pending actions need human approval through the desktop/CLI unless --allow-approvals explicitly delegates that authority. Closing this MCP connection cancels its unfinished tasks.",json!({"task":s,"model":s,"purpose":s,"permission_level":{"type":"string","enum":["read_only","workspace","elevated"]},"queue":b}),vec!["task"],false),
        ("shadow_jobs","Inspect this connection's delegated jobs, their pending approvals and paginated events, or cancel one. Cannot control tasks created by another client.",json!({"job_id":id,"cancel":b,"after":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":100}}),vec![],false),
        ("shadow_approve","Resolve an approval for this connection's own task. Approving requires --allow-write --allow-approvals; denial is always allowed. External clients must obtain user approval for the exact displayed arguments.",json!({"approval_id":id,"decision":{"type":"string","enum":["approve","deny"]}}),vec!["approval_id","decision"],false),
        ("shadow_checkpoint","Read a file checkpoint from this workspace. Without task_id, inspect the most recent task with changes.",json!({"task_id":id}),vec![],true),
        ("shadow_rollback","Restore an exact reviewed checkpoint with conflict checks. Requires task_id, confirm=true, --allow-write and project trust.",json!({"task_id":id,"confirm":b}),vec!["task_id","confirm"],false),
        ("shadow_tools","List native harness tools and their read-only status. Descriptions do not grant execution permission.",json!({}),vec![],true),
    ];
    definitions.into_iter().map(|(name,description,mut properties,required,read)| {
        properties["workspace"] = json!({"type":"string","description":"If supplied, must equal the project selected when this server started."});
        serde_json::from_value(json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},"annotations":{"readOnlyHint":read,"destructiveHint":!read,"idempotentHint":read,"openWorldHint":matches!(name,"shadow_run"|"shadow_models"|"shadow_approve")}})).expect("static MCP tool schema")
    }).collect()
}

pub(super) fn validate(tool: &Tool, args: &Value) -> anyhow::Result<()> {
    use anyhow::{ensure, Context};
    let args = args.as_object().context("Arguments must be an object")?;
    let schema = &tool.input_schema;
    let props = schema["properties"].as_object().unwrap();
    ensure!(
        args.keys().all(|k| props.contains_key(k)),
        "Unknown tool argument"
    );
    for key in schema["required"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
    {
        ensure!(args.contains_key(key), "Missing required argument: {key}");
    }
    for (key, value) in args {
        let prop = &props[key];
        ensure!(
            match prop["type"].as_str().unwrap() {
                "string" => value.is_string(),
                "boolean" => value.is_boolean(),
                "integer" => value.is_u64(),
                _ => false,
            },
            "Invalid type for {key}"
        );
        if let Some(choices) = prop["enum"].as_array() {
            ensure!(choices.contains(value), "Invalid value for {key}");
        }
        if prop["format"] == "uuid" {
            uuid::Uuid::parse_str(value.as_str().unwrap()).context("Invalid ID")?;
        }
        if let Some(n) = value.as_u64() {
            ensure!(
                n >= prop["minimum"].as_u64().unwrap_or(0)
                    && n <= prop["maximum"].as_u64().unwrap_or(u64::MAX),
                "Argument out of range: {key}"
            );
        }
    }
    Ok(())
}
