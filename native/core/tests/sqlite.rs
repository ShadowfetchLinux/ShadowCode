#![cfg(unix)]
use rusqlite::Connection;
use serde_json::{json, Value};
use shadowcode_core::{
    sqlite::{self, Request},
    workspace::Workspace,
};
use std::{
    fs,
    os::unix::fs::{symlink, PermissionsExt},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

fn fixture() -> (tempfile::TempDir, Arc<Workspace>) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("project");
    fs::create_dir(&path).unwrap();
    let db = Connection::open(path.join("data.db")).unwrap();
    db.execute_batch("CREATE TABLE items(id INTEGER PRIMARY KEY, label TEXT, optional TEXT, payload BLOB); INSERT INTO items VALUES(1,'naïve ; -- /* ? #',NULL,x'00ff'); INSERT INTO items VALUES(2,'second','value',x'');").unwrap();
    drop(db);
    (root, Arc::new(Workspace::open(&path).unwrap()))
}
fn request(value: Value) -> Request {
    serde_json::from_value(value).unwrap()
}
async fn read(workspace: &Arc<Workspace>, value: Value) -> anyhow::Result<Value> {
    sqlite::inspect(workspace.clone(), request(value), CancellationToken::new()).await
}

#[tokio::test]
async fn native_queries_preserve_types_parameters_and_database_bytes() {
    let (_root, ws) = fixture();
    let before = fs::read(ws.path.join("data.db")).unwrap();
    fs::set_permissions(ws.path.join("data.db"), fs::Permissions::from_mode(0o444)).unwrap();
    assert_eq!(
        read(&ws, json!({"path":"data.db"})).await.unwrap()["tables"],
        json!(["items"])
    );
    let rows = read(&ws,json!({"path":"data.db","sql":"/* leading comment */ WITH selected AS (SELECT * FROM items WHERE id=?) SELECT id,label,optional,payload,? AS truth FROM selected; -- trailing","params":[1,true]})).await.unwrap();
    assert_eq!(
        rows["rows"][0],
        json!({"id":1,"label":"naïve ; -- /* ? #","optional":null,"payload":{"type":"blob","hex":"00ff","bytes":2},"truth":1})
    );
    assert_eq!(rows["truncated"], false);
    for sql in [
        "PRAGMA table_info(items)",
        "SELECT name FROM pragma_table_info('items')",
        "PRAGMA table_xinfo(items)",
    ] {
        assert_eq!(
            read(&ws, json!({"path":"data.db","sql":sql}))
                .await
                .unwrap()["rows"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
    }
    assert!(
        read(&ws, json!({"path":"data.db"})).await.unwrap()["rows"][0]["sql"]
            .as_str()
            .unwrap()
            .contains("CREATE TABLE items")
    );
    let injected = "' OR 1=1; DROP TABLE items; --";
    assert!(read(
        &ws,
        json!({"path":"data.db","sql":"SELECT * FROM items WHERE label=?","params":[injected]})
    )
    .await
    .unwrap()["rows"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(fs::read(ws.path.join("data.db")).unwrap(), before);
    assert_eq!(fs::read_dir(&ws.path).unwrap().count(), 1);
    let renamed = "query ? # % ü.db";
    fs::rename(ws.path.join("data.db"), ws.path.join(renamed)).unwrap();
    assert_eq!(
        read(&ws, json!({"path":renamed})).await.unwrap()["tables"],
        json!(["items"])
    );
}

#[tokio::test]
async fn read_only_authorization_rejects_writes_attachments_pragmas_and_statement_chains() {
    let (_root, ws) = fixture();
    let before = fs::read(ws.path.join("data.db")).unwrap();
    for sql in [
        "DELETE FROM items RETURNING id",
        "WITH ids AS (SELECT id FROM items) DELETE FROM items WHERE id IN ids RETURNING id",
        "CREATE TABLE nope(x)",
        "PRAGMA user_version=4",
        "PRAGMA query_only=OFF",
        "VACUUM INTO 'copy.db'",
        "ATTACH ':memory:' AS extra",
        "SELECT load_extension('/tmp/nope')",
        "SELECT writefile('bad','x')",
        "SELECT 1; SELECT 2",
        "SELECT 1; DROP TABLE items",
        "BEGIN",
        "SELECT 1 AS same, 2 AS same",
    ] {
        assert!(
            read(&ws, json!({"path":"data.db","sql":sql}))
                .await
                .is_err(),
            "accepted {sql}"
        );
    }
    let chain = "SELECT 1;".repeat(6000);
    assert!(read(&ws, json!({"path":"data.db","sql":chain}))
        .await
        .is_err());
    assert_eq!(
        read(
            &ws,
            json!({"path":"data.db","sql":"SELECT ';'';--' AS literal; /* ; */ ; -- final\n"})
        )
        .await
        .unwrap()["rows"][0]["literal"],
        ";';--"
    );
    assert!(read(
        &ws,
        json!({"path":"data.db","sql":"SELECT ?","params":[18446744073709551615_u64]})
    )
    .await
    .is_err());
    assert!(read(
        &ws,
        json!({"path":"data.db","sql":"SELECT ?","params":[{}]})
    )
    .await
    .is_err());
    assert_eq!(fs::read(ws.path.join("data.db")).unwrap(), before);
    assert_eq!(fs::read_dir(&ws.path).unwrap().count(), 1);
    assert!(
        rusqlite::version_number() >= 3051003,
        "bundled SQLite must contain the WAL-reset fix"
    );
}

#[tokio::test]
async fn wide_schemas_busy_databases_and_corruption_have_bounded_results() {
    let (_root, ws) = fixture();
    let db = Connection::open(ws.path.join("data.db")).unwrap();
    let columns = (0..300)
        .map(|i| format!("c{i} INTEGER"))
        .collect::<Vec<_>>()
        .join(",");
    db.execute_batch(&format!(
        "CREATE TABLE wide({columns}); INSERT INTO wide(c299) VALUES(42);"
    ))
    .unwrap();
    assert_eq!(
        read(&ws, json!({"path":"data.db"})).await.unwrap()["tables"],
        json!(["items", "wide"])
    );
    assert_eq!(
        read(&ws, json!({"path":"data.db","sql":"SELECT c299 FROM wide"}))
            .await
            .unwrap()["rows"][0]["c299"],
        42
    );
    let wide = read(&ws, json!({"path":"data.db","sql":"SELECT * FROM wide"}))
        .await
        .unwrap_err();
    assert!(format!("{wide:#}").contains("128 columns"));
    db.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let start = Instant::now();
    let busy = read(&ws, json!({"path":"data.db"})).await.unwrap_err();
    assert!(format!("{busy:#}").contains("locked"));
    assert!(start.elapsed() < Duration::from_secs(2));
    db.execute_batch("ROLLBACK").unwrap();
    assert!(read(&ws, json!({"path":"data.db"})).await.is_ok());
    fs::write(ws.path.join("corrupt.db"), b"not a SQLite database").unwrap();
    assert!(read(&ws, json!({"path":"corrupt.db"})).await.is_err());
    assert_eq!(
        fs::read(ws.path.join("corrupt.db")).unwrap(),
        b"not a SQLite database"
    );
}

#[tokio::test]
async fn database_paths_and_sidecars_are_confined_without_creating_missing_files() {
    let (root, ws) = fixture();
    let outside = root.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::copy(ws.path.join("data.db"), outside.join("secret.db")).unwrap();
    symlink(&outside, ws.path.join("escape")).unwrap();
    symlink(outside.join("secret.db"), ws.path.join("link.db")).unwrap();
    for path in [
        "missing.db",
        "../outside/secret.db",
        "escape/secret.db",
        "link.db",
        ".",
        "file:data.db?mode=rw",
    ] {
        assert!(
            read(&ws, json!({"path":path})).await.is_err(),
            "accepted {path}"
        );
    }
    for suffix in ["-wal", "-shm", "-journal"] {
        let sidecar = ws.path.join(format!("data.db{suffix}"));
        symlink(outside.join("secret.db"), &sidecar).unwrap();
        assert!(read(&ws, json!({"path":"data.db"})).await.is_err());
        fs::remove_file(sidecar).unwrap();
    }
    let fifo = ws.path.join("blocked.db");
    let name = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    assert!(tokio::time::timeout(
        Duration::from_secs(2),
        read(&ws, json!({"path":"blocked.db"}))
    )
    .await
    .unwrap()
    .is_err());
    assert!(!ws.path.join("missing.db").exists());
    // A held workspace still refers to its original directory after a rename;
    // replacing the ambient path with a link must not redirect the database.
    fs::rename(&ws.path, root.path().join("moved")).unwrap();
    symlink(&outside, &ws.path).unwrap();
    assert_eq!(
        read(&ws, json!({"path":"data.db"})).await.unwrap()["tables"],
        json!(["items"])
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_wal_commits_from_another_process_are_visible_without_database_changes() {
    use std::io::{BufRead, BufReader, Write};
    let (_root, ws) = fixture();
    let path = ws.path.join("live.db");
    // python3 + stdlib sqlite3: Ubuntu /usr/bin/node is often 18 and has no
    // node:sqlite, which made this look like a parallel flake on sanitized PATH.
    let mut child = std::process::Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/sqlite-writer.py"
        ))
        .arg(&path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("python3 must be on PATH to drive the live-WAL writer");
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut ready = String::new();
    output.read_line(&mut ready).unwrap();
    assert_eq!(ready.trim_end(), "ready", "writer stderr: {}", {
        let mut err = String::new();
        let _ = child
            .stderr
            .as_mut()
            .map(|stderr| std::io::Read::read_to_string(stderr, &mut err));
        err
    });
    let contents =
        || ["", "-wal"].map(|suffix| fs::read(ws.path.join(format!("live.db{suffix}"))).unwrap());
    let before = contents();
    assert_eq!(
        read(
            &ws,
            json!({"path":"live.db","sql":"SELECT sum(value) AS total FROM live"})
        )
        .await
        .unwrap()["rows"][0]["total"],
        1
    );
    assert_eq!(contents(), before);
    fs::copy(&path, ws.path.join("orphan.db")).unwrap();
    fs::copy(ws.path.join("live.db-wal"), ws.path.join("orphan.db-wal")).unwrap();
    assert_eq!(
        read(&ws, json!({"path":"orphan.db"})).await.unwrap()["tables"],
        json!(["live"])
    );
    assert_eq!(fs::read(ws.path.join("orphan.db-wal")).unwrap(), before[1]);
    assert_eq!(fs::read(ws.path.join("orphan.db")).unwrap(), before[0]);
    writeln!(input, "next").unwrap();
    let mut updated = String::new();
    output.read_line(&mut updated).unwrap();
    assert_eq!(updated.trim_end(), "updated");
    let before = contents();
    assert_eq!(
        read(
            &ws,
            json!({"path":"live.db","sql":"SELECT sum(value) AS total FROM live"})
        )
        .await
        .unwrap()["rows"][0]["total"],
        3
    );
    assert_eq!(contents(), before);
    writeln!(input, "stop").unwrap();
    drop(input);
    assert!(child.wait().unwrap().success());
    assert_eq!(
        read(
            &ws,
            json!({"path":"live.db","sql":"SELECT sum(value) AS total FROM live"})
        )
        .await
        .unwrap()["rows"][0]["total"],
        3
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queries_bound_rows_bytes_work_time_and_cancel_then_release_locks() {
    let (_root, ws) = fixture();
    let truncated = read(
        &ws,
        json!({"path":"data.db","sql":"SELECT * FROM items ORDER BY id","limit":1}),
    )
    .await
    .unwrap();
    assert_eq!(truncated["rows"].as_array().unwrap().len(), 1);
    assert_eq!(truncated["truncated"], true);
    assert_eq!(
        read(
            &ws,
            json!({"path":"data.db","sql":"SELECT * FROM items ORDER BY id","limit":2})
        )
        .await
        .unwrap()["truncated"],
        false
    );
    let large = read(&ws,json!({"path":"data.db","sql":"WITH RECURSIVE seq(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM seq WHERE n<100) SELECT printf('%0100000d',n) AS content FROM seq"})).await.unwrap();
    assert_eq!(large["truncated"], true);
    assert!(serde_json::to_vec(&large).unwrap().len() <= 1_000_000);
    assert!(read(
        &ws,
        json!({"path":"data.db","sql":"SELECT randomblob(1000000000)"})
    )
    .await
    .is_err());
    let heavy = "WITH RECURSIVE seq(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM seq WHERE n<1000000000) SELECT sum(n) FROM seq";
    let work_start = Instant::now();
    assert!(read(
        &ws,
        json!({"path":"data.db","sql":heavy,"timeout_ms":10000})
    )
    .await
    .is_err());
    assert!(
        work_start.elapsed() < Duration::from_secs(3),
        "VM work budget was not enforced"
    );
    let start = Instant::now();
    let timeout = read(&ws, json!({"path":"data.db","sql":heavy,"timeout_ms":1}))
        .await
        .unwrap_err();
    assert!(format!("{timeout:#}").contains("time limit"));
    assert!(start.elapsed() < Duration::from_secs(2));
    let cancel = CancellationToken::new();
    let worker = tokio::spawn(sqlite::inspect(
        ws.clone(),
        request(json!({"path":"data.db","sql":heavy,"timeout_ms":10000})),
        cancel.clone(),
    ));
    tokio::time::sleep(Duration::from_millis(5)).await;
    cancel.cancel();
    assert!(format!("{:#}", worker.await.unwrap().unwrap_err()).contains("cancelled"));
    let worker = tokio::spawn(sqlite::inspect(
        ws.clone(),
        request(json!({"path":"data.db","sql":heavy,"timeout_ms":10000})),
        CancellationToken::new(),
    ));
    tokio::time::sleep(Duration::from_millis(5)).await;
    worker.abort();
    let _ = worker.await;
    let db = Connection::open(ws.path.join("data.db")).unwrap();
    db.busy_timeout(Duration::from_secs(2)).unwrap();
    db.execute_batch(
        "BEGIN EXCLUSIVE; INSERT INTO items VALUES(3,'after cancellation',NULL,NULL); COMMIT",
    )
    .unwrap();
    assert_eq!(
        read(
            &ws,
            json!({"path":"data.db","sql":"SELECT count(*) AS total FROM items"})
        )
        .await
        .unwrap()["rows"][0]["total"],
        3
    );
    assert!(read(&ws, json!({"path":"data.db","limit":1001}))
        .await
        .is_err());
    assert!(read(&ws, json!({"path":"data.db","timeout_ms":10001}))
        .await
        .is_err());
}
