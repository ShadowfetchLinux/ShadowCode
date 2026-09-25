//! The window's feed: approvals and jobs in one read, `job.changed`
//! wake-ups for queue changes, and batched diff counts.
use serde_json::{json, Value};
use shadowcode_core::{
    config::Config,
    paths::AppPaths,
    service::{Request, Service},
};
use std::{fs, path::Path, process::Command, time::Duration};

fn setup() -> (tempfile::TempDir, Service) {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("project");
    fs::create_dir(&workspace).unwrap();
    let paths = AppPaths::isolated(&root.path().join("profile")).unwrap();
    Config::patch(&paths, json!({"trusted_workspaces":[workspace],"model":{"provider":"local","name":"fixture","endpoint":"http://127.0.0.1:9/v1"},"permissions":{"approve_shell":false}})).unwrap();
    (root, Service::open(paths, Some(workspace)).unwrap())
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
fn git(project: &Path, args: &[&str]) {
    let result = Command::new("git")
        .args([
            "-c",
            "user.name=Feed Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgSign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .current_dir(project)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[tokio::test]
async fn feed_lists_approvals_jobs_and_the_events_that_change_them() {
    let (_root, service) = setup();
    let feed = call(&service, "GET", "/api/feed?session_id=missing", json!({}))
        .await
        .unwrap();
    assert_eq!(feed["approvals"], json!([]));
    assert!(feed["jobs"].as_array().unwrap().is_empty());
    let events: Vec<_> = feed["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e.as_str().unwrap())
        .collect();
    for kind in ["approval.requested", "approval.resolved", "job.changed"] {
        assert!(events.contains(&kind), "{kind} wakes the feed");
    }

    let mut wakeups = service.engine.subscribe();
    let job = call(
        &service,
        "POST",
        "/api/jobs",
        json!({"task":"Exercise the feed"}),
    )
    .await
    .unwrap();
    let queued = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let event = wakeups.recv().await.unwrap();
            if event["type"] == "job.changed" {
                break event;
            }
        }
    })
    .await
    .expect("a new job wakes the feed");
    assert_eq!(queued["session_id"], job["session_id"]);
    assert_eq!(queued["payload"]["job_id"], job["id"]);
    assert_eq!(queued["payload"]["status"], "queued");
    assert!(queued.get("id").is_none(), "Wake-ups are not stored rows");
    let feed = call(&service, "GET", "/api/feed", json!({})).await.unwrap();
    assert!(feed["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["id"] == job["id"]));
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if wakeups.recv().await.unwrap()["type"] == "agent.completed" {
                break;
            }
        }
    })
    .await
    .expect("the failing fixture job finishes");
    service.engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn diff_counts_match_per_file_diffs_in_one_request() {
    let (_root, service) = setup();
    let project = service.workspace().unwrap();
    git(&project, &["init", "-q"]);
    fs::write(project.join("sample.txt"), "one\ntwo\nthree\n").unwrap();
    fs::write(project.join("clean.txt"), "same\n").unwrap();
    git(&project, &["add", "."]);
    git(&project, &["commit", "-qm", "Fixture"]);
    fs::write(project.join("sample.txt"), "one\n2\nthree\nfour\n").unwrap();
    fs::write(project.join("staged.txt"), "a\nb\n").unwrap();
    git(&project, &["add", "staged.txt"]);
    fs::write(project.join("staged.txt"), "a\nb\nc\n").unwrap();
    fs::write(project.join("new.txt"), "first\nsecond").unwrap();
    fs::write(project.join("blob.bin"), [0u8, 1, 2]).unwrap();
    let absolute = project.join("sample.txt").to_string_lossy().into_owned();
    let stats = call(
        &service,
        "POST",
        "/api/workspace/diffstat",
        json!({"paths":["sample.txt", "staged.txt", "new.txt", "blob.bin", "clean.txt", absolute]}),
    )
    .await
    .unwrap()["stats"]
        .clone();
    assert_eq!(stats["sample.txt"], json!({"add": 2, "del": 1}));
    assert_eq!(stats[&absolute], json!({"add": 2, "del": 1}));
    // Staged (2 lines) plus unstaged (1 line), as the per-file diff counts.
    assert_eq!(stats["staged.txt"], json!({"add": 3, "del": 0}));
    assert_eq!(stats["new.txt"], json!({"add": 2, "del": 0}));
    assert_eq!(stats["blob.bin"], Value::Null);
    assert_eq!(stats["clean.txt"], json!({"add": 0, "del": 0}));

    let per_file = call(
        &service,
        "GET",
        "/api/workspace/diff?path=sample.txt",
        json!({}),
    )
    .await
    .unwrap();
    let lines = |kind: &str| {
        per_file["hunks"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|h| h["lines"].as_array().unwrap())
            .filter(|l| l["kind"] == kind)
            .count()
    };
    assert_eq!((lines("add"), lines("del")), (2, 1));

    for bad in [
        json!({"paths":["../outside.txt"]}),
        json!({"paths":["."]}),
        json!({}),
    ] {
        assert!(call(&service, "POST", "/api/workspace/diffstat", bad)
            .await
            .is_err());
    }
    let many: Vec<_> = (0..201).map(|i| format!("f{i}.txt")).collect();
    assert!(call(
        &service,
        "POST",
        "/api/workspace/diffstat",
        json!({"paths": many})
    )
    .await
    .is_err());
    let empty = call(
        &service,
        "POST",
        "/api/workspace/diffstat",
        json!({"paths":[]}),
    )
    .await
    .unwrap();
    assert_eq!(empty, json!({"stats": {}}));
}

#[tokio::test]
async fn diff_counts_outside_git_are_unknown() {
    let (_root, service) = setup();
    fs::write(service.workspace().unwrap().join("a.txt"), "x\n").unwrap();
    let stats = call(
        &service,
        "POST",
        "/api/workspace/diffstat",
        json!({"paths":["a.txt"]}),
    )
    .await
    .unwrap();
    assert_eq!(stats, json!({"stats": {"a.txt": null}}));
}
