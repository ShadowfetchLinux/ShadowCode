//! Bounded, read-only inspection of workspace databases. Uses the same bundled
//! SQLite as native persistence; no Python, extension loader or shell process.
mod vfs;
use crate::workspace::Workspace;
use anyhow::{bail, ensure, Context, Result};
use rusqlite::{
    config::DbConfig,
    hooks::{AuthAction, AuthContext, Authorization},
    limits::Limit,
    types::{Value as SqlValue, ValueRef},
    Connection, OpenFlags,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::{
    collections::HashSet,
    os::fd::AsRawFd,
    sync::{Arc, LazyLock},
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

const MAX_SQL: usize = 64_000;
const MAX_OUTPUT: usize = 1_000_000;
const MAX_OPS: usize = 10_000_000;
const DEFAULT_ROWS: usize = 200;
static READERS: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(8)));

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub path: String,
    #[serde(default)]
    pub sql: Option<String>,
    #[serde(default)]
    pub params: Vec<Value>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}
fn default_limit() -> usize {
    DEFAULT_ROWS
}
fn default_timeout() -> u64 {
    5000
}

pub async fn inspect(
    workspace: Arc<Workspace>,
    request: Request,
    cancel: CancellationToken,
) -> Result<Value> {
    ensure!(
        !request.path.is_empty() && request.path.len() <= 4096,
        "Database path is required (at most 4096 bytes)"
    );
    ensure!(
        (1..=1000).contains(&request.limit),
        "SQLite row limit must be between 1 and 1000"
    );
    ensure!(
        (1..=10000).contains(&request.timeout_ms),
        "SQLite timeout must be between 1 and 10000 ms"
    );
    ensure!(
        request.params.len() <= 128 && serde_json::to_vec(&request.params)?.len() <= MAX_SQL,
        "SQLite parameters exceed the limit"
    );
    if let Some(sql) = &request.sql {
        ensure!(
            !sql.trim().is_empty() && sql.len() <= MAX_SQL && !sql.contains('\0'),
            "Supply one read-only SQL statement of at most 64 KB, without NUL bytes"
        );
    } else {
        ensure!(
            request.params.is_empty(),
            "Table listing does not take SQL parameters"
        );
    }
    let slot = READERS
        .clone()
        .try_acquire_owned()
        .context("Eight SQLite inspections are already running; retry when one completes")?;
    let cancel = cancel.child_token();
    let _cancel_on_drop = cancel.clone().drop_guard();
    // A cancelled caller drops the guard; the blocking worker retains its permit
    // until its progress handler stops and its SQLite connection closes.
    tokio::task::spawn_blocking(move || {
        let _slot = slot;
        query(&workspace, &request, &cancel)
    })
    .await
    .context("SQLite worker stopped unexpectedly")?
}

fn allowed(context: AuthContext<'_>) -> Authorization {
    match context.action {
        AuthAction::Select | AuthAction::Read { .. } | AuthAction::Recursive => {
            Authorization::Allow
        }
        AuthAction::Pragma { pragma_name, .. }
            if matches!(
                pragma_name.to_ascii_lowercase().as_str(),
                "table_info"
                    | "table_xinfo"
                    | "index_list"
                    | "index_info"
                    | "index_xinfo"
                    | "foreign_key_list"
                    | "table_list"
            ) =>
        {
            Authorization::Allow
        }
        AuthAction::Function { function_name }
            if !matches!(
                function_name.to_ascii_lowercase().as_str(),
                "load_extension" | "writefile" | "readfile" | "fts3_tokenizer"
            ) =>
        {
            Authorization::Allow
        }
        _ => Authorization::Deny,
    }
}

fn parameter(value: &Value) -> Result<SqlValue> {
    Ok(match value {
        Value::Null => SqlValue::Null,
        Value::Bool(v) => SqlValue::Integer(i64::from(*v)),
        Value::Number(v) if v.is_i64() => SqlValue::Integer(v.as_i64().unwrap()),
        Value::Number(v) if v.is_f64() => SqlValue::Real(v.as_f64().unwrap()),
        Value::String(v) => SqlValue::Text(v.clone()),
        _ => bail!(
            "SQL parameters must be null, booleans, signed 64-bit integers, finite numbers or text"
        ),
    })
}

// rusqlite's ordinary prepare checks the SQL tail recursively. Validate at most
// two statements through SQLite itself first, so a hostile 64 KB chain cannot
// exhaust the Rust stack. No SQL is executed here; every raw statement is
// finalized before returning and before the connection is borrowed again.
fn single_statement(connection: &Connection, sql: &str) -> Result<usize> {
    use rusqlite::ffi;
    use std::ffi::{CStr, CString};
    let input = CString::new(sql)?;
    let mut start = input.as_ptr();
    let mut length = sql.len();
    for index in 0..2 {
        let mut statement = std::ptr::null_mut();
        let mut tail = std::ptr::null();
        let code = unsafe {
            ffi::sqlite3_prepare_v3(connection.handle(), start, -1, 0, &mut statement, &mut tail)
        };
        let present = !statement.is_null();
        if present {
            unsafe {
                ffi::sqlite3_finalize(statement);
            }
        }
        if code != ffi::SQLITE_OK {
            let message = unsafe { CStr::from_ptr(ffi::sqlite3_errmsg(connection.handle())) }
                .to_string_lossy()
                .into_owned();
            return Err(
                rusqlite::Error::SqliteFailure(ffi::Error::new(code), Some(message)).into(),
            );
        }
        if index == 1 {
            ensure!(!present, "Only one read-only SQL statement is allowed");
            break;
        }
        ensure!(present, "Supply a read-only SQL query");
        length = unsafe { tail.offset_from(input.as_ptr()) } as usize;
        start = tail;
    }
    Ok(length)
}

fn query(workspace: &Workspace, request: &Request, cancel: &CancellationToken) -> Result<Value> {
    ensure!(!cancel.is_cancelled(), "SQLite inspection cancelled");
    let deadline = Instant::now() + Duration::from_millis(request.timeout_ms);
    let result = query_inner(workspace, request, cancel, deadline);
    if cancel.is_cancelled() {
        bail!("SQLite inspection cancelled");
    }
    if Instant::now() >= deadline {
        bail!("SQLite query exceeded its time limit");
    }
    result.context("SQLite inspection failed (queries are read-only and bounded; SQLite must be able to access WAL coordination files)")
}

fn query_inner(
    workspace: &Workspace,
    request: &Request,
    cancel: &CancellationToken,
    deadline: Instant,
) -> Result<Value> {
    let (parent, name) = workspace.confined_parent(&request.path)?;
    // Inspect every existing sidecar without opening a FIFO or following links.
    for suffix in ["", "-wal", "-shm", "-journal"] {
        match parent.symlink_metadata(format!("{name}{suffix}")) {
            Ok(metadata) => {
                use cap_std::fs::MetadataExt;
                ensure!(
                    metadata.is_file(),
                    "Database and sidecars must be regular files, not symlinks or special files"
                );
                ensure!(
                    suffix.is_empty() || metadata.nlink() == 1,
                    "Database sidecars must not have additional hard links"
                );
            }
            Err(e) if !suffix.is_empty() && e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => {
                return Err(e).context(
                    "Database file is missing or inaccessible; inspection never creates it",
                )
            }
        }
    }
    vfs::register()?;
    let path = format!("/proc/self/fd/{}/{}", parent.as_raw_fd(), name);
    let mut uri = reqwest::Url::from_file_path(&path)
        .map_err(|_| anyhow::anyhow!("Invalid database path"))?;
    uri.query_pairs_mut().append_pair("mode", "ro");
    // `parent` outlives this connection. Do not use immutable=1: live WAL
    // writers must retain SQLite's normal transaction and locking semantics.
    let connection = Connection::open_with_flags_and_vfs(
        uri.as_str(),
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        vfs::NAME,
    )
    .context("Cannot open this project database read-only")?;
    let finished = CancellationToken::new();
    let _finish_guard = finished.clone().drop_guard();
    let interrupt = connection.get_interrupt_handle();
    let interrupted = cancel.clone();
    tokio::spawn(async move {
        tokio::select! {
            biased;
            _ = finished.cancelled() => (),
            _ = interrupted.cancelled() => interrupt.interrupt(),
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => interrupt.interrupt(),
        }
    });
    connection.busy_timeout(Duration::from_millis(request.timeout_ms.min(100)))?;
    connection.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)?;
    connection.set_db_config(DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA, false)?;
    connection.set_db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_TRIGGER, false)?;
    for (limit, value) in [
        (Limit::SQLITE_LIMIT_LENGTH, MAX_OUTPUT as i32),
        (Limit::SQLITE_LIMIT_SQL_LENGTH, MAX_SQL as i32),
        // This also limits existing table definitions; cap result columns below.
        (Limit::SQLITE_LIMIT_COLUMN, 2000),
        (Limit::SQLITE_LIMIT_EXPR_DEPTH, 100),
        (Limit::SQLITE_LIMIT_COMPOUND_SELECT, 20),
        (Limit::SQLITE_LIMIT_VDBE_OP, 100_000),
        (Limit::SQLITE_LIMIT_FUNCTION_ARG, 32),
        (Limit::SQLITE_LIMIT_ATTACHED, 0),
        (Limit::SQLITE_LIMIT_VARIABLE_NUMBER, 128),
        (Limit::SQLITE_LIMIT_LIKE_PATTERN_LENGTH, 1000),
        (Limit::SQLITE_LIMIT_WORKER_THREADS, 0),
    ] {
        connection.set_limit(limit, value)?;
    }
    let ct = cancel.clone();
    let mut operations = 0;
    connection.progress_handler(
        1000,
        Some(move || {
            operations += 1000;
            ct.is_cancelled() || Instant::now() >= deadline || operations > MAX_OPS
        }),
    )?;
    // Allow SQLite to spill instead of keeping an unbounded sort/recursive table
    // in RAM. The private VFS rejects temporary disk files, so oversized working
    // sets fail with an error; they never create files in the project or /tmp.
    connection.execute_batch("PRAGMA query_only=ON; PRAGMA temp_store=FILE; PRAGMA cache_size=-2048; PRAGMA mmap_size=0;")?;
    connection.authorizer(Some(allowed))?;
    let sql = request
        .sql
        .as_deref()
        .unwrap_or("SELECT name, sql FROM sqlite_schema WHERE type='table' ORDER BY name");
    let run = || -> Result<Value> {
        let length = single_statement(&connection, sql)?;
        let mut statement = connection
            .prepare(&sql[..length])
            .context("Only one read-only SELECT/WITH or schema PRAGMA is permitted")?;
        ensure!(
            statement.readonly() && statement.column_count() > 0,
            "Only a read-only query returning columns is permitted"
        );
        ensure!(
            statement.column_count() <= 128,
            "Query returns more than 128 columns; select an explicit subset"
        );
        let columns: Vec<String> = statement
            .column_names()
            .iter()
            .map(|v| (*v).to_owned())
            .collect();
        ensure!(
            columns.iter().collect::<HashSet<_>>().len() == columns.len(),
            "Duplicate result column names; use AS aliases to keep every value"
        );
        let parameters = request
            .params
            .iter()
            .map(parameter)
            .collect::<Result<Vec<_>>>()?;
        let mut cursor = statement.query(rusqlite::params_from_iter(parameters))?;
        let mut rows = Vec::new();
        let mut bytes = serde_json::to_vec(&columns)?.len() + request.path.len() + 1024;
        let mut truncated = false;
        while let Some(row) = cursor.next()? {
            ensure!(!cancel.is_cancelled(), "SQLite inspection cancelled");
            ensure!(
                Instant::now() < deadline,
                "SQLite query exceeded its time limit"
            );
            if rows.len() == request.limit {
                truncated = true;
                break;
            }
            let mut object = Map::new();
            for (index, column) in columns.iter().enumerate() {
                let value = match row.get_ref(index)? {
                    ValueRef::Null => Value::Null,
                    ValueRef::Integer(v) => json!(v),
                    ValueRef::Real(v) => {
                        ensure!(
                            v.is_finite(),
                            "A non-finite SQLite number cannot be represented in JSON"
                        );
                        json!(v)
                    }
                    ValueRef::Text(v) => json!(std::str::from_utf8(v)
                        .context("Result contains invalid UTF-8; select hex(column) instead")?),
                    ValueRef::Blob(v) => {
                        json!({"type":"blob","hex":v.iter().map(|b| format!("{b:02x}")).collect::<String>(),"bytes":v.len()})
                    }
                };
                object.insert(column.clone(), value);
            }
            let size = serde_json::to_vec(&object)?.len()
                + 1
                + if request.sql.is_none() {
                    serde_json::to_vec(&object["name"])?.len() + 1
                } else {
                    0
                };
            if bytes + size > MAX_OUTPUT {
                truncated = true;
                break;
            }
            bytes += size;
            rows.push(Value::Object(object));
        }
        let mut result = json!({"ok":true,"path":workspace.relative(&request.path)?.to_string_lossy(),"columns":columns,"rows":rows,"truncated":truncated,"limit":request.limit,"read_only":true});
        if request.sql.is_none() {
            result["tables"] = json!(rows
                .iter()
                .map(|row| row["name"].clone())
                .collect::<Vec<_>>());
        }
        ensure!(
            serde_json::to_vec(&result)?.len() <= MAX_OUTPUT,
            "Result metadata exceeds the 1 MB limit; narrow the query"
        );
        Ok(result)
    };
    run()
}
