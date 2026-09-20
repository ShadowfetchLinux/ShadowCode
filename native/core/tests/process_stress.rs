use shadowcode_core::process::{run, ProcessSpec};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn output_flood_is_bounded_without_pipe_deadlock() {
    let root = tempfile::tempdir().unwrap();
    let mut spec = ProcessSpec::shell(
        "yes output | head -c 8000000; yes error | head -c 8000000 >&2",
        root.path().into(),
        Duration::from_secs(10),
    );
    spec.output_limit = 8192;
    let result = run(spec, CancellationToken::new(), None).await.unwrap();
    assert!(result.ok);
    assert!(result.truncated);
    assert!(result.stdout.len() + result.stderr.len() <= 8192);
}

#[tokio::test]
async fn timeout_kills_shell_and_grandchildren() {
    let root = tempfile::tempdir().unwrap();
    let spec = ProcessSpec::shell(
        "sleep 60 & echo $! > child.pid; wait",
        root.path().into(),
        Duration::from_millis(150),
    );
    let start = Instant::now();
    let result = run(spec, CancellationToken::new(), None).await.unwrap();
    assert!(result.timed_out);
    assert!(!result.ok);
    assert!(start.elapsed() < Duration::from_secs(3));
    let pid = std::fs::read_to_string(root.path().join("child.pid")).unwrap();
    let status =
        std::fs::read_to_string(format!("/proc/{}/status", pid.trim())).unwrap_or_default();
    assert!(
        status.is_empty() || status.contains("State:\tZ"),
        "Grandchild remained running"
    );
}

#[tokio::test]
async fn cancellation_stops_active_execution_and_honors_pre_cancel() {
    let root = tempfile::tempdir().unwrap();
    let cancel = CancellationToken::new();
    let later = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        later.cancel();
    });
    let result = run(
        ProcessSpec::shell("sleep 60", root.path().into(), Duration::from_secs(30)),
        cancel.clone(),
        None,
    )
    .await
    .unwrap();
    assert!(result.cancelled);
    assert!(!result.ok);
    assert!(run(
        ProcessSpec::shell(
            "touch should-not-exist",
            root.path().into(),
            Duration::from_secs(1)
        ),
        cancel,
        None
    )
    .await
    .is_err());
    assert!(!root.path().join("should-not-exist").exists());
}

#[tokio::test]
async fn dropping_execution_does_not_leave_the_process_group_alive() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().to_path_buf();
    let task = tokio::spawn(run(
        ProcessSpec::shell(
            "echo $$ > shell.pid; sleep 60",
            path.clone(),
            Duration::from_secs(60),
        ),
        CancellationToken::new(),
        None,
    ));
    for _ in 0..100 {
        if path.join("shell.pid").exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let pid = std::fs::read_to_string(path.join("shell.pid")).unwrap();
    task.abort();
    let _ = task.await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let status =
        std::fs::read_to_string(format!("/proc/{}/status", pid.trim())).unwrap_or_default();
    assert!(status.is_empty() || status.contains("State:\tZ"));
}

#[tokio::test]
async fn a_detached_pipe_cannot_hang_completion() {
    let root = tempfile::tempdir().unwrap();
    let result = run(
        ProcessSpec::shell(
            "setsid sh -c 'echo $$ > escaped.pid; sleep 8' & sleep .1; printf done",
            root.path().into(),
            Duration::from_secs(3),
        ),
        CancellationToken::new(),
        None,
    )
    .await
    .unwrap();
    if let Ok(pid) = std::fs::read_to_string(root.path().join("escaped.pid")) {
        let pid: i32 = pid.trim().parse().unwrap();
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
    assert!(result.ok);
    assert!(result.truncated);
    assert!(result.stdout.contains("done"));
    assert!(result.duration_ms < 3000);
}

#[tokio::test]
async fn repeated_concurrent_commands_preserve_outputs() {
    let root = tempfile::tempdir().unwrap();
    let mut tasks = Vec::new();
    for index in 0..32 {
        let spec = ProcessSpec::shell(
            &format!("printf task-{index}"),
            root.path().into(),
            Duration::from_secs(3),
        );
        tasks.push((
            index,
            tokio::spawn(run(spec, CancellationToken::new(), None)),
        ));
    }
    for (index, task) in tasks {
        let result = task.await.unwrap().unwrap();
        assert!(result.ok);
        assert_eq!(result.stdout, format!("task-{index}"));
    }
}
