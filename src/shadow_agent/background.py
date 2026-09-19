"""Background task manager — long-running work that continues while the agent does other things.

``/background`` and ``shadow background`` show:

    #1 npm dev server     RUNNING   pid 12345
    #2 pytest             RUNNING   pid 12346
    #3 ShadowCode agent   RUNNING   pid 12347
    #4 Docker             RUNNING   pid 12348

Tasks are tracked in SQLite so they survive a UI restart. The manager owns
the process lifecycle (start / list / stop / read output) and exposes a
simple status panel for the UI and CLI.
"""

from __future__ import annotations

import json
import os
import signal
import sqlite3
import subprocess
import threading
import time
import uuid
from collections.abc import Callable
from dataclasses import dataclass, field
from enum import Enum
from pathlib import Path
from typing import Any

from shadow_agent import paths


class TaskStatus(str, Enum):
    RUNNING = "RUNNING"
    COMPLETED = "COMPLETED"
    FAILED = "FAILED"
    CANCELLED = "CANCELLED"


@dataclass
class BackgroundTask:
    id: str
    name: str
    command: str
    cwd: str
    status: TaskStatus = TaskStatus.RUNNING
    pid: int = 0
    started_at: float = 0.0
    ended_at: float = 0.0
    exit_code: int | None = None
    output: str = ""
    error: str = ""

    def to_dict(self) -> dict[str, Any]:
        return {
            "id": self.id,
            "name": self.name,
            "command": self.command,
            "cwd": self.cwd,
            "status": self.status.value,
            "pid": self.pid,
            "started_at": self.started_at,
            "ended_at": self.ended_at,
            "exit_code": self.exit_code,
            "output": self.output[-4000:],
            "error": self.error,
        }


class BackgroundManager:
    """Tracks subprocesses that run alongside the agent loop.

    Persistence: a small SQLite table under ``state_dir/background.db`` so the
    panel can show history across restarts. Live process handles live in
    memory; on startup we mark any orphaned RUNNING rows as CANCELLED.
    """

    def __init__(self, state_dir: Path | None = None) -> None:
        self.root = state_dir or paths.state_dir()
        self.root.mkdir(parents=True, exist_ok=True)
        self.db = self.root / "background.db"
        self._lock = threading.Lock()
        self._procs: dict[str, subprocess.Popen] = {}
        self._readers: dict[str, threading.Thread] = {}
        self._init_db()
        self._reap_orphans()

    def _init_db(self) -> None:
        with self._lock:
            conn = sqlite3.connect(self.db)
            conn.execute(
                """
                CREATE TABLE IF NOT EXISTS tasks (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    command TEXT NOT NULL,
                    cwd TEXT NOT NULL,
                    status TEXT NOT NULL,
                    pid INTEGER,
                    started_at REAL,
                    ended_at REAL,
                    exit_code INTEGER,
                    output TEXT,
                    error TEXT
                )
                """
            )
            conn.commit()
            conn.close()

    def _reap_orphans(self) -> None:
        with self._lock:
            conn = sqlite3.connect(self.db)
            conn.execute("UPDATE tasks SET status='CANCELLED' WHERE status='RUNNING'")
            conn.commit()
            conn.close()

    def start(self, name: str, command: str, cwd: Path | None = None) -> BackgroundTask:
        task = BackgroundTask(
            id=uuid.uuid4().hex[:8],
            name=name,
            command=command,
            cwd=str(cwd or Path.cwd()),
            started_at=time.time(),
        )
        try:
            proc = subprocess.Popen(  # noqa: S603
                command,
                shell=True,
                cwd=task.cwd,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
                bufsize=1,
                preexec_fn=os.setsid if hasattr(os, "setsid") else None,
            )
        except OSError as exc:
            task.status = TaskStatus.FAILED
            task.error = str(exc)
            task.ended_at = time.time()
            self._persist(task)
            return task
        task.pid = proc.pid
        self._procs[task.id] = proc
        self._persist(task)
        reader = threading.Thread(target=self._reader, args=(task.id, proc), daemon=True)
        self._readers[task.id] = reader
        reader.start()
        return task

    def _reader(self, task_id: str, proc: subprocess.Popen) -> None:
        try:
            assert proc.stdout is not None
            for line in proc.stdout:
                self._append_output(task_id, line)
        except Exception:  # noqa: BLE001
            pass
        rc = proc.wait()
        self._finalize(task_id, rc)

    def _append_output(self, task_id: str, line: str) -> None:
        with self._lock:
            conn = sqlite3.connect(self.db)
            conn.execute("UPDATE tasks SET output = output || ? WHERE id = ?", (line, task_id))
            conn.commit()
            conn.close()

    def _finalize(self, task_id: str, exit_code: int) -> None:
        with self._lock:
            conn = sqlite3.connect(self.db)
            status = TaskStatus.COMPLETED if exit_code == 0 else TaskStatus.FAILED
            conn.execute(
                "UPDATE tasks SET status = ?, ended_at = ?, exit_code = ? WHERE id = ?",
                (status.value, time.time(), exit_code, task_id),
            )
            conn.commit()
            conn.close()
        self._procs.pop(task_id, None)

    def _persist(self, task: BackgroundTask) -> None:
        with self._lock:
            conn = sqlite3.connect(self.db)
            conn.execute(
                "INSERT OR REPLACE INTO tasks(id, name, command, cwd, status, pid, started_at, ended_at, exit_code, output, error) "
                "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                (task.id, task.name, task.command, task.cwd, task.status.value, task.pid, task.started_at, task.ended_at, task.exit_code, task.output, task.error),
            )
            conn.commit()
            conn.close()

    def list(self) -> list[BackgroundTask]:
        with self._lock:
            conn = sqlite3.connect(self.db)
            conn.row_factory = sqlite3.Row
            rows = conn.execute("SELECT * FROM tasks ORDER BY started_at DESC LIMIT 100").fetchall()
            conn.close()
        return [BackgroundTask(
            id=r["id"],
            name=r["name"],
            command=r["command"],
            cwd=r["cwd"],
            status=TaskStatus(r["status"]),
            pid=r["pid"] or 0,
            started_at=r["started_at"] or 0.0,
            ended_at=r["ended_at"] or 0.0,
            exit_code=r["exit_code"],
            output=r["output"] or "",
            error=r["error"] or "",
        ) for r in rows]

    def get(self, task_id: str) -> BackgroundTask | None:
        with self._lock:
            conn = sqlite3.connect(self.db)
            conn.row_factory = sqlite3.Row
            row = conn.execute("SELECT * FROM tasks WHERE id = ?", (task_id,)).fetchone()
            conn.close()
        if not row:
            return None
        return BackgroundTask(
            id=row["id"],
            name=row["name"],
            command=row["command"],
            cwd=row["cwd"],
            status=TaskStatus(row["status"]),
            pid=row["pid"] or 0,
            started_at=row["started_at"] or 0.0,
            ended_at=row["ended_at"] or 0.0,
            exit_code=row["exit_code"],
            output=row["output"] or "",
            error=row["error"] or "",
        )

    def stop(self, task_id: str) -> BackgroundTask | None:
        proc = self._procs.get(task_id)
        if proc is not None and proc.poll() is None:
            try:
                if hasattr(os, "killpg"):
                    os.killpg(os.getpgid(proc.pid), signal.SIGTERM)
                else:
                    proc.terminate()
                try:
                    proc.wait(timeout=2.0)
                except subprocess.TimeoutExpired:
                    if hasattr(os, "killpg"):
                        os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
                    else:
                        proc.kill()
            except Exception:  # noqa: BLE001
                pass
        with self._lock:
            conn = sqlite3.connect(self.db)
            conn.execute("UPDATE tasks SET status = ?, ended_at = ? WHERE id = ?", (TaskStatus.CANCELLED.value, time.time(), task_id))
            conn.commit()
            conn.close()
        self._procs.pop(task_id, None)
        return self.get(task_id)

    def render_panel(self) -> str:
        tasks = self.list()
        if not tasks:
            return "No background tasks."
        lines = []
        for i, t in enumerate(tasks, 1):
            tag = t.status.value
            pid = f"pid {t.pid}" if t.pid else "—"
            lines.append(f"#{i} {t.name:20} {tag:10} {pid}")
        return "\n".join(lines)
