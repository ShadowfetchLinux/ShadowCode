use crate::{id, now};
use anyhow::{ensure, Context, Result};
use rusqlite::{params, types::ValueRef, Connection, OptionalExtension, Params};
use serde_json::{json, Map, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    time::Duration,
};

mod background;
mod goals;
mod memory;
pub use goals::MilestoneSpec;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
 id TEXT PRIMARY KEY, workspace TEXT NOT NULL, created_at REAL NOT NULL,
 updated_at REAL NOT NULL, model_id TEXT, status TEXT NOT NULL,
 title TEXT, usage_json TEXT, parent_id TEXT, branched_at REAL
);
CREATE TABLE IF NOT EXISTS tasks (
 id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(id),
 prompt TEXT NOT NULL, status TEXT NOT NULL, summary TEXT,
 created_at REAL NOT NULL, completed_at REAL, usage_json TEXT
);
CREATE TABLE IF NOT EXISTS events (
 id INTEGER PRIMARY KEY AUTOINCREMENT, ts REAL NOT NULL, type TEXT NOT NULL,
 session_id TEXT, task_id TEXT, payload TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS events_session_id ON events(session_id,id);
CREATE INDEX IF NOT EXISTS events_task_id ON events(task_id,id);
CREATE TABLE IF NOT EXISTS models (
 id TEXT PRIMARY KEY, name TEXT NOT NULL, provider TEXT NOT NULL,
 endpoint TEXT, context_limit INTEGER, metadata TEXT
);
CREATE TABLE IF NOT EXISTS projects (
 id TEXT PRIMARY KEY, path TEXT UNIQUE NOT NULL, name TEXT NOT NULL, last_opened REAL NOT NULL
);
CREATE TABLE IF NOT EXISTS session_meta (
 session_id TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL, PRIMARY KEY(session_id,key)
);
CREATE TABLE IF NOT EXISTS pins (
 id INTEGER PRIMARY KEY AUTOINCREMENT, session_id TEXT NOT NULL,
 task_id TEXT, ts REAL NOT NULL, label TEXT NOT NULL, body TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS desktop_jobs (id TEXT PRIMARY KEY, payload TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS background_processes (
 id TEXT PRIMARY KEY, workspace TEXT NOT NULL, started_at REAL NOT NULL,
 status TEXT NOT NULL, payload TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS background_workspace ON background_processes(workspace,started_at);
CREATE TABLE IF NOT EXISTS native_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS goals (
 id TEXT PRIMARY KEY, workspace TEXT NOT NULL, instruction TEXT NOT NULL,
 status TEXT NOT NULL, progress REAL NOT NULL, title TEXT,
 created_at REAL NOT NULL, updated_at REAL NOT NULL
);
CREATE TABLE IF NOT EXISTS milestones (
 id TEXT PRIMARY KEY, goal_id TEXT NOT NULL REFERENCES goals(id),
 title TEXT NOT NULL, status TEXT NOT NULL, order_index INTEGER NOT NULL,
 detail TEXT, task_id TEXT, created_at REAL NOT NULL, updated_at REAL NOT NULL
);
CREATE TABLE IF NOT EXISTS goal_runs (
 goal_id TEXT PRIMARY KEY REFERENCES goals(id), session_id TEXT NOT NULL,
 job_id TEXT, status TEXT NOT NULL, detail TEXT NOT NULL, updated_at REAL NOT NULL
);
CREATE TABLE IF NOT EXISTS file_changes (
 id INTEGER PRIMARY KEY AUTOINCREMENT, task_id TEXT NOT NULL,
 workspace TEXT NOT NULL, path TEXT NOT NULL, before_bytes BLOB,
 before_mode INTEGER, after_hash TEXT, observed_hash TEXT, restored INTEGER NOT NULL DEFAULT 0,
 UNIQUE(task_id,workspace,path)
);
CREATE TABLE IF NOT EXISTS job_messages (
 job_id TEXT NOT NULL, ordinal INTEGER NOT NULL, payload TEXT NOT NULL,
 PRIMARY KEY(job_id,ordinal)
);
CREATE TABLE IF NOT EXISTS queued_tasks (
 id TEXT PRIMARY KEY, session_id TEXT NOT NULL, workspace TEXT NOT NULL,
 payload TEXT NOT NULL, status TEXT NOT NULL, created_at REAL NOT NULL
);
CREATE TABLE IF NOT EXISTS task_notes (
 task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
 content TEXT NOT NULL, updated_at REAL NOT NULL
);
"#;

/// Current schema version. Older databases are backed up, then migrated
/// forward one step at a time inside one transaction.
pub const SCHEMA_VERSION: i64 = 25;

/// Version 25: persisted subscription usage snapshots, so the Accounts page
/// and picker can show "Last checked …" before the first refresh. Execution
/// targets and vendor session ids live in `session_meta` / `native_meta`.
const MIGRATION_25: &str = r#"
CREATE TABLE IF NOT EXISTS usage_snapshots (
 vendor TEXT NOT NULL, account TEXT NOT NULL, pool TEXT NOT NULL,
 fetched_at REAL NOT NULL, payload TEXT NOT NULL,
 PRIMARY KEY(vendor,account,pool)
);
CREATE INDEX IF NOT EXISTS sessions_updated ON sessions(updated_at);
"#;

pub struct Store {
    pub path: PathBuf,
    connection: Mutex<Connection>,
}

/// One persisted provider usage payload (raw official data, e.g. the Codex
/// `account/rateLimits/read` result) with the time it was fetched.
#[derive(Clone, Debug, PartialEq)]
pub struct UsageRow {
    pub vendor: String,
    pub account: String,
    pub pool: String,
    pub fetched_at: f64,
    pub payload: Value,
}

impl Store {
    pub fn upsert_usage_snapshot(&self, row: &UsageRow) -> Result<()> {
        self.lock()?.execute(
            "INSERT INTO usage_snapshots(vendor,account,pool,fetched_at,payload) VALUES(?,?,?,?,?) ON CONFLICT(vendor,account,pool) DO UPDATE SET fetched_at=excluded.fetched_at,payload=excluded.payload",
            params![row.vendor, row.account, row.pool, row.fetched_at, row.payload.to_string()],
        )?;
        Ok(())
    }
    pub fn usage_snapshots(&self) -> Result<Vec<UsageRow>> {
        let db = self.lock()?;
        let mut statement = db.prepare(
            "SELECT vendor,account,pool,fetched_at,payload FROM usage_snapshots ORDER BY fetched_at DESC",
        )?;
        let rows = statement
            .query_map([], |r| {
                Ok(UsageRow {
                    vendor: r.get(0)?,
                    account: r.get(1)?,
                    pool: r.get(2)?,
                    fetched_at: r.get(3)?,
                    payload: serde_json::from_str(&r.get::<_, String>(4)?).unwrap_or(Value::Null),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
    /// Forget usage for a vendor (disconnect, account switch).
    pub fn delete_usage_snapshots(&self, vendor: &str) -> Result<usize> {
        Ok(self
            .lock()?
            .execute("DELETE FROM usage_snapshots WHERE vendor=?", [vendor])?)
    }
    pub fn native_meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .lock()?
            .query_row("SELECT value FROM native_meta WHERE key=?", [key], |r| {
                r.get::<_, String>(0)
            })
            .optional()?)
    }
    pub fn set_native_meta(&self, key: &str, value: &str) -> Result<()> {
        self.lock()?.execute(
            "INSERT INTO native_meta(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
    /// `session_meta` rows whose key starts with `prefix`, as (key, value).
    pub fn session_meta_prefixed(
        &self,
        session_id: &str,
        prefix: &str,
    ) -> Result<Vec<(String, String)>> {
        let db = self.lock()?;
        let pattern = format!("{}%", prefix.replace('%', "\\%"));
        let mut statement = db.prepare(
            "SELECT key,value FROM session_meta WHERE session_id=? AND key LIKE ? ESCAPE '\\' ORDER BY key",
        )?;
        let rows = statement
            .query_map(params![session_id, pattern], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
    /// Job records of one conversation, oldest first (at most `limit`, the
    /// most recent ones).
    pub fn session_jobs(&self, session_id: &str, limit: usize) -> Result<Vec<Value>> {
        let mut rows: Vec<Value> = self
            .query(
                "SELECT payload FROM desktop_jobs WHERE json_extract(payload,'$.session_id')=? ORDER BY rowid DESC LIMIT ?",
                params![session_id, limit.clamp(1, 1000)],
            )?
            .into_iter()
            .map(|row| row["payload"].clone())
            .collect();
        rows.reverse();
        Ok(rows)
    }
    /// Workspace paths a set of tasks changed, from vendor `files.changed`
    /// events and native file-change records.
    pub fn changed_files(&self, task_ids: &[String]) -> Result<Vec<String>> {
        let mut paths = Vec::new();
        for task in task_ids {
            for row in self.query(
                "SELECT payload FROM events WHERE task_id=? AND type='files.changed'",
                [task],
            )? {
                let payload: Value = match &row["payload"] {
                    Value::String(text) => serde_json::from_str(text).unwrap_or(Value::Null),
                    other => other.clone(),
                };
                for path in payload["paths"].as_array().into_iter().flatten() {
                    if let Some(path) = path.as_str() {
                        paths.push(path.to_owned());
                    }
                }
            }
            for row in self.query("SELECT path FROM file_changes WHERE task_id=?", [task])? {
                if let Some(path) = row["path"].as_str() {
                    paths.push(path.to_owned());
                }
            }
        }
        let mut seen = std::collections::HashSet::new();
        paths.retain(|p| seen.insert(p.clone()));
        Ok(paths)
    }
}

impl Store {
    /// Small per-session key/value used for execution targets and vendor
    /// session ids (`native_session:<vendor>`).
    pub fn set_session_meta(&self, session_id: &str, key: &str, value: &str) -> Result<()> {
        let connection = self.lock()?;
        connection.execute(
            "INSERT INTO session_meta(session_id,key,value) VALUES(?,?,?) ON CONFLICT(session_id,key) DO UPDATE SET value=excluded.value",
            params![session_id, key, value],
        )?;
        Ok(())
    }
    pub fn session_meta(&self, session_id: &str, key: &str) -> Result<Option<String>> {
        let connection = self.lock()?;
        let value = connection
            .query_row(
                "SELECT value FROM session_meta WHERE session_id=? AND key=?",
                params![session_id, key],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        Ok(value)
    }
    pub fn clear_session_meta_prefix(&self, key_prefix: &str) -> Result<usize> {
        let connection = self.lock()?;
        let pattern = format!("{}%", key_prefix.replace('%', "\\%"));
        let count = connection.execute(
            "DELETE FROM session_meta WHERE key LIKE ? ESCAPE '\\'",
            params![pattern],
        )?;
        Ok(count)
    }
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let existed = path.exists();
        let mut connection = Connection::open(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        }
        connection.busy_timeout(Duration::from_secs(10))?;
        connection.pragma_update(None, "foreign_keys", true)?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
        ensure!(
            version <= SCHEMA_VERSION,
            "This database was created by a newer ShadowCode version"
        );
        if version < SCHEMA_VERSION {
            if existed {
                let backup_path = path.with_extension(format!("pre-native-{}.sqlite", id()));
                let mut backup = Connection::open(&backup_path)?;
                rusqlite::backup::Backup::new(&connection, &mut backup)?.run_to_completion(
                    128,
                    Duration::from_millis(5),
                    None,
                )?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&backup_path, fs::Permissions::from_mode(0o600))?;
                }
            }
            let tx = connection.transaction()?;
            if version < 24 {
                migrate_to_24(&tx)?;
            }
            // Ordered forward steps; each is idempotent.
            if version < 25 {
                tx.execute_batch(MIGRATION_25)?;
            }
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            tx.commit()?;
        }
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        let store = Self {
            path: path.into(),
            connection: Mutex::new(connection),
        };
        store.import_legacy_goals()?;
        store.import_legacy_background()?;
        Ok(store)
    }
}

/// The flat pre-0.28 schema (user_version 24): every table plus the columns
/// older databases lacked.
fn migrate_to_24(tx: &rusqlite::Transaction<'_>) -> Result<()> {
    tx.execute_batch(SCHEMA)?;
    for (table, column, kind) in [
        ("sessions", "title", "TEXT"),
        ("sessions", "usage_json", "TEXT"),
        ("sessions", "parent_id", "TEXT"),
        ("sessions", "branched_at", "REAL"),
        ("tasks", "usage_json", "TEXT"),
        ("file_changes", "observed_hash", "TEXT"),
        ("milestones", "mode", "TEXT NOT NULL DEFAULT 'code'"),
        (
            "milestones",
            "require_verification",
            "INTEGER NOT NULL DEFAULT 0",
        ),
    ] {
        let columns: Vec<String> = tx
            .prepare(&format!("PRAGMA table_info({table})"))?
            .query_map([], |r| r.get(1))?
            .collect::<rusqlite::Result<_>>()?;
        if !columns.iter().any(|c| c == column) {
            tx.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {kind}"))?;
        }
    }
    Ok(())
}

impl Store {
    fn import_legacy_goals(&self) -> Result<()> {
        let legacy = self.path.with_file_name("goals.db");
        if legacy == self.path || !legacy.is_file() {
            return Ok(());
        }
        let mut db = self.lock()?;
        if db
            .query_row(
                "SELECT value FROM native_meta WHERE key='legacy_goals_imported'",
                [],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .is_some()
        {
            return Ok(());
        }
        db.execute(
            "ATTACH DATABASE ? AS legacy_goals",
            params![legacy.to_string_lossy()],
        )?;
        let result = (|| -> Result<()> {
            let tx = db.transaction()?;
            tx.execute_batch(
                "INSERT OR IGNORE INTO goals SELECT * FROM legacy_goals.goals;
                INSERT OR IGNORE INTO milestones(id,goal_id,title,status,order_index,detail,task_id,created_at,updated_at)
                    SELECT id,goal_id,title,status,order_index,detail,task_id,created_at,updated_at FROM legacy_goals.milestones;
                INSERT INTO native_meta(key,value) VALUES('legacy_goals_imported','true');",
            )?;
            tx.commit()?;
            Ok(())
        })();
        db.execute("DETACH DATABASE legacy_goals", [])?;
        result
    }

    pub(crate) fn lock(&self) -> Result<MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|_| anyhow::anyhow!("Database lock was poisoned"))
    }
    pub(crate) fn query(&self, sql: &str, args: impl Params) -> Result<Vec<Value>> {
        query_rows(&*self.lock()?, sql, args)
    }
    pub(crate) fn execute(&self, sql: &str, args: impl Params) -> Result<usize> {
        Ok(self.lock()?.execute(sql, args)?)
    }

    pub fn create_session(&self, workspace: &Path, model: &str, title: &str) -> Result<Value> {
        let sid = id();
        let now = now();
        self.execute("INSERT INTO sessions(id,workspace,created_at,updated_at,model_id,status,title) VALUES(?,?,?,?,?,'active',?)",
            params![sid,workspace.to_string_lossy(),now,now,model,title])?;
        self.session(&sid)?.context("Created session disappeared")
    }
    pub fn session(&self, sid: &str) -> Result<Option<Value>> {
        Ok(self
            .query("SELECT * FROM sessions WHERE id=?", [sid])?
            .into_iter()
            .next())
    }
    /// Resolve identifiers against the complete indexed history, never a recent
    /// list of potentially large job payloads. Two rows suffice for ambiguity.
    pub fn resolve_id(&self, kind: &str, prefix: &str) -> Result<String> {
        ensure!(
            !prefix.is_empty()
                && prefix.len() <= 128
                && prefix
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')),
            "Provide a valid ID or ID prefix"
        );
        let table = match kind {
            "session" => "sessions",
            "job" => "desktop_jobs",
            _ => anyhow::bail!("Unknown identifier kind"),
        };
        let rows = self.query(
            &format!("SELECT id FROM {table} WHERE id>=? AND id<? ORDER BY id LIMIT 2"),
            params![prefix, format!("{prefix}~")],
        )?;
        ensure!(!rows.is_empty(), "No {kind} matches {prefix}");
        ensure!(
            rows.len() == 1,
            "Choose a unique {kind} ID prefix; at least two matches found"
        );
        Ok(rows[0]["id"]
            .as_str()
            .context("Stored identifier missing")?
            .into())
    }
    pub fn sessions(&self, search: &str, limit: usize) -> Result<Vec<Value>> {
        self.sessions_in(search, limit, None)
    }
    pub fn sessions_in(
        &self,
        search: &str,
        limit: usize,
        workspace: Option<&Path>,
    ) -> Result<Vec<Value>> {
        let needle = format!(
            "%{}%",
            search
                .replace('!', "!!")
                .replace('%', "!%")
                .replace('_', "!_")
        );
        self.query("SELECT s.* FROM sessions s WHERE (? IS NULL OR s.workspace=?) AND (s.title LIKE ? ESCAPE '!' OR s.workspace LIKE ? ESCAPE '!'
            OR EXISTS(SELECT 1 FROM tasks t WHERE t.session_id=s.id AND t.prompt LIKE ? ESCAPE '!'))
            ORDER BY s.updated_at DESC LIMIT ?", params![workspace.map(|p|p.to_string_lossy()),workspace.map(|p|p.to_string_lossy()),needle,needle,needle,limit.clamp(1,10000)])
    }
    pub fn rename_session(&self, sid: &str, title: &str) -> Result<()> {
        ensure!(title.len() <= 500, "Task title is too long");
        ensure!(
            self.execute(
                "UPDATE sessions SET title=?,updated_at=? WHERE id=?",
                params![title, now(), sid]
            )? == 1,
            "Session not found"
        );
        Ok(())
    }
    pub fn branch_session(&self, sid: &str, title: &str) -> Result<Value> {
        self.branch_session_with_memory(sid, title, "")
    }

    /// Fork session state from a checkpoint/event id into a new session branch
    /// without deleting the original. Copies only events with id <= event_id.
    pub fn fork_session_from_event(&self, sid: &str, event_id: i64, title: &str) -> Result<Value> {
        ensure!(event_id > 0, "event_id must be positive");
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        let parent = query_rows(&tx, "SELECT * FROM sessions WHERE id=?", [sid])?
            .pop()
            .context("Session not found")?;
        let selected: i64 = tx.query_row(
            "SELECT COUNT(*) FROM events WHERE session_id=? AND id=?",
            params![sid, event_id],
            |row| row.get(0),
        )?;
        ensure!(selected == 1, "event_id does not belong to this session");
        // Reuse only the tape of a task completed at or before the cut. A newer
        // tape would leak future turns into the fork; events alone lose tool results.
        let completed = query_rows(&tx,
            "SELECT e.id,e.task_id FROM events e WHERE e.session_id=? AND e.id<=? AND e.type='agent.completed' ORDER BY e.id DESC LIMIT 1",
            params![sid,event_id])?;
        let mut seed: Vec<Value> = Vec::new();
        let mut after = 0;
        if let Some(event) = completed.first() {
            let tape = query_rows(&tx,
                "SELECT payload FROM job_messages WHERE job_id=(SELECT id FROM desktop_jobs WHERE json_extract(payload,'$.task_id')=? ORDER BY rowid DESC LIMIT 1) ORDER BY ordinal",
                [event["task_id"].as_str().unwrap_or("")])?;
            if !tape.is_empty() {
                seed = tape.into_iter().map(|row| row["payload"].clone()).collect();
                after = event["id"].as_i64().unwrap_or(0);
            }
        }
        // A mid-turn cut contains only observed text. Do not reconstruct or
        // replay unfinished tool calls from transcript fragments.
        for event in query_rows(&tx,
            "SELECT type,payload FROM events WHERE session_id=? AND id>? AND id<=? AND type IN ('user.message','model.delta','agent.message') ORDER BY id",
            params![sid,after,event_id])? {
            let kind = event["type"].as_str().unwrap_or("");
            if kind == "model.delta" && event["payload"]["complete"] != true { continue; }
            if let Some(text) = event["payload"]["text"].as_str() {
                seed.push(json!({"role":if kind=="user.message" {"user"} else {"assistant"},"content":text}));
            }
        }
        crate::context::repair_incomplete(&mut seed);
        let branch = id();
        let time = now();
        let title = if title.is_empty() {
            format!(
                "{} (fork@{event_id})",
                parent["title"].as_str().unwrap_or("Task")
            )
        } else {
            title.into()
        };
        tx.execute(
            "INSERT INTO sessions(id,workspace,created_at,updated_at,model_id,status,title,parent_id,branched_at) VALUES(?,?,?,?,?,'active',?,?,?)",
            params![
                branch,
                parent["workspace"].as_str(),
                time,
                time,
                parent["model_id"].as_str(),
                title,
                sid,
                time
            ],
        )?;
        tx.execute(
            "INSERT INTO events(ts,type,session_id,task_id,payload) SELECT ts,type,?,task_id,payload FROM events WHERE session_id=? AND id<=? ORDER BY id",
            params![branch, sid, event_id],
        )?;
        tx.execute(
            "INSERT INTO session_meta(session_id,key,value) VALUES(?,'forked_from_event',?)",
            params![branch, event_id.to_string()],
        )?;
        let result = query_rows(&tx, "SELECT * FROM sessions WHERE id=?", [&branch])?
            .pop()
            .context("Fork not found")?;
        tx.execute(
            "INSERT INTO session_meta(session_id,key,value) VALUES(?,'message_seed',?)",
            params![branch, serde_json::to_string(&seed)?],
        )?;
        let original = query_rows(&tx, "SELECT * FROM sessions WHERE id=?", [sid])?
            .pop()
            .context("Original session missing after fork")?;
        tx.commit()?;
        Ok(json!({
            "fork": result,
            "original": original,
            "forked_from_event": event_id,
            "original_intact": true
        }))
    }
    pub fn branch_session_with_memory(
        &self,
        sid: &str,
        title: &str,
        memory: &str,
    ) -> Result<Value> {
        ensure!(
            memory.len() <= 32_000_000,
            "Branch memory exceeds its limit"
        );
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        let parent = query_rows(&tx, "SELECT * FROM sessions WHERE id=?", [sid])?
            .pop()
            .context("Session not found")?;
        let branch = id();
        let time = now();
        let title = if title.is_empty() {
            format!("{} (branch)", parent["title"].as_str().unwrap_or("Task"))
        } else {
            title.into()
        };
        tx.execute("INSERT INTO sessions(id,workspace,created_at,updated_at,model_id,status,title,parent_id,branched_at) VALUES(?,?,?,?,?,'active',?,?,?)",
            params![branch,parent["workspace"].as_str(),time,time,parent["model_id"].as_str(),title,sid,time])?;
        tx.execute("INSERT INTO events(ts,type,session_id,task_id,payload) SELECT ts,type,?,task_id,payload FROM events WHERE session_id=? ORDER BY id",params![branch,sid])?;
        tx.execute("INSERT INTO pins(session_id,task_id,ts,label,body) SELECT ?,task_id,ts,label,body FROM pins WHERE session_id=?",params![branch,sid])?;
        let tape=query_rows(&tx,"SELECT payload FROM job_messages WHERE job_id=(SELECT id FROM desktop_jobs WHERE json_extract(payload,'$.session_id')=? AND EXISTS(SELECT 1 FROM job_messages WHERE job_id=desktop_jobs.id) ORDER BY rowid DESC LIMIT 1) ORDER BY ordinal",[sid])?;
        if !tape.is_empty() {
            let messages: Vec<_> = tape.into_iter().map(|r| r["payload"].clone()).collect();
            tx.execute(
                "INSERT INTO session_meta(session_id,key,value) VALUES(?,'message_seed',?)",
                params![branch, json!(messages).to_string()],
            )?;
        } else {
            tx.execute("INSERT INTO session_meta(session_id,key,value) SELECT ?,key,value FROM session_meta WHERE session_id=? AND key='message_seed'",params![branch,sid])?;
        }
        if !memory.is_empty() {
            tx.execute(
                "INSERT INTO session_meta(session_id,key,value) VALUES(?,'memory_seed',?)",
                params![branch, memory],
            )?;
        }
        let result = query_rows(&tx, "SELECT * FROM sessions WHERE id=?", [&branch])?
            .pop()
            .context("Branch not found")?;
        tx.commit()?;
        Ok(result)
    }
    pub fn delete_session(&self, sid: &str) -> Result<bool> {
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        let active: i64 = tx.query_row("SELECT count(*) FROM desktop_jobs WHERE json_extract(payload,'$.session_id')=? AND json_extract(payload,'$.status') IN ('queued','running','paused','cancelling')",[sid],|r|r.get(0))?;
        ensure!(
            active == 0,
            "Stop the running task before deleting this session"
        );
        for table in ["events", "pins", "session_meta", "queued_tasks"] {
            tx.execute(&format!("DELETE FROM {table} WHERE session_id=?"), [sid])?;
        }
        tx.execute(
            "DELETE FROM file_changes WHERE task_id IN (SELECT id FROM tasks WHERE session_id=?)",
            [sid],
        )?;
        tx.execute("DELETE FROM job_messages WHERE job_id IN (SELECT id FROM desktop_jobs WHERE json_extract(payload,'$.session_id')=?)",[sid])?;
        tx.execute(
            "DELETE FROM desktop_jobs WHERE json_extract(payload,'$.session_id')=?",
            [sid],
        )?;
        tx.execute("DELETE FROM tasks WHERE session_id=?", [sid])?;
        tx.execute(
            "UPDATE sessions SET parent_id=NULL WHERE parent_id=?",
            [sid],
        )?;
        let deleted = tx.execute("DELETE FROM sessions WHERE id=?", [sid])? != 0;
        tx.commit()?;
        Ok(deleted)
    }
    pub fn create_task(&self, sid: &str, prompt: &str) -> Result<String> {
        let task = id();
        self.execute(
            "INSERT INTO tasks(id,session_id,prompt,status,created_at) VALUES(?,?,?,'running',?)",
            params![task, sid, prompt, now()],
        )?;
        Ok(task)
    }
    pub fn finish_task(&self, tid: &str, status: &str, summary: &str, usage: &Value) -> Result<()> {
        self.finish_transaction(tid, status, summary, usage, None)
            .map(|_| ())
    }
    pub fn finish_job(&self, job: &mut Value) -> Result<Value> {
        let tid = job["task_id"]
            .as_str()
            .context("Missing task ID")?
            .to_owned();
        let status = job["status"].as_str().context("Missing status")?.to_owned();
        let summary = job["summary"].as_str().unwrap_or("").to_owned();
        let usage = job["usage"].clone();
        self.finish_transaction(&tid, &status, &summary, &usage, Some(job))?
            .context("Missing completion event")
    }
    fn finish_transaction(
        &self,
        tid: &str,
        status: &str,
        summary: &str,
        usage: &Value,
        job: Option<&mut Value>,
    ) -> Result<Option<Value>> {
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        let sid = finish_task_on(&tx, tid, status, summary, usage)?;
        let event = if let Some(job) = job {
            let ts = now();
            let payload = job["result"].clone();
            tx.execute("INSERT INTO events(ts,type,session_id,task_id,payload) VALUES(?,'agent.completed',?,?,?)",params![ts,sid,tid,payload.to_string()])?;
            let cursor = tx.last_insert_rowid();
            job["event_cursor"] = json!(cursor);
            tx.execute(
                "UPDATE desktop_jobs SET payload=? WHERE id=?",
                params![job.to_string(), job["id"].as_str()],
            )?;
            Some(
                json!({"id":cursor,"ts":ts,"type":"agent.completed","session_id":sid,"task_id":tid,"payload":payload}),
            )
        } else {
            None
        };
        tx.commit()?;
        Ok(event)
    }
    pub fn tasks(&self, sid: &str, limit: usize) -> Result<Vec<Value>> {
        self.query(
            "SELECT * FROM tasks WHERE session_id=? ORDER BY created_at DESC LIMIT ?",
            params![sid, limit.clamp(1, 10000)],
        )
    }
    pub fn task(&self, tid: &str) -> Result<Option<Value>> {
        Ok(self.query("SELECT * FROM tasks WHERE id=?", [tid])?.pop())
    }
    pub fn last_task_event(&self, tid: &str, kind: &str) -> Result<Option<Value>> {
        Ok(self
            .query(
                "SELECT * FROM events WHERE task_id=? AND type=? ORDER BY id DESC LIMIT 1",
                params![tid, kind],
            )?
            .pop())
    }
    pub fn add_event(
        &self,
        kind: &str,
        payload: &Value,
        sid: Option<&str>,
        tid: Option<&str>,
    ) -> Result<Value> {
        let db = self.lock()?;
        let time = now();
        db.execute(
            "INSERT INTO events(ts,type,session_id,task_id,payload) VALUES(?,?,?,?,?)",
            params![time, kind, sid, tid, payload.to_string()],
        )?;
        Ok(
            json!({"id":db.last_insert_rowid(),"ts":time,"type":kind,"session_id":sid,"task_id":tid,"payload":payload}),
        )
    }
    /// Local-only store metrics for Doctor. No telemetry is sent.
    pub fn local_stats(&self) -> Result<Value> {
        let db = self.lock()?;
        let events: i64 = db.query_row("SELECT count(*) FROM events", [], |r| r.get(0))?;
        let sessions: i64 = db.query_row("SELECT count(*) FROM sessions", [], |r| r.get(0))?;
        let jobs: i64 = db.query_row("SELECT count(*) FROM desktop_jobs", [], |r| r.get(0))?;
        let page_count: i64 = db.query_row("PRAGMA page_count", [], |r| r.get(0))?;
        let page_size: i64 = db.query_row("PRAGMA page_size", [], |r| r.get(0))?;
        let wal = db
            .query_row("PRAGMA journal_mode", [], |r| r.get::<_, String>(0))
            .unwrap_or_default();
        let bytes = fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
        Ok(json!({
            "events": events,
            "sessions": sessions,
            "jobs": jobs,
            "bytes": bytes,
            "page_count": page_count,
            "page_size": page_size,
            "journal_mode": wal,
            "telemetry": false
        }))
    }
    pub fn event_cursor(&self, sid: &str) -> Result<i64> {
        Ok(self.lock()?.query_row(
            "SELECT coalesce(max(id),0) FROM events WHERE session_id=?",
            [sid],
            |r| r.get(0),
        )?)
    }
    pub fn events_after(
        &self,
        sid: &str,
        after: i64,
        through: Option<i64>,
        limit: usize,
    ) -> Result<Vec<Value>> {
        self.query(
            "SELECT * FROM events WHERE session_id=? AND id>? AND id<=? ORDER BY id LIMIT ?",
            params![
                sid,
                after.max(0),
                through.unwrap_or(i64::MAX),
                limit.clamp(1, 2000)
            ],
        )
    }
    pub fn recent_events(&self, sid: &str, limit: usize) -> Result<Vec<Value>> {
        self.recent_events_through(sid, i64::MAX, limit)
    }
    pub fn recent_events_through(
        &self,
        sid: &str,
        through: i64,
        limit: usize,
    ) -> Result<Vec<Value>> {
        let mut rows = self.query(
            "SELECT * FROM events WHERE session_id=? AND id<=? ORDER BY id DESC LIMIT ?",
            params![sid, through, limit.clamp(1, 10000)],
        )?;
        rows.reverse();
        Ok(rows)
    }
    /// A bounded desktop history page. Oversized events remain in the store/export.
    pub fn history_page(&self, sid: &str, through: i64) -> Result<Value> {
        let events = self.query(
            "WITH candidates AS (SELECT id,ts,session_id,task_id,
                CASE WHEN length(CAST(payload AS BLOB))>262144 THEN 'history.omitted' ELSE type END AS type,
                CASE WHEN length(CAST(payload AS BLOB))>262144 THEN json_object('text','A large saved event is omitted from this preview. Use Export this task as JSON in the command palette to read its original content.','original_type',type,'original_bytes',length(CAST(payload AS BLOB))) ELSE payload END AS payload
                FROM events WHERE session_id=? AND id<=? ORDER BY id DESC LIMIT 128),
             bounded AS (SELECT *,sum(length(CAST(payload AS BLOB))) OVER (ORDER BY id DESC) AS bytes FROM candidates)
             SELECT id,ts,session_id,task_id,type,payload FROM bounded WHERE bytes<=2097152 ORDER BY id",
            params![sid, through],
        )?;
        let first = events.first().and_then(|e| e["id"].as_i64()).unwrap_or(0);
        let has_older = first > 0
            && !self
                .query(
                    "SELECT id FROM events WHERE session_id=? AND id<? LIMIT 1",
                    params![sid, first],
                )?
                .is_empty();
        Ok(
            json!({"events":events,"first_cursor":first,"event_cursor":through,"has_older":has_older}),
        )
    }
    pub fn save_job(&self, job: &Value) -> Result<()> {
        let id = job["id"].as_str().context("Job requires an ID")?;
        self.execute("INSERT INTO desktop_jobs(id,payload) VALUES(?,?) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",params![id,job.to_string()])?;
        Ok(())
    }
    /// Queue the task and its recoverable job atomically.
    pub fn create_job(&self, job: &Value) -> Result<()> {
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        let task_id = job["task_id"].as_str().context("Missing task ID")?;
        let session_id = job["session_id"].as_str().context("Missing session ID")?;
        let prompt = job["task"].as_str().context("Missing task text")?;
        tx.execute(
            "INSERT INTO tasks(id,session_id,prompt,status,created_at) VALUES(?,?,?,'queued',?)",
            params![task_id, session_id, prompt, now()],
        )?;
        tx.execute(
            "INSERT INTO desktop_jobs(id,payload) VALUES(?,?)",
            params![job["id"].as_str(), job.to_string()],
        )?;
        tx.execute(
            "INSERT INTO events(ts,type,session_id,task_id,payload) VALUES(?,'user.message',?,?,?)",
            params![
                now(),
                session_id,
                task_id,
                json!({"text":prompt}).to_string()
            ],
        )?;
        tx.execute("UPDATE sessions SET updated_at=?,title=CASE WHEN title IS NULL OR title='' THEN ? ELSE title END WHERE id=?",params![now(),prompt.chars().take(80).collect::<String>(),session_id])?;
        tx.commit()?;
        Ok(())
    }
    pub fn latest_session_messages(
        &self,
        session_id: &str,
        excluding_job: &str,
    ) -> Result<Vec<Value>> {
        let row=self.query("SELECT id FROM desktop_jobs WHERE json_extract(payload,'$.session_id')=? AND id!=? AND EXISTS(SELECT 1 FROM job_messages WHERE job_id=desktop_jobs.id) ORDER BY rowid DESC LIMIT 1",params![session_id,excluding_job])?;
        match row.first().and_then(|r| r["id"].as_str()) {
            Some(id) => self.messages(id),
            None => {
                let seed = self.query(
                    "SELECT value FROM session_meta WHERE session_id=? AND key='message_seed'",
                    [session_id],
                )?;
                match seed.first().and_then(|r| r["value"].as_str()) {
                    Some(seed) => Ok(serde_json::from_str(seed)?),
                    None => Ok(Vec::new()),
                }
            }
        }
    }
    pub fn jobs(&self, limit: usize) -> Result<Vec<Value>> {
        Ok(self
            .query(
                "SELECT payload FROM desktop_jobs ORDER BY rowid DESC LIMIT ?",
                [limit.clamp(1, 10000)],
            )?
            .into_iter()
            .map(|v| v["payload"].clone())
            .collect())
    }
    pub fn job(&self, id: &str) -> Result<Option<Value>> {
        Ok(self
            .query("SELECT payload FROM desktop_jobs WHERE id=?", [id])?
            .pop()
            .map(|v| v["payload"].clone()))
    }
    /// Keep active work visible even after unrelated projects produce a large
    /// amount of completed history. Insertion order is the workspace FIFO order.
    pub fn active_and_recent_jobs(&self, limit: usize) -> Result<Vec<Value>> {
        Ok(self.query(
            "SELECT payload FROM desktop_jobs WHERE rowid IN (SELECT rowid FROM desktop_jobs ORDER BY rowid DESC LIMIT ?) OR json_extract(payload,'$.status') IN ('queued','running','paused','cancelling') ORDER BY rowid DESC",
            [limit.clamp(1, 10000)],
        )?.into_iter().map(|row|row["payload"].clone()).collect())
    }
    /// Small polling records. Full prompts/results remain available by exact ID.
    pub fn job_summaries(&self, limit: usize) -> Result<Vec<Value>> {
        Ok(self.query(
            "SELECT json_object('id',id,'workspace',json_extract(payload,'$.workspace'),'session_id',json_extract(payload,'$.session_id'),'task_id',json_extract(payload,'$.task_id'),'status',json_extract(payload,'$.status'),'mode',json_extract(payload,'$.mode'),'purpose',substr(json_extract(payload,'$.routing.purpose'),1,32),'model',substr(json_extract(payload,'$.model'),1,512),'started_at',json_extract(payload,'$.started_at'),'finished_at',json_extract(payload,'$.finished_at'),'event_cursor',json_extract(payload,'$.event_cursor'),'task',substr(json_extract(payload,'$.task'),1,512),'task_truncated',CASE WHEN length(json_extract(payload,'$.task'))>512 THEN json('true') ELSE json('false') END) AS payload FROM desktop_jobs WHERE rowid IN (SELECT rowid FROM desktop_jobs ORDER BY rowid DESC LIMIT ?) OR json_extract(payload,'$.status') IN ('queued','running','paused','cancelling') ORDER BY rowid DESC",
            [limit.clamp(1,100)],
        )?.into_iter().map(|row|row["payload"].clone()).collect())
    }
    pub fn current_job(&self, session: &str, include_finished: bool) -> Result<Option<Value>> {
        Ok(self.query(
            "SELECT payload FROM desktop_jobs WHERE json_extract(payload,'$.session_id')=? AND (? OR json_extract(payload,'$.status') IN ('queued','running','paused','cancelling')) ORDER BY CASE json_extract(payload,'$.status') WHEN 'running' THEN 0 WHEN 'paused' THEN 0 WHEN 'cancelling' THEN 1 WHEN 'queued' THEN 2 ELSE 3 END, CASE WHEN json_extract(payload,'$.status')='queued' THEN rowid END ASC, json_extract(payload,'$.finished_at') DESC, rowid DESC LIMIT 1",
            rusqlite::params![session,include_finished],
        )?.pop().map(|row|row["payload"].clone()))
    }
    /// Called only by the profile-lock owner, before accepting new work.
    pub fn recover_jobs(&self) -> Result<usize> {
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        let jobs = query_rows(&tx,"SELECT payload FROM desktop_jobs WHERE json_extract(payload,'$.status') IN ('queued','running','paused','cancelling')",[])?;
        for row in &jobs {
            let mut job = row["payload"].clone();
            job["status"] = json!("interrupted");
            job["finished_at"] = json!(now());
            job["summary"]=json!("The application stopped before this task finished. Review its changes, then continue.");
            let result = json!({
                "success": false,
                "cancelled": false,
                "interrupted": true,
                "summary": job["summary"],
                "usage": job.get("usage").cloned().unwrap_or(json!({}))
            });
            job["result"] = result.clone();
            if let Some(tid) = job["task_id"].as_str() {
                let sid = finish_task_on(
                    &tx,
                    tid,
                    "interrupted",
                    job["summary"].as_str().unwrap_or("Task interrupted"),
                    &job["usage"],
                )?;
                let ts = now();
                tx.execute(
                    "INSERT INTO events(ts,type,session_id,task_id,payload) VALUES(?,'agent.completed',?,?,?)",
                    params![ts, sid, tid, result.to_string()],
                )?;
                job["event_cursor"] = json!(tx.last_insert_rowid());
            }
            tx.execute(
                "UPDATE desktop_jobs SET payload=? WHERE id=?",
                params![job.to_string(), job["id"].as_str()],
            )?;
        }
        tx.commit()?;
        Ok(jobs.len())
    }
    pub fn save_messages(&self, job_id: &str, messages: &[Value]) -> Result<()> {
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        tx.execute("DELETE FROM job_messages WHERE job_id=?", [job_id])?;
        for (ordinal, message) in messages.iter().enumerate() {
            tx.execute(
                "INSERT INTO job_messages(job_id,ordinal,payload) VALUES(?,?,?)",
                params![job_id, ordinal, message.to_string()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn messages(&self, job_id: &str) -> Result<Vec<Value>> {
        Ok(self
            .query(
                "SELECT payload FROM job_messages WHERE job_id=? ORDER BY ordinal",
                [job_id],
            )?
            .into_iter()
            .map(|v| v["payload"].clone())
            .collect())
    }
    pub fn touch_project(&self, path: &Path) -> Result<()> {
        let path = path.canonicalize()?;
        self.execute("INSERT INTO projects(id,path,name,last_opened) VALUES(?,?,?,?) ON CONFLICT(path) DO UPDATE SET last_opened=excluded.last_opened",params![id(),path.to_string_lossy(),path.file_name().map(|s|s.to_string_lossy().into_owned()).unwrap_or_else(||"/".into()),now()])?;
        Ok(())
    }
    pub fn projects(&self) -> Result<Vec<Value>> {
        self.query(
            "SELECT * FROM projects ORDER BY last_opened DESC LIMIT 1000",
            [],
        )
    }
    pub fn models(&self) -> Result<Vec<Value>> {
        self.query("SELECT * FROM models ORDER BY name", [])
    }
    pub fn upsert_model(&self, model: &Value) -> Result<()> {
        self.execute("INSERT INTO models(id,name,provider,endpoint,context_limit,metadata) VALUES(?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,provider=excluded.provider,endpoint=excluded.endpoint,context_limit=excluded.context_limit,metadata=excluded.metadata",
            params![model["id"].as_str().context("Model ID required")?,model["name"].as_str().context("Model name required")?,model["provider"].as_str().context("Provider required")?,model["endpoint"].as_str().unwrap_or(""),model["context_limit"].as_u64().unwrap_or(128000),model.get("metadata").unwrap_or(&json!({})).to_string()])?;
        Ok(())
    }
    pub fn upsert_detected_model(&self, model: &Value) -> Result<()> {
        self.execute("INSERT INTO models(id,name,provider,endpoint,context_limit,metadata) VALUES(?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET context_limit=CASE WHEN json_extract(models.metadata,'$.api_key_env') IS NULL THEN excluded.context_limit ELSE models.context_limit END,metadata=json_patch(models.metadata,excluded.metadata) WHERE models.name=excluded.name AND models.provider=excluded.provider AND models.endpoint=excluded.endpoint",
            params![model["id"].as_str().context("Model ID required")?,model["name"].as_str().context("Model name required")?,model["provider"].as_str().context("Provider required")?,model["endpoint"].as_str().context("Endpoint required")?,model["context_limit"].as_u64().context("Context limit required")?,model["metadata"].to_string()])?;
        Ok(())
    }
    pub fn pins(&self, sid: &str) -> Result<Vec<Value>> {
        self.query("SELECT * FROM pins WHERE session_id=? ORDER BY id", [sid])
    }
    pub fn add_pin(&self, sid: &str, label: &str, body: &str) -> Result<i64> {
        ensure!(self.session(sid)?.is_some(), "Session not found");
        let db = self.lock()?;
        db.execute(
            "INSERT INTO pins(session_id,ts,label,body) VALUES(?,?,?,?)",
            params![sid, now(), label, body],
        )?;
        Ok(db.last_insert_rowid())
    }
    pub fn delete_pin(&self, sid: &str, id: i64) -> Result<()> {
        self.execute(
            "DELETE FROM pins WHERE session_id=? AND id=?",
            params![sid, id],
        )?;
        Ok(())
    }
}

fn query_rows(db: &Connection, sql: &str, args: impl Params) -> Result<Vec<Value>> {
    let mut statement = db.prepare(sql)?;
    let columns: Vec<String> = statement
        .column_names()
        .iter()
        .map(|s| (*s).into())
        .collect();
    let rows = statement
        .query_map(args, |row| {
            let mut out = Map::new();
            for (i, key) in columns.iter().enumerate() {
                let value = match row.get_ref(i)? {
                    ValueRef::Null => Value::Null,
                    ValueRef::Integer(v) => json!(v),
                    ValueRef::Real(v) => json!(v),
                    ValueRef::Text(v) => {
                        let text = String::from_utf8_lossy(v).into_owned();
                        if matches!(key.as_str(), "payload" | "metadata") {
                            serde_json::from_str(&text).unwrap_or(Value::Null)
                        } else {
                            json!(text)
                        }
                    }
                    ValueRef::Blob(_) => Value::Null,
                };
                out.insert(key.clone(), value);
            }
            Ok(Value::Object(out))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn finish_task_on(
    tx: &Connection,
    tid: &str,
    status: &str,
    summary: &str,
    usage: &Value,
) -> Result<String> {
    let sid: String = tx.query_row("SELECT session_id FROM tasks WHERE id=?", [tid], |r| {
        r.get(0)
    })?;
    let previous: Option<String> =
        tx.query_row("SELECT usage_json FROM tasks WHERE id=?", [tid], |r| {
            r.get(0)
        })?;
    let previous: Value = previous
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(json!({}));
    let current: Option<String> =
        tx.query_row("SELECT usage_json FROM sessions WHERE id=?", [&sid], |r| {
            r.get(0)
        })?;
    let mut total: Value = current
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(json!({}));
    if let Some(usage) = usage.as_object() {
        for (key, value) in usage {
            total[key] = json!(total[key]
                .as_u64()
                .unwrap_or(0)
                .saturating_sub(previous[key].as_u64().unwrap_or(0))
                .saturating_add(value.as_u64().unwrap_or(0)));
        }
    }
    tx.execute(
        "UPDATE tasks SET status=?,summary=?,completed_at=?,usage_json=? WHERE id=?",
        params![status, summary, now(), usage.to_string(), tid],
    )?;
    tx.execute(
        "UPDATE sessions SET updated_at=?,usage_json=? WHERE id=?",
        params![now(), total.to_string(), sid],
    )?;
    Ok(sid)
}
