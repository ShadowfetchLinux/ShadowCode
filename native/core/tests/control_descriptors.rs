//! Process-wide `/proc/self/fd` counts. This file is a separate integration
//! binary so sibling `control` tests cannot inflate the measurement.
use serde_json::json;
use shadowcode_core::{config::Config, control::Server, paths::AppPaths, service::Service};
use std::fs;

fn setup() -> (tempfile::TempDir, Service) {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("project");
    fs::create_dir(&workspace).unwrap();
    let paths = AppPaths::isolated(&root.path().join("profile")).unwrap();
    Config::patch(&paths,json!({"trusted_workspaces":[workspace],"model":{"provider":"local","name":"fixture","endpoint":"http://127.0.0.1:9/v1"},"permissions":{"approve_shell":false}})).unwrap();
    (root, Service::open(paths, Some(workspace)).unwrap())
}

async fn stop(server: Server, service: Service) {
    server.close();
    server.wait_closed().await;
    service.engine.shutdown().await.unwrap();
}

fn descriptor_count() -> usize {
    std::fs::read_dir("/proc/self/fd")
        .map(|entries| entries.count())
        .unwrap_or(0)
}

#[tokio::test]
async fn attach_close_loop_does_not_grow_descriptors() {
    let (_root, service) = setup();
    let server = Server::start_with_mode(service.clone(), "server").unwrap();
    let client = server.endpoint().client(service.workspace().unwrap(), None);
    for _ in 0..2 {
        let view = client.open_view().await.unwrap();
        view.close().await.unwrap();
    }
    let baseline = descriptor_count();
    for _ in 0..20 {
        let view = client.open_view().await.unwrap();
        view.close().await.unwrap();
    }
    let after = descriptor_count();
    eprintln!("attach_close_loop descriptors baseline={baseline} after={after}");
    assert!(
        after <= baseline + 24,
        "descriptor leak: baseline={baseline} after={after}"
    );
    stop(server, service).await;
}
