"""SQLite persistence for sessions, tasks, events, models, and projects."""

from __future__ import annotations

import json
import sqlite3
import threading
import time
import uuid
from pathlib import Path
from typing import Any

from shadow_agent import paths

SCHEMA = """
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    workspace TEXT NOT NULL,
    created_at REAL NOT NULL,
    updated_at REAL NOT NULL,
    model_id TEXT,
    status TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    prompt TEXT NOT NULL,
    status TEXT NOT NULL,
    summary TEXT,
    created_at REAL NOT NULL,
    completed_at REAL,
    FOREIGN KEY(session_id) REFERENCES sessions(id)
);
CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts REAL NOT NULL,
    type TEXT NOT NULL,
    session_id TEXT,
    task_id TEXT,
    payload TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS models (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    provider TEXT NOT NULL,
    endpoint TEXT,
    context_limit INTEGER,
    metadata TEXT
);
CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY,
    path TEXT UNIQUE NOT NULL,
    name TEXT NOT NULL,
    last_opened REAL NOT NULL
);
"""


class Store:
    def __init__(self, db_path: Path | None = None) -> None:
        self.path = db_path or paths.db_file()
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self._conn = sqlite3.connect(str(self.path), check_same_thread=False)
        self._conn.row_factory = sqlite3.Row
        self._conn.execute("PRAGMA foreign_keys = ON")
        self._lock = threading.Lock()
        with self._lock:
            self._conn.executescript(SCHEMA)
            self._migrate()
            self._conn.commit()

    def _migrate(self) -> None:
        session_cols = {row[1] for row in self._conn.execute("PRAGMA table_info(sessions)")}
        if "title" not in session_cols:
            self._conn.execute("ALTER TABLE sessions ADD COLUMN title TEXT")
        if "usage_json" not in session_cols:
            self._conn.execute("ALTER TABLE sessions ADD COLUMN usage_json TEXT")
        task_cols = {row[1] for row in self._conn.execute("PRAGMA table_info(tasks)")}
        if "usage_json" not in task_cols:
            self._conn.execute("ALTER TABLE tasks ADD COLUMN usage_json TEXT")

    def close(self) -> None:
        with self._lock:
            self._conn.close()

    def create_session(self, workspace: str, model_id: str | None = None, title: str = "") -> str:
        sid = uuid.uuid4().hex
        now = time.time()
        with self._lock:
            self._conn.execute(
                "INSERT INTO sessions(id, workspace, created_at, updated_at, model_id, status, title) "
                "VALUES (?, ?, ?, ?, ?, ?, ?)",
                (sid, workspace, now, now, model_id, "active", title),
            )
            self._conn.commit()
        return sid

    def set_session_title(self, session_id: str, title: str) -> None:
        with self._lock:
            self._conn.execute("UPDATE sessions SET title = ?, updated_at = ? WHERE id = ?", (title, time.time(), session_id))
            self._conn.commit()

    def add_usage(self, session_id: str, task_id: str | None, usage: dict[str, int]) -> None:
        if not usage:
            return
        with self._lock:
            if task_id:
                self._conn.execute("UPDATE tasks SET usage_json = ? WHERE id = ?", (json.dumps(usage), task_id))
            row = self._conn.execute("SELECT usage_json FROM sessions WHERE id = ?", (session_id,)).fetchone()
            current = {}
            if row and row["usage_json"]:
                try:
                    current = json.loads(row["usage_json"])
                except json.JSONDecodeError:
                    current = {}
            merged = dict(current)
            for key, value in usage.items():
                merged[key] = int(merged.get(key, 0)) + int(value)
            self._conn.execute(
                "UPDATE sessions SET usage_json = ?, updated_at = ? WHERE id = ?",
                (json.dumps(merged), time.time(), session_id),
            )
            self._conn.commit()

    def touch_session(self, session_id: str, status: str | None = None) -> None:
        with self._lock:
            if status:
                self._conn.execute(
                    "UPDATE sessions SET updated_at = ?, status = ? WHERE id = ?",
                    (time.time(), status, session_id),
                )
            else:
                self._conn.execute(
                    "UPDATE sessions SET updated_at = ? WHERE id = ?",
                    (time.time(), session_id),
                )
            self._conn.commit()

    def get_session(self, session_id: str) -> dict[str, Any] | None:
        with self._lock:
            row = self._conn.execute("SELECT * FROM sessions WHERE id = ?", (session_id,)).fetchone()
        return dict(row) if row else None

    def list_sessions(self, limit: int = 50) -> list[dict[str, Any]]:
        with self._lock:
            rows = self._conn.execute(
                "SELECT * FROM sessions ORDER BY updated_at DESC LIMIT ?", (limit,)
            ).fetchall()
        return [dict(row) for row in rows]

    def create_task(self, session_id: str, prompt: str) -> str:
        tid = uuid.uuid4().hex
        with self._lock:
            self._conn.execute(
                "INSERT INTO tasks(id, session_id, prompt, status, created_at) VALUES (?, ?, ?, ?, ?)",
                (tid, session_id, prompt, "running", time.time()),
            )
            self._conn.commit()
        return tid

    def finish_task(self, task_id: str, status: str, summary: str | None = None) -> None:
        with self._lock:
            self._conn.execute(
                "UPDATE tasks SET status = ?, summary = ?, completed_at = ? WHERE id = ?",
                (status, summary, time.time(), task_id),
            )
            self._conn.commit()

    def get_task(self, task_id: str) -> dict[str, Any] | None:
        with self._lock:
            row = self._conn.execute("SELECT * FROM tasks WHERE id = ?", (task_id,)).fetchone()
        return dict(row) if row else None

    def list_tasks(self, session_id: str | None = None, limit: int = 50) -> list[dict[str, Any]]:
        with self._lock:
            if session_id:
                rows = self._conn.execute(
                    "SELECT * FROM tasks WHERE session_id = ? ORDER BY created_at DESC LIMIT ?",
                    (session_id, limit),
                ).fetchall()
            else:
                rows = self._conn.execute(
                    "SELECT * FROM tasks ORDER BY created_at DESC LIMIT ?", (limit,)
                ).fetchall()
        return [dict(row) for row in rows]

    def add_event(
        self,
        event_type: str,
        payload: dict[str, Any],
        session_id: str | None = None,
        task_id: str | None = None,
        ts: float | None = None,
    ) -> int:
        with self._lock:
            cur = self._conn.execute(
                "INSERT INTO events(ts, type, session_id, task_id, payload) VALUES (?, ?, ?, ?, ?)",
                (ts or time.time(), event_type, session_id, task_id, json.dumps(payload, default=str)),
            )
            self._conn.commit()
            return int(cur.lastrowid)

    def list_events(
        self,
        session_id: str | None = None,
        event_type: str | None = None,
        limit: int = 200,
    ) -> list[dict[str, Any]]:
        sql = "SELECT * FROM events"
        clauses: list[str] = []
        args: list[Any] = []
        if session_id:
            clauses.append("session_id = ?")
            args.append(session_id)
        if event_type:
            clauses.append("type = ?")
            args.append(event_type)
        if clauses:
            sql += " WHERE " + " AND ".join(clauses)
        sql += " ORDER BY id DESC LIMIT ?"
        args.append(limit)
        with self._lock:
            rows = self._conn.execute(sql, args).fetchall()
        out = []
        for row in rows:
            item = dict(row)
            item["payload"] = json.loads(item["payload"])
            out.append(item)
        return list(reversed(out))

    def upsert_model(
        self,
        model_id: str,
        name: str,
        provider: str,
        endpoint: str = "",
        context_limit: int = 128000,
        metadata: dict[str, Any] | None = None,
    ) -> None:
        with self._lock:
            self._conn.execute(
                "INSERT INTO models(id, name, provider, endpoint, context_limit, metadata) "
                "VALUES (?, ?, ?, ?, ?, ?) "
                "ON CONFLICT(id) DO UPDATE SET name=excluded.name, provider=excluded.provider, "
                "endpoint=excluded.endpoint, context_limit=excluded.context_limit, "
                "metadata=excluded.metadata",
                (model_id, name, provider, endpoint, context_limit, json.dumps(metadata or {})),
            )
            self._conn.commit()

    def list_models(self) -> list[dict[str, Any]]:
        with self._lock:
            rows = self._conn.execute("SELECT * FROM models ORDER BY name").fetchall()
        out = []
        for row in rows:
            item = dict(row)
            item["metadata"] = json.loads(item["metadata"] or "{}")
            out.append(item)
        return out

    def touch_project(self, workspace: Path) -> str:
        workspace = workspace.resolve()
        with self._lock:
            row = self._conn.execute(
                "SELECT id FROM projects WHERE path = ?", (str(workspace),)
            ).fetchone()
            now = time.time()
            if row:
                self._conn.execute(
                    "UPDATE projects SET last_opened = ? WHERE id = ?", (now, row["id"])
                )
                self._conn.commit()
                return str(row["id"])
            pid = uuid.uuid4().hex
            self._conn.execute(
                "INSERT INTO projects(id, path, name, last_opened) VALUES (?, ?, ?, ?)",
                (pid, str(workspace), workspace.name, now),
            )
            self._conn.commit()
            return pid

    def list_projects(self, limit: int = 30) -> list[dict[str, Any]]:
        with self._lock:
            rows = self._conn.execute(
                "SELECT * FROM projects ORDER BY last_opened DESC LIMIT ?", (limit,)
            ).fetchall()
        return [dict(row) for row in rows]
