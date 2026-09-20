use rmcp::model::Tool;
use serde_json::{json, Value};

pub(super) fn tools() -> Vec<Tool> {
    let s = json!({"type":"string"});
    let b = json!({"type":"boolean"});
    let id = json!({"type":"string","format":"uuid"});
    let definitions = vec![
        ("shadow_understand","Inspect the selected project's stack, modules, source markers and test candidates without running code or a model. save=true updates only the generated section of project notes and requires --allow-write.",json!({"save":b}),vec![],false),
        ("shadow_doctor","Check the native runtime, configuration, private profile files, SQLite integrity and project toolchains. test_model=true explicitly sends a small diagnostic model prompt. No automatic fixes or installs.",json!({"test_model":b}),vec![],false),
        ("shadow_why","Read bounded commit history and current/staged diffs for a project path. Recorded evidence does not establish author intent.",json!({"path":s,"count":{"type":"integer","minimum":1,"maximum":50}}),vec![],true),
        ("shadow_test","Run an exact test command as an owned native task, without a model. If omitted, auto-select only a single unambiguous detected command. Requires --allow-write and an exact command approval. Returns a job ID; inspect shadow_jobs for completion, stdout/stderr, exit status and approvals. It belongs to the stdio connection or authenticated HTTP gateway; owner shutdown cancels unfinished work.",json!({"command":s,"timeout":{"type":"integer","minimum":1,"maximum":3600},"queue":b}),vec![],false),
        ("shadow_status","Read this server's workspace, model, permissions and active jobs.",json!({}),vec![],true),
        ("shadow_sqlite","List tables when sql is omitted, or execute a read-only SELECT/WITH or schema PRAGMA in an existing project database. Bind ? placeholders with params. No SQL writes, attachments, extensions or Python. Results are capped at 1000 rows / 1 MB; check truncated. SQLite may maintain WAL coordination sidecars.",json!({"path":s,"sql":s,"params":{"type":"array","items":{"type":["string","number","boolean","null"]},"maxItems":128},"limit":{"type":"integer","minimum":1,"maximum":1000},"timeout_ms":{"type":"integer","minimum":1,"maximum":10000}}),vec!["path"],true),
        ("shadow_models","List saved models; detect=true also probes configured/local providers.",json!({"detect":b}),vec![],false),
        ("shadow_sessions","List recent conversations in this workspace only.",json!({"limit":{"type":"integer","minimum":1,"maximum":100}}),vec![],true),
        ("shadow_review","Read Git status and pending/staged diffs in this workspace.",json!({"path":s}),vec![],true),
        ("shadow_memory","Read, append or replace project/task notes. Replacement requires the exact current project_hash/task_hash as expected_hash; an empty note clears it. scope defaults to project; task scope requires an exact existing task_id in this project. Changes require --allow-write and project trust. Notes do not grant permissions or establish verified facts.",json!({"action":{"type":"string","enum":["read","append","replace"]},"scope":{"type":"string","enum":["project","task"]},"task_id":id,"note":s,"expected_hash":s}),vec!["action"],false),
        ("shadow_goal","Create, list, inspect, advance or abandon a durable goal in this workspace. Changes require --allow-write; creation does not run milestones.",json!({"action":{"type":"string","enum":["create","list","get","advance","abandon"]},"instruction":s,"goal_id":id,"milestone_id":id,"status":s,"detail":s}),vec!["action"],false),
        ("shadow_run","Start one owned agent task using the native engine. Returns its job ID; use shadow_jobs to observe/cancel it. Default access is read-only. Pending actions need human approval through the desktop/CLI unless --allow-approvals explicitly delegates that authority. The stdio connection or authenticated HTTP gateway owns this task; owner shutdown cancels unfinished work.",json!({"task":s,"model":s,"purpose":s,"permission_level":{"type":"string","enum":["read_only","workspace","elevated"]},"queue":b}),vec!["task"],false),
        ("shadow_jobs","Inspect this MCP owner's delegated jobs, their pending approvals and paginated events, or cancel one. Cannot control another owner's tasks.",json!({"job_id":id,"cancel":b,"after":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":100}}),vec![],false),
        ("shadow_approve","Resolve an approval for this MCP owner's task. Approving requires --allow-write --allow-approvals; denial is always allowed. External clients must obtain user approval for the exact displayed arguments.",json!({"approval_id":id,"decision":{"type":"string","enum":["approve","deny"]}}),vec!["approval_id","decision"],false),
        ("shadow_checkpoint","Read a file checkpoint from this workspace. Without task_id, inspect the most recent task with changes.",json!({"task_id":id}),vec![],true),
        ("shadow_rollback","Restore an exact reviewed checkpoint with conflict checks. Requires task_id, confirm=true, --allow-write and project trust.",json!({"task_id":id,"confirm":b}),vec!["task_id","confirm"],false),
        ("shadow_tools","List native harness tools and their read-only status. Descriptions do not grant execution permission.",json!({}),vec![],true),
    ];
    definitions.into_iter().map(|(name,description,mut properties,required,read)| {
        properties["workspace"] = json!({"type":"string","description":"If supplied, must equal the project selected when this server started."});
        serde_json::from_value(json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},"annotations":{"readOnlyHint":read,"destructiveHint":!read,"idempotentHint":read,"openWorldHint":matches!(name,"shadow_run"|"shadow_models"|"shadow_approve"|"shadow_test"|"shadow_doctor")}})).expect("static MCP tool schema")
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
                "array" => value.is_array(),
                _ => false,
            },
            "Invalid type for {key}"
        );
        if let Some(choices) = prop["enum"].as_array() {
            ensure!(choices.contains(value), "Invalid value for {key}");
        }
        if let (Some(values), Some(limit)) = (value.as_array(), prop["maxItems"].as_u64()) {
            ensure!(values.len() <= limit as usize, "Too many values for {key}");
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
