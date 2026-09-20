mod support;
use serde_json::{json, Value};
use shadowcode_core::{
    config::Config,
    paths::AppPaths,
    plugins::Bundle,
    service::{Request, Service},
    workflows,
    workspace::{hash, Workspace},
};
use std::{fs, time::Duration};
fn fixture(endpoint: &str) -> (tempfile::TempDir, Service) {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    fs::create_dir(&project).unwrap();
    let paths = AppPaths::isolated(&root.path().join("profile")).unwrap();
    Config::patch(&paths,json!({"trusted_workspaces":[project],"model":{"provider":"local","name":"fixture","endpoint":endpoint,"context_limit":16384},"permissions":{"approve_shell":false}})).unwrap();
    (root, Service::open(paths, Some(project)).unwrap())
}
async fn api(s: &Service, method: &str, path: &str, body: Value) -> anyhow::Result<Value> {
    s.dispatch(Request {
        method: method.into(),
        path: path.into(),
        body,
    })
    .await
}
fn bundle() -> Value {
    json!({"format":"shadowcode-plugin-v1","name":"fixture","version":"1.0.0","description":"A test plugin",
        "skills":{"audit":{"description":"Read the project","mode":"review","content":"PLUGIN_SELECTED: inspect $ARGUMENTS using reference.txt.","files":{"reference.txt":"Plugin supporting text"}}},
        "commands":{"check":{"description":"Complete a check","content":"PLUGIN_COMMAND: inspect $ARGUMENTS and report."}},
        "hooks":[{"name":"finished","events":["on_complete"],"command":"printf plugin-hook-ran > plugin-result.txt","timeout_sec":3}],
        "mcp_servers":[{"name":"peer","command":["sh","-c","touch plugin-peer-ran"]}]})
}
async fn install(s: &Service, bundle: Value) -> Value {
    let p = api(s, "POST", "/api/plugins/preview", json!({"bundle":bundle}))
        .await
        .unwrap();
    api(
        s,
        "POST",
        "/api/plugins/install",
        json!({"workspace":s.workspace().unwrap(),"bundle":p["bundle"],"hash":p["hash"]}),
    )
    .await
    .unwrap()
}
async fn remove(s: &Service) -> Value {
    let catalog = api(s, "GET", "/api/plugins", Value::Null).await.unwrap();
    let p = &catalog["installed"][0];
    api(
        s,
        "POST",
        "/api/plugins/remove",
        json!({"workspace":s.workspace().unwrap(),"name":p["name"],"hash":p["hash"]}),
    )
    .await
    .unwrap()
}
#[test]
fn validates_schema_names_limits_and_supporting_file_confinement() {
    assert_eq!(Bundle::parse(bundle()).unwrap().files().unwrap().len(), 5);
    for field in ["agents", "install", "permissions", "unknown"] {
        let mut b = bundle();
        b[field] = json!(["sh"]);
        assert!(Bundle::parse(b).is_err(), "{field}");
    }
    for name in ["../escape", "UPPER", "x/y", "", &"x".repeat(33)] {
        let mut b = bundle();
        b["name"] = json!(name);
        assert!(Bundle::parse(b).is_err());
    }
    for path in [
        "../escape",
        "/absolute",
        ".git/config",
        "nested/../escape",
        "a//b",
        "a/",
        "SKILL.md",
        "nested/SKILL.md",
        "a\\b",
        ".hidden/file",
    ] {
        let mut b = bundle();
        b["skills"]["audit"]["files"] = json!({path:"text"});
        assert!(Bundle::parse(b).is_err(), "{path}");
    }
    let mut duplicate = bundle();
    duplicate["commands"]["audit"] = duplicate["skills"]["audit"].clone();
    assert!(Bundle::parse(duplicate).is_err());
    let mut ancestor = bundle();
    ancestor["skills"]["audit"]["files"] = json!({"folder":"file", "folder/child":"child"});
    assert!(Bundle::parse(ancestor).is_err());
    let mut b = bundle();
    b["skills"]["audit"]["mode"] = json!("elevated");
    assert!(Bundle::parse(b).is_err());
    let mut b = bundle();
    b["mcp_servers"][0]["env"] = json!({"TOKEN":"private"});
    assert!(Bundle::parse(b).is_err());
    let mut b = bundle();
    b["hooks"][0]["events"] = json!(["unknown"]);
    assert!(Bundle::parse(b).is_err());
    let mut b = bundle();
    b["hooks"] = json!([b["hooks"][0], b["hooks"][0]]);
    assert!(Bundle::parse(b).is_err());
    let mut b = bundle();
    b["skills"]["audit"]["content"] = json!("x".repeat(256001));
    assert!(Bundle::parse(b).is_err());
}
#[tokio::test]
async fn real_workflows_hooks_mcp_activation_and_uninstall_preserve_edits() {
    let model = support::server(|_,_|(json!({"choices":[{"message":{"role":"assistant","content":"Inspected the requested scope."},"finish_reason":"stop"}]}),Duration::ZERO)).await;
    let (_root, s) = fixture(&model.endpoint);
    let result = install(&s, bundle()).await;
    assert_eq!(result["catalog"]["installed"][0]["state"], "installed");
    let ws = Workspace::open(&s.workspace().unwrap()).unwrap();
    let skills = workflows::discover(&ws);
    assert!(skills.issues.is_empty(), "{:?}", skills.issues);
    assert!(skills.resolve("fixture--audit", None).is_ok());
    assert!(skills.resolve("fixture--check", None).is_ok());
    for (url, field) in [("/api/hooks", "hooks"), ("/api/mcp/servers", "servers")] {
        let c = api(&s, "GET", url, Value::Null).await.unwrap();
        assert_eq!(c[field][0]["enabled"], false);
    }
    let job = api(
        &s,
        "POST",
        "/api/commands/run",
        json!({"name":"skill","args":"fixture--audit README.md"}),
    )
    .await
    .unwrap()["metadata"]["job"]
        .clone();
    let done = tokio::time::timeout(
        Duration::from_secs(8),
        s.engine.wait(job["id"].as_str().unwrap()),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(done.status, "completed");
    assert_eq!(done.mode, "review");
    assert!(model.requests.lock().unwrap()[0]
        .to_string()
        .contains("PLUGIN_SELECTED: inspect README.md"));
    assert!(!ws.path.join("plugin-result.txt").exists());
    assert!(!ws.path.join("plugin-peer-ran").exists());
    let hook = api(&s, "GET", "/api/hooks", Value::Null).await.unwrap()["hooks"][0].clone();
    api(
        &s,
        "POST",
        "/api/hooks/activation",
        json!({"workspace":ws.path,"path":hook["path"],"hash":hook["hash"],"enabled":true}),
    )
    .await
    .unwrap();
    let job = api(
        &s,
        "POST",
        "/api/commands/run",
        json!({"name":"fixture--check","args":"README.md"}),
    )
    .await
    .unwrap()["metadata"]["job"]
        .clone();
    let done = tokio::time::timeout(
        Duration::from_secs(8),
        s.engine.wait(job["id"].as_str().unwrap()),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(done.status, "completed", "{}", done.summary);
    assert_eq!(
        fs::read_to_string(ws.path.join("plugin-result.txt")).unwrap(),
        "plugin-hook-ran"
    );
    assert!(model.requests.lock().unwrap()[1]
        .to_string()
        .contains("PLUGIN_COMMAND: inspect README.md"));
    let server = api(&s, "GET", "/api/mcp/servers", Value::Null)
        .await
        .unwrap()["servers"][0]
        .clone();
    api(
        &s,
        "POST",
        "/api/mcp/activation",
        json!({"workspace":ws.path,"server":server["id"],"hash":server["hash"],"enabled":true}),
    )
    .await
    .unwrap();
    let edited = ".shadow/commands/fixture--check.md";
    ws.write(edited, b"My modified workflow", None).unwrap();
    let result = remove(&s).await;
    assert_eq!(result["result"]["retained"][0]["path"], edited);
    assert_eq!(ws.read(edited).unwrap().content, "My modified workflow");
    assert!(!ws.path.join(".shadowcode/skills/fixture--audit").exists());
    assert!(workflows::discover(&ws).issues.is_empty());
    let cfg = Config::load(s.engine.paths(), None).unwrap();
    assert!(cfg.hooks.approved.is_empty());
    assert!(shadowcode_core::mcp::registry::activations(&cfg)
        .unwrap()
        .is_empty());
    assert!(!ws.path.join("plugin-peer-ran").exists());
    s.engine.shutdown().await.unwrap();
}
#[tokio::test]
async fn mutations_require_trust_current_project_current_hash_and_idle_workspace() {
    let (_root, s) = fixture("http://127.0.0.1:9/v1");
    let p = api(
        &s,
        "POST",
        "/api/plugins/preview",
        json!({"bundle":bundle()}),
    )
    .await
    .unwrap();
    let body = json!({"workspace":s.workspace().unwrap(),"bundle":p["bundle"],"hash":p["hash"]});
    let mut wrong = body.clone();
    wrong["workspace"] = json!("/another");
    assert!(api(&s, "POST", "/api/plugins/install", wrong)
        .await
        .unwrap_err()
        .to_string()
        .contains("Project changed"));
    let mut wrong = body.clone();
    wrong["hash"] = json!("stale");
    assert!(api(&s, "POST", "/api/plugins/install", wrong)
        .await
        .is_err());
    Config::patch(s.engine.paths(), json!({"trusted_workspaces":[]})).unwrap();
    assert!(api(&s, "POST", "/api/plugins/install", body.clone())
        .await
        .is_err());
    Config::patch(
        s.engine.paths(),
        json!({"trusted_workspaces":[s.workspace().unwrap()],"permissions":{"level":"read_only"}}),
    )
    .unwrap();
    assert!(api(&s, "POST", "/api/plugins/install", body.clone())
        .await
        .is_err());
    Config::patch(
        s.engine.paths(),
        json!({"permissions":{"level":"workspace"}}),
    )
    .unwrap();
    let reservation = s.engine.reserve_workspace(&s.workspace().unwrap()).unwrap();
    assert!(api(&s, "POST", "/api/plugins/install", body.clone())
        .await
        .is_err());
    drop(reservation);
    let r = api(&s, "POST", "/api/plugins/install", body.clone())
        .await
        .unwrap();
    assert!(api(&s, "POST", "/api/plugins/install", body).await.is_err());
    assert!(api(
        &s,
        "POST",
        "/api/plugins/remove",
        json!({"workspace":s.workspace().unwrap(),"name":"fixture","hash":"stale"})
    )
    .await
    .is_err());
    let other = s.workspace().unwrap().parent().unwrap().join("other");
    fs::create_dir(&other).unwrap();
    let independent = s.fork_selection(other, None).unwrap();
    assert!(api(&independent, "GET", "/api/plugins", Value::Null)
        .await
        .unwrap()["installed"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(r["catalog"]["installed"].as_array().unwrap().len(), 1);
    remove(&s).await;
    s.engine.shutdown().await.unwrap();
}
#[tokio::test]
async fn install_preflights_conflicts_symlinks_and_clears_stale_executable_grants() {
    let (root, s) = fixture("http://127.0.0.1:9/v1");
    let ws = Workspace::open(&s.workspace().unwrap()).unwrap();
    ws.write(
        ".shadowcode/hooks/fixture--finished.json",
        b"user data",
        Some("missing"),
    )
    .unwrap();
    let b = Bundle::parse(bundle()).unwrap();
    assert!(shadowcode_core::plugins::install(
        s.engine.paths(),
        &ws,
        b.clone(),
        &b.hash().unwrap()
    )
    .is_err());
    assert!(!ws.path.join(".shadow/commands/fixture--check.md").exists());
    ws.delete(".shadowcode/hooks/fixture--finished.json", None)
        .unwrap();
    let outside = root.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::create_dir_all(ws.path.join(".shadowcode/skills")).unwrap();
    std::os::unix::fs::symlink(&outside, ws.path.join(".shadowcode/skills/fixture--audit"))
        .unwrap();
    assert!(shadowcode_core::plugins::install(
        s.engine.paths(),
        &ws,
        b.clone(),
        &b.hash().unwrap()
    )
    .is_err());
    assert!(fs::read_dir(outside).unwrap().next().is_none());
    fs::remove_file(ws.path.join(".shadowcode/skills/fixture--audit")).unwrap();
    // A previous identical definition's grant must not activate a fresh install.
    let files = b.files().unwrap();
    let hook = files.iter().find(|f| f.kind == "hook").unwrap();
    let mut config = Config::load(s.engine.paths(), None).unwrap();
    config
        .hooks
        .approved
        .push(shadowcode_core::hooks::Approval {
            workspace: ws.path.to_string_lossy().into(),
            path: hook.path.clone(),
            hash: hook.hash.clone(),
        });
    config.save(s.engine.paths()).unwrap();
    install(&s, bundle()).await;
    assert_eq!(
        api(&s, "GET", "/api/hooks", Value::Null).await.unwrap()["hooks"][0]["enabled"],
        false
    );
    remove(&s).await;
}
#[tokio::test]
async fn interrupted_journal_recovers_only_owned_bytes_and_preserves_legacy() {
    let (_root, s) = fixture("http://127.0.0.1:9/v1");
    let ws = Workspace::open(&s.workspace().unwrap()).unwrap();
    let b = Bundle::parse(bundle()).unwrap();
    let files = b.files().unwrap();
    let private = Workspace::open(&s.engine.paths().state).unwrap();
    let path = format!(
        "native-plugins/{}/fixture.json",
        hash(ws.path.as_os_str().as_encoded_bytes())
    );
    private
        .write(
            &path,
            serde_json::to_string(&json!({"workspace":ws.path,"state":"prepared","bundle":b}))
                .unwrap()
                .as_bytes(),
            Some("missing"),
        )
        .unwrap();
    ws.write(&files[0].path, files[0].content.as_bytes(), Some("missing"))
        .unwrap();
    ws.write(&files[1].path, b"Concurrent user data", Some("missing"))
        .unwrap();
    let legacy = s
        .engine
        .paths()
        .config
        .join("shadowcode/plugins/python-expert/plugin.yaml");
    fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    fs::write(&legacy, "legacy content").unwrap();
    let catalog = api(&s, "GET", "/api/plugins", Value::Null).await.unwrap();
    assert_eq!(catalog["installed"][0]["state"], "prepared");
    assert_eq!(catalog["legacy"], json!(["python-expert"]));
    let result = remove(&s).await;
    assert_eq!(result["result"]["retained"].as_array().unwrap().len(), 1);
    assert!(ws.snapshot(&files[0].path).unwrap().bytes.is_none());
    assert_eq!(
        ws.read(&files[1].path).unwrap().content,
        "Concurrent user data"
    );
    assert!(private.snapshot(&path).unwrap().bytes.is_none());
    assert_eq!(fs::read_to_string(legacy).unwrap(), "legacy content");
}
#[tokio::test]
async fn builtins_install_into_real_discovery_and_remove_without_broken_skills() {
    let (_root, s) = fixture("http://127.0.0.1:9/v1");
    let catalog = api(&s, "GET", "/api/plugins", Value::Null).await.unwrap();
    for entry in catalog["available"].as_array().unwrap() {
        let p = api(
            &s,
            "POST",
            "/api/plugins/preview",
            json!({"name":entry["name"]}),
        )
        .await
        .unwrap();
        install(&s, p["bundle"].clone()).await;
        let ws = Workspace::open(&s.workspace().unwrap()).unwrap();
        assert!(!workflows::discover(&ws).definitions.is_empty());
        remove(&s).await;
        let c = workflows::discover(&ws);
        assert!(c.definitions.is_empty());
        assert!(c.issues.is_empty(), "{:?}", c.issues);
    }
}

#[tokio::test]
async fn install_refuses_ambiguous_workflows_and_full_hook_catalogs_before_writing() {
    let (_root, s) = fixture("http://127.0.0.1:9/v1");
    let ws = Workspace::open(&s.workspace().unwrap()).unwrap();
    let b = Bundle::parse(bundle()).unwrap();
    ws.write(
        ".agents/skills/conflict/SKILL.md",
        b"---\nname: fixture--check\n---\nUser workflow",
        Some("missing"),
    )
    .unwrap();
    assert!(shadowcode_core::plugins::install(
        s.engine.paths(),
        &ws,
        b.clone(),
        &b.hash().unwrap()
    )
    .unwrap_err()
    .to_string()
    .contains("already exists"));
    assert!(!ws.path.join(".shadow/commands/fixture--check.md").exists());
    ws.delete(".agents/skills/conflict/SKILL.md", None).unwrap();
    for i in 0..64 {
        ws.write(
            &format!(".shadowcode/hooks/existing-{i}.json"),
            b"invalid but user-owned",
            Some("missing"),
        )
        .unwrap();
    }
    assert!(shadowcode_core::plugins::install(
        s.engine.paths(),
        &ws,
        b.clone(),
        &b.hash().unwrap()
    )
    .unwrap_err()
    .to_string()
    .contains("64 definitions"));
    assert!(!ws
        .path
        .join(".shadowcode/hooks/fixture--finished.json")
        .exists());
}
