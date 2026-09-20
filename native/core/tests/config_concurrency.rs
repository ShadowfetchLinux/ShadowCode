use serde_json::json;
use shadowcode_core::{
    config::{self, Config},
    paths::AppPaths,
};
use std::sync::Barrier;

#[test]
fn concurrent_setting_patches_preserve_every_independent_field() {
    let root = tempfile::tempdir().unwrap();
    let paths = AppPaths::isolated(root.path()).unwrap();
    let barrier = Barrier::new(12);
    std::thread::scope(|scope| {
        for worker in 0..12 {
            let paths = &paths;
            let barrier = &barrier;
            scope.spawn(move || {
                let mut results = Vec::new();
                for round in 0..8 {
                    barrier.wait();
                    results.push(Config::patch(
                        paths,
                        json!({"ui":{format!("worker_{worker}"):round}}),
                    ));
                    barrier.wait();
                }
                for result in results {
                    result.unwrap();
                }
            });
        }
    });
    let config = Config::load(&paths, None).unwrap();
    for worker in 0..12 {
        assert_eq!(
            config.ui[format!("worker_{worker}")],
            7,
            "Independent setting lost for writer {worker}"
        );
    }
}
#[test]
fn concurrent_secret_updates_preserve_every_independent_entry() {
    let root = tempfile::tempdir().unwrap();
    let paths = AppPaths::isolated(root.path()).unwrap();
    let barrier = Barrier::new(12);
    std::thread::scope(|scope| {
        for worker in 0..12 {
            let paths = &paths;
            let barrier = &barrier;
            scope.spawn(move || {
                let mut results = Vec::new();
                for round in 0..8 {
                    barrier.wait();
                    results.push(config::set_secret(
                        paths,
                        &format!("FIXTURE_{worker}"),
                        &format!("fixture-{round}"),
                    ));
                    barrier.wait();
                }
                for result in results {
                    result.unwrap();
                }
            });
        }
    });
    for worker in 0..12 {
        assert_eq!(
            config::secret(&paths, &format!("FIXTURE_{worker}"))
                .unwrap()
                .as_deref(),
            Some("fixture-7"),
            "Independent secret lost for writer {worker}"
        );
    }
}

#[test]
fn rejected_update_keeps_exact_file_and_releases_the_writer() {
    let root = tempfile::tempdir().unwrap();
    let paths = AppPaths::isolated(root.path()).unwrap();
    Config::patch(&paths, json!({"ui":{"theme":"dark"}})).unwrap();
    let before = std::fs::read(paths.config_file()).unwrap();
    assert!(Config::update(&paths, |cfg| {
        cfg.ui["theme"] = json!("changed");
        anyhow::bail!("Rejected fixture update")
    })
    .is_err());
    assert_eq!(std::fs::read(paths.config_file()).unwrap(), before);
    assert!(Config::patch(&paths, json!({"agent":{"max_steps":0}})).is_err());
    assert!(Config::patch(&paths, json!({"ui":[]})).is_err());
    assert_eq!(std::fs::read(paths.config_file()).unwrap(), before);
    Config::patch(&paths, json!({"ui":{"notify":false}})).unwrap();
    assert_eq!(Config::load(&paths, None).unwrap().ui["theme"], "dark");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn simultaneous_projects_keep_trust_settings_and_integration_grants() {
    use serde_json::Value;
    use shadowcode_core::{
        service::{Request, Service},
        workspace::Workspace,
    };
    use std::sync::Arc;
    async fn call(service: &Service, method: &str, path: &str, body: Value) -> Value {
        service
            .dispatch(Request {
                method: method.into(),
                path: path.into(),
                body,
            })
            .await
            .unwrap()
    }
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let paths = AppPaths::isolated(&root.path().join("profile")).unwrap();
    let service = Service::open(paths.clone(), Some(home)).unwrap();
    let barrier = Arc::new(tokio::sync::Barrier::new(8));
    let mut handles = Vec::new();
    for worker in 0..8 {
        let project = root.path().join(format!("project-{worker}"));
        std::fs::create_dir(&project).unwrap();
        let client = service.fork_selection(project.clone(), None).unwrap();
        let barrier = barrier.clone();
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            call(&client,"POST","/api/projects/trust",json!({"path":project})).await;
            let ws=Workspace::open(&project).unwrap();
            let hook=".shadowcode/hooks/existing.json";
            let hook_hash=ws.write(hook,b"{\"name\":\"existing\",\"events\":[\"on_complete\"],\"command\":\"true\"}",Some("missing")).unwrap();
            barrier.wait().await;
            call(&client,"POST","/api/hooks/activation",json!({"workspace":project,"path":hook,"hash":hook_hash,"enabled":true})).await;
            barrier.wait().await;
            let name=format!("peer-{worker}");
            call(&client,"POST","/api/mcp/servers",json!({"definition":{"name":name,"command":["true"]}})).await;
            let catalog=call(&client,"GET","/api/mcp/servers",Value::Null).await;
            let entry=catalog["servers"].as_array().unwrap().iter().find(|row|row["name"]==name).unwrap();
            call(&client,"POST","/api/mcp/activation",json!({"workspace":project,"server":entry["id"],"hash":entry["hash"],"enabled":true})).await;
            let preview=call(&client,"POST","/api/plugins/preview",json!({"name":"python-expert"})).await;
            barrier.wait().await;
            call(&client,"POST","/api/plugins/install",json!({"workspace":project,"bundle":preview["bundle"],"hash":preview["hash"]})).await;
            let hooks=call(&client,"GET","/api/hooks",Value::Null).await;
            for hook in hooks["hooks"].as_array().unwrap().iter().filter(|h|h["name"].as_str().unwrap().starts_with("python-expert--")) {
                call(&client,"POST","/api/hooks/activation",json!({"workspace":project,"path":hook["path"],"hash":hook["hash"],"enabled":true})).await;
            }
            barrier.wait().await;
            call(&client,"PUT","/api/config",json!({"values":{"ui":{format!("project_{worker}"):"preserved"}},"api_key_env":format!("PARALLEL_{worker}"),"api_key":"fixture-only"})).await;
            let installed=call(&client,"GET","/api/plugins",Value::Null).await;
            call(&client,"POST","/api/plugins/remove",json!({"workspace":project,"name":"python-expert","hash":installed["installed"][0]["hash"]})).await;
            project
        }));
    }
    let projects = tokio::time::timeout(std::time::Duration::from_secs(20), async {
        let mut projects = Vec::new();
        for handle in handles {
            projects.push(handle.await.unwrap());
        }
        projects
    })
    .await
    .expect("Concurrent configuration requests did not finish");
    let cfg = Config::load(&paths, None).unwrap();
    assert_eq!(cfg.hooks.approved.len(), 8);
    assert_eq!(
        shadowcode_core::mcp::registry::activations(&cfg)
            .unwrap()
            .len(),
        8
    );
    assert_eq!(cfg.mcp["servers"].as_array().unwrap().len(), 8);
    for (worker, project) in projects.into_iter().enumerate() {
        assert!(cfg.is_trusted(&project));
        assert_eq!(cfg.ui[format!("project_{worker}")], "preserved");
        assert_eq!(
            config::secret(&paths, &format!("PARALLEL_{worker}"))
                .unwrap()
                .as_deref(),
            Some("fixture-only")
        );
        assert!(cfg
            .hooks
            .approved
            .iter()
            .any(|grant| grant.workspace == project.to_string_lossy()
                && grant.path == ".shadowcode/hooks/existing.json"));
    }
    service.engine.shutdown().await.unwrap();
}

#[test]
fn bounded_writes_never_create_settings_or_secrets_the_reader_cannot_reopen() {
    let root = tempfile::tempdir().unwrap();
    let paths = AppPaths::isolated(root.path()).unwrap();
    Config::patch(&paths, json!({"ui":{"theme":"dark"}})).unwrap();
    let before = std::fs::read(paths.config_file()).unwrap();
    assert!(Config::patch(&paths, json!({"ui":{"oversized":"x".repeat(1_000_000)}})).is_err());
    assert_eq!(std::fs::read(paths.config_file()).unwrap(), before);
    // A near-limit legacy file may be read, but a write must not make it unreadable.
    let original = format!("EXISTING={}\n", "x".repeat(990_000));
    std::fs::write(paths.secrets_file(), &original).unwrap();
    assert!(config::set_secret(&paths, "NEW_VALUE", &"x".repeat(16_384)).is_err());
    assert_eq!(
        std::fs::read_to_string(paths.secrets_file()).unwrap(),
        original
    );
    assert!(config::secrets(&paths).unwrap().contains_key("EXISTING"));
    // Both initial metadata and concurrent growth are bounded by the actual read.
    std::fs::write(paths.config_file(), "x".repeat(1_000_001)).unwrap();
    assert!(Config::load(&paths, None).is_err());
    std::fs::write(paths.secrets_file(), "x".repeat(1_000_001)).unwrap();
    assert!(config::secrets(&paths).is_err());
}

#[cfg(unix)]
#[test]
fn configuration_and_secret_pipes_are_rejected_without_waiting_for_a_writer() {
    use std::os::unix::ffi::OsStrExt;
    let root = tempfile::tempdir().unwrap();
    let paths = AppPaths::isolated(root.path()).unwrap();
    for path in [paths.config_file(), paths.secrets_file()] {
        let path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    }
    let start = std::time::Instant::now();
    assert!(Config::load(&paths, None)
        .unwrap_err()
        .to_string()
        .contains("regular files"));
    assert!(config::secrets(&paths)
        .unwrap_err()
        .to_string()
        .contains("regular files"));
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
}
