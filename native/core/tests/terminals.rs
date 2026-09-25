//! The drawer's interactive terminals: a real `sh` on a pseudo-terminal,
//! typed into, read by cursor, resized and hung up, and the routes the
//! window uses.
use base64::Engine as _;
use serde_json::{json, Value};
use shadowcode_core::{
    config::Config,
    paths::AppPaths,
    service::{Request, Service},
    terminal::{OpenOptions, Terminals},
};
use std::{
    path::Path,
    time::{Duration, Instant},
};
use tokio::sync::broadcast;

fn sh() -> OpenOptions {
    OpenOptions {
        cols: 80,
        rows: 24,
        shell: Some("/bin/sh".into()),
        login: false,
    }
}

/// Read from `cursor` until the output contains `needle`.
fn read_until(hub: &Terminals, id: &str, cursor: &mut u64, needle: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut seen = String::new();
    while Instant::now() < deadline {
        let chunk = hub.read(id, *cursor).unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(chunk["data"].as_str().unwrap())
            .unwrap();
        seen.push_str(&String::from_utf8_lossy(&bytes));
        *cursor = chunk["cursor"].as_u64().unwrap();
        if seen.contains(needle) {
            return seen;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("terminal never printed {needle:?}; saw {seen:?}");
}

fn alive(pid: i32) -> bool {
    // A reaped or never-started process has no /proc entry; a zombie is
    // gone for our purposes.
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .map(|stat| {
            let state = stat[stat.rfind(')').unwrap() + 2..].chars().next();
            state != Some('Z')
        })
        .unwrap_or(false)
}

fn wait_gone(pid: i32) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while alive(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!alive(pid), "process {pid} outlived its terminal");
}

fn pid_from(dir: &Path, name: &str) -> i32 {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(text) = std::fs::read_to_string(dir.join(name)) {
            if let Ok(pid) = text.trim().parse() {
                return pid;
            }
        }
        assert!(Instant::now() < deadline, "{name} was never written");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn shell_runs_is_resized_and_hangs_up_with_its_jobs() {
    let dir = tempfile::tempdir().unwrap();
    let (sender, mut wakeups) = broadcast::channel(4096);
    let hub = Terminals::new(sender);
    let opened = hub.open(dir.path(), sh()).unwrap();
    let id = opened["id"].as_str().unwrap().to_owned();
    assert_eq!(opened["title"], "Terminal 1");
    assert_eq!(opened["exited"], false);

    let mut cursor = 0;
    hub.write(&id, b"pwd; echo ready-$((40+2))\n").unwrap();
    let text = read_until(&hub, &id, &mut cursor, "ready-42");
    // It started in the project folder.
    assert!(text.contains(&*dir.path().file_name().unwrap().to_string_lossy()));

    // Wake-ups name the terminal and never carry its output.
    let wake = wakeups.try_recv().unwrap();
    assert_eq!(wake["type"], "terminal.output");
    assert_eq!(wake["terminal_id"], id.as_str());
    assert!(wake.get("payload").is_none() && !wake.to_string().contains("ready"));

    // The shell sees the new size.
    let size = hub.resize(&id, 100, 30).unwrap();
    assert_eq!(size, json!({"cols": 100, "rows": 30}));
    hub.write(&id, b"stty size\n").unwrap();
    read_until(&hub, &id, &mut cursor, "30 100");

    // A replay from the start returns everything kept so far.
    let replay = hub.read(&id, 0).unwrap();
    assert_eq!(replay["from"], 0);
    assert_eq!(replay["truncated"], false);

    // A background job and the shell itself go when the terminal closes.
    hub.write(&id, b"echo $$ > shell.pid; sleep 600 & echo $! > job.pid\n")
        .unwrap();
    let shell = pid_from(dir.path(), "shell.pid");
    let job = pid_from(dir.path(), "job.pid");
    assert!(alive(shell) && alive(job));
    assert_eq!(hub.list(Some(dir.path())).unwrap().len(), 1);
    hub.close(&id).unwrap();
    assert!(hub.list(None).unwrap().is_empty());
    assert!(hub.read(&id, 0).is_err());
    wait_gone(shell);
    wait_gone(job);
}

#[test]
fn exited_shells_report_their_status_and_numbers_are_reused() {
    let dir = tempfile::tempdir().unwrap();
    let (sender, _) = broadcast::channel(4096);
    let hub = Terminals::new(sender);
    let first = hub.open(dir.path(), sh()).unwrap();
    let second = hub.open(dir.path(), sh()).unwrap();
    assert_eq!(second["title"], "Terminal 2");
    let id = first["id"].as_str().unwrap();
    hub.write(id, b"exit 3\n").unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let state = hub.read(id, 0).unwrap();
        if state["exited"] == true {
            assert_eq!(state["exit_code"], 3);
            break;
        }
        assert!(Instant::now() < deadline, "shell never exited");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(hub.write(id, b"echo\n").is_err());
    hub.close(id).unwrap();
    let third = hub.open(dir.path(), sh()).unwrap();
    assert_eq!(third["title"], "Terminal 1");
    // Dropping the hub (the window went away) hangs up what is left.
    let pid_dir = dir.path().to_owned();
    hub.write(second["id"].as_str().unwrap(), b"echo $$ > second.pid\n")
        .unwrap();
    let pid = pid_from(&pid_dir, "second.pid");
    drop(hub);
    wait_gone(pid);
}

async fn call(service: &Service, method: &str, path: &str, body: Value) -> anyhow::Result<Value> {
    service
        .dispatch(Request {
            method: method.into(),
            path: path.into(),
            body,
        })
        .await
}

#[tokio::test(flavor = "multi_thread")]
async fn routes_open_type_read_and_close_terminals_per_view() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("project");
    std::fs::create_dir(&workspace).unwrap();
    let paths = AppPaths::isolated(&root.path().join("profile")).unwrap();
    // Untrusted and read-only: terminals are the user's own and need neither.
    Config::patch(&paths, json!({"permissions":{"level":"read_only"}})).unwrap();
    let service = Service::open(paths, Some(workspace.clone())).unwrap();
    let opened = call(
        &service,
        "POST",
        "/api/terminals",
        json!({"cols": 90, "rows": 20}),
    )
    .await
    .unwrap();
    let id = opened["id"].as_str().unwrap().to_owned();
    assert_eq!(opened["cols"], 90);
    let listed = call(&service, "GET", "/api/terminals", Value::Null)
        .await
        .unwrap();
    assert_eq!(listed["terminals"].as_array().unwrap().len(), 1);
    assert_eq!(listed["limits"]["open"], 12);
    call(
        &service,
        "POST",
        &format!("/api/terminals/{id}/input"),
        json!({"data": "echo route-$((6*7))\n"}),
    )
    .await
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut cursor = 0u64;
    let mut seen = String::new();
    while !seen.contains("route-42") {
        assert!(Instant::now() < deadline, "no output: {seen:?}");
        let chunk = call(
            &service,
            "GET",
            &format!("/api/terminals/{id}/output?after={cursor}"),
            Value::Null,
        )
        .await
        .unwrap();
        cursor = chunk["cursor"].as_u64().unwrap();
        seen.push_str(&String::from_utf8_lossy(
            &base64::engine::general_purpose::STANDARD
                .decode(chunk["data"].as_str().unwrap())
                .unwrap(),
        ));
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    call(
        &service,
        "POST",
        &format!("/api/terminals/{id}/resize"),
        json!({"cols": 120, "rows": 40}),
    )
    .await
    .unwrap();
    // Another view (an attached window) has its own, empty set.
    let other = service.fork_selection(workspace.clone(), None).unwrap();
    let theirs = call(&other, "GET", "/api/terminals", Value::Null)
        .await
        .unwrap();
    assert!(theirs["terminals"].as_array().unwrap().is_empty());
    assert!(call(
        &other,
        "GET",
        &format!("/api/terminals/{id}/output"),
        Value::Null
    )
    .await
    .is_err());
    assert!(call(
        &service,
        "GET",
        "/api/terminals/..%2Fetc/output",
        Value::Null
    )
    .await
    .is_err());
    call(
        &service,
        "POST",
        &format!("/api/terminals/{id}/close"),
        json!({}),
    )
    .await
    .unwrap();
    let listed = call(&service, "GET", "/api/terminals", Value::Null)
        .await
        .unwrap();
    assert!(listed["terminals"].as_array().unwrap().is_empty());
}
