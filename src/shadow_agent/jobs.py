"""Long-running jobs — survive the UI closing.

A Job wraps an AgentRunner.run() call in a background thread, persists its
state to SQLite so `shadow jobs` can list it after the UI closes, and exposes
a `status` snapshot (project, agent active, runtime, milestone checklist).

This is a minimal daemon-mode: jobs run in-process threads, not separate OS
processes, so they live as long as the `shadow` process that started them.
`shadow jobs --daemon` starts a long-lived process that owns the job table;
the TUI/UI submit jobs to it via the existing HTTP API.
"""

from __future__ import annotations

import json
import sqlite3
import threading
import time
import uuid
from pathlib import Path
from typing import Any

from shadow_agent import paths


_JOBS_SCHEMA = """
CREATE TABLE IF NOT EXISTS jobs (
    id TEXT PRIMARY KEY,
    workspace TEXT NOT NULL,
    task TEXT NOT NULL,
    status TEXT NOT NULL,
    summary TEXT,
    created_at REAL NOT NULL,
    started_at REAL,
    finished_at REAL,
    runtime_sec REAL,
    goal_id TEXT,
    session_id TEXT,
    task_id TEXT,
    exit_ok INTEGER
);
"""


class JobRecord:
    def __init__(self, **kw: Any) -> None:
        self.id = kw.get("id") or uuid.uuid4().hex
        self.workspace = kw["workspace"]
        self.task = kw["task"]
        self.status = kw.get("status", "queued")
        self.summary = kw.get("summary", "")
        self.created_at = kw.get("created_at", time.time())
        self.started_at = kw.get("started_at")
        self.finished_at = kw.get("finished_at")
        self.runtime_sec = kw.get("runtime_sec")
        self.goal_id = kw.get("goal_id", "")
        self.session_id = kw.get("session_id", "")
        self.task_id = kw.get("task_id", "")
        self.exit_ok = kw.get("exit_ok")
        self._thread: threading.Thread | None = None
        self._runner = None  # AgentRunner, set when running
        self._cancel = threading.Event()

    def to_dict(self) -> dict[str, Any]:
        return {
            "id": self.id,
            "workspace": self.workspace,
            "task": self.task,
            "status": self.status,
            "summary": self.summary,
            "created_at": self.created_at,
            "started_at": self.started_at,
            "finished_at": self.finished_at,
            "runtime_sec": self.runtime_sec if self.runtime_sec is not None else self._runtime_now(),
            "goal_id": self.goal_id,
            "session_id": self.session_id,
            "task_id": self.task_id,
            "exit_ok": self.exit_ok,
        }

    def _runtime_now(self) -> float:
        if self.started_at is None:
            return 0.0
        end = self.finished_at or time.time()
        return round(end - self.started_at, 1)

    def snapshot(self) -> dict[str, Any]:
        """Status snapshot for `shadow status` / `shadow jobs`."""
        snap = self.to_dict()
        snap["agent_active"] = self.status == "running"
        snap["runtime"] = self._runtime_now()
        # Milestone checklist (if this job belongs to a goal).
        if self.goal_id:
            try:
                from shadow_agent.goal import GoalStore

                goal = GoalStore().get_goal(self.goal_id)
                if goal:
                    snap["milestones"] = [
                        {"title": m["title"], "status": m["status"]} for m in goal.get("milestones", [])
                    ]
                    snap["progress"] = goal.get("progress", 0.0)
            except Exception:
                snap["milestones"] = []
        return snap


class JobStore:
    """SQLite persistence for jobs (separate from sessions DB)."""

    def __init__(self, db_path: Path | None = None) -> None:
        self.path = db_path or paths.state_dir() / "jobs.db"
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self._lock = threading.Lock()
        self._conn = sqlite3.connect(str(self.path), check_same_thread=False)
        self._conn.row_factory = sqlite3.Row
        with self._lock:
            self._conn.executescript(_JOBS_SCHEMA)
            self._conn.commit()

    def save(self, job: JobRecord) -> None:
        d = job.to_dict()
        with self._lock:
            self._conn.execute(
                "INSERT INTO jobs(id, workspace, task, status, summary, created_at, started_at, finished_at, runtime_sec, goal_id, session_id, task_id, exit_ok) "
                "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) "
                "ON CONFLICT(id) DO UPDATE SET status=excluded.status, summary=excluded.summary, "
                "started_at=excluded.started_at, finished_at=excluded.finished_at, runtime_sec=excluded.runtime_sec, "
                "goal_id=excluded.goal_id, session_id=excluded.session_id, task_id=excluded.task_id, exit_ok=excluded.exit_ok",
                (d["id"], d["workspace"], d["task"], d["status"], d["summary"], d["created_at"], d["started_at"], d["finished_at"], d["runtime_sec"], d["goal_id"], d["session_id"], d["task_id"], d["exit_ok"]),
            )
            self._conn.commit()

    def list(self, workspace: Path | None = None, limit: int = 50) -> list[dict[str, Any]]:
        with self._lock:
            if workspace is not None:
                rows = self._conn.execute(
                    "SELECT * FROM jobs WHERE workspace = ? ORDER BY created_at DESC LIMIT ?",
                    (str(Path(workspace).resolve()), limit),
                ).fetchall()
            else:
                rows = self._conn.execute("SELECT * FROM jobs ORDER BY created_at DESC LIMIT ?", (limit,)).fetchall()
        return [dict(r) for r in rows]

    def get(self, job_id: str) -> dict[str, Any] | None:
        with self._lock:
            row = self._conn.execute("SELECT * FROM jobs WHERE id = ?", (job_id,)).fetchone()
        return dict(row) if row else None


class JobManager:
    """In-process job runner. Jobs run as daemon threads."""

    def __init__(self, store: JobStore | None = None) -> None:
        self.store = store or JobStore()
        self._jobs: dict[str, JobRecord] = {}
        self._lock = threading.Lock()

    def submit(self, workspace: Path, task: str, goal_id: str = "", session_id: str = "", work_until_tests_pass: bool = False) -> JobRecord:
        job = JobRecord(workspace=str(Path(workspace).resolve()), task=task, goal_id=goal_id, session_id=session_id)
        job.status = "queued"
        self.store.save(job)
        with self._lock:
            self._jobs[job.id] = job
        job._thread = threading.Thread(target=self._run, args=(job, work_until_tests_pass), daemon=True, name=f"shadow-job-{job.id[:8]}")
        job._thread.start()
        return job

    def _run(self, job: JobRecord, work_until_tests_pass: bool) -> None:
        from shadow_agent.agent.loop import AgentRunner
        from shadow_agent.config import load_config
        from shadow_agent.events import EventBus
        from shadow_agent.store import Store

        job.status = "running"
        job.started_at = time.time()
        self.store.save(job)
        workspace = Path(job.workspace)
        store = Store()
        bus = EventBus()
        cfg = load_config(workspace)
        try:
            runner = AgentRunner(workspace, config=cfg, store=store, events=bus, session_id=job.session_id or None)
            job._runner = runner
            result = runner.run(job.task)
            job.task_id = result.task_id
            job.session_id = result.session_id
            job.summary = result.summary
            job.exit_ok = 1 if result.success else 0
            # If asked to work until tests pass and they didn't, retry once more.
            if work_until_tests_pass and not result.success:
                result2 = runner.run(f"Continue: {job.task}. Make the tests pass.")
                job.summary = result2.summary
                job.exit_ok = 1 if result2.success else 0
        except Exception as exc:  # noqa: BLE001 - surface to job record
            job.summary = f"job error: {exc}"
            job.exit_ok = 0
        finally:
            job.status = "completed" if job.exit_ok else "failed"
            job.finished_at = time.time()
            job.runtime_sec = round(job.finished_at - (job.started_at or job.finished_at), 1)
            self.store.save(job)

    def cancel(self, job_id: str) -> bool:
        with self._lock:
            job = self._jobs.get(job_id)
        if job and job._runner is not None:
            job._runner.cancel()
            return True
        return False

    def list_active(self) -> list[JobRecord]:
        with self._lock:
            return [j for j in self._jobs.values() if j.status == "running"]

    def snapshot(self, workspace: Path | None = None) -> dict[str, Any]:
        """The `shadow status` payload: project, agent active, runtime, milestones."""
        rows = self.store.list(workspace=workspace, limit=20)
        active = [r for r in rows if r["status"] == "running"]
        return {
            "workspace": str(Path(workspace).resolve()) if workspace else "",
            "active_jobs": len(active),
            "jobs": rows,
        }


def render_status(snapshot: dict[str, Any]) -> str:
    lines = [f"shadow status  ·  {snapshot.get('workspace') or '(no workspace)'}"]
    lines.append(f"  active jobs: {snapshot['active_jobs']}")
    for job in snapshot.get("jobs", [])[:10]:
        mark = "▸" if job["status"] == "running" else ("✓" if job["exit_ok"] else "✗" if job["status"] == "failed" else "○")
        runtime = f"{job.get('runtime_sec') or 0:.1f}s"
        lines.append(f"  {mark} {job['id'][:8]}  {job['status']:10}  {runtime:7}  {job['task'][:60]}")
    return "\n".join(lines)
