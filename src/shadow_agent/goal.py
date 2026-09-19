"""Goal Mode — turn a one-line instruction into a project with milestones + progress.

A Goal is a persistent, multi-milestone project the agent works through. The
agent loop runs each milestone as a task; milestones flip to `done` as their
VERIFY passes. Progress is the fraction of milestones done.

Storage: SQLite (`goals`, `milestones` tables) so goals survive the UI closing
and can be listed/resumed from the CLI (`shadow goal`, `shadow goals`).
"""

from __future__ import annotations

import json
import re
import sqlite3
import threading
import time
import uuid
from pathlib import Path
from typing import Any, Callable

from shadow_agent import paths
from shadow_agent.store import Store


_GOAL_SCHEMA = """
CREATE TABLE IF NOT EXISTS goals (
    id TEXT PRIMARY KEY,
    workspace TEXT NOT NULL,
    instruction TEXT NOT NULL,
    status TEXT NOT NULL,
    progress REAL NOT NULL,
    title TEXT,
    created_at REAL NOT NULL,
    updated_at REAL NOT NULL
);
CREATE TABLE IF NOT EXISTS milestones (
    id TEXT PRIMARY KEY,
    goal_id TEXT NOT NULL,
    title TEXT NOT NULL,
    status TEXT NOT NULL,
    order_index INTEGER NOT NULL,
    detail TEXT,
    task_id TEXT,
    created_at REAL NOT NULL,
    updated_at REAL NOT NULL,
    FOREIGN KEY(goal_id) REFERENCES goals(id)
);
"""


def _slug(text: str, n: int = 60) -> str:
    text = re.sub(r"[^A-Za-z0-9 _-]+", " ", text).strip()
    return text[:n] if text else "milestone"


def plan_milestones(instruction: str) -> list[dict[str, str]]:
    """Derive 3-7 milestone titles from a one-line instruction.

    This is a deterministic, model-agnostic planner: it splits the instruction
    into the canonical software-lifecycle phases (understand → scaffold →
    implement → test → verify → document) and keeps only the phases the
    instruction implies. A real model can override via `update_milestones`.
    """
    lowered = instruction.lower()
    titles: list[str] = []
    if any(w in lowered for w in ("understand", "analyze", "inspect", "map")):
        titles.append("Understand the project")
    if any(w in lowered for w in ("scaffold", "bootstrap", "create", "init", "new", "start")):
        titles.append("Scaffold the project structure")
    titles.append("Implement the core feature")
    if "test" in lowered or "pytest" in lowered or "verify" in lowered:
        titles.append("Write and run tests")
    else:
        titles.append("Add a smoke test")
    titles.append("Run the verification gate (build → test → lint → review)")
    if any(w in lowered for w in ("doc", "readme", "document")):
        titles.append("Document the result")
    # Cap to 3-7 milestones.
    while len(titles) < 3:
        titles.append(f"Step {len(titles) + 1}: complete the work")
    return [{"title": t} for t in titles[:7]]


class GoalStore:
    """SQLite-backed goal + milestone persistence (separate from sessions DB)."""

    def __init__(self, db_path: Path | None = None) -> None:
        self.path = db_path or paths.state_dir() / "goals.db"
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self._lock = threading.Lock()
        self._conn = sqlite3.connect(str(self.path), check_same_thread=False)
        self._conn.row_factory = sqlite3.Row
        with self._lock:
            self._conn.executescript(_GOAL_SCHEMA)
            self._conn.commit()

    def close(self) -> None:
        with self._lock:
            self._conn.close()

    def create_goal(self, workspace: Path, instruction: str, milestones: list[dict[str, str]] | None = None) -> dict[str, Any]:
        gid = uuid.uuid4().hex
        now = time.time()
        title = instruction.splitlines()[0][:80]
        ms = milestones or plan_milestones(instruction)
        with self._lock:
            self._conn.execute(
                "INSERT INTO goals(id, workspace, instruction, status, progress, title, created_at, updated_at) "
                "VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                (gid, str(Path(workspace).resolve()), instruction, "active", 0.0, title, now, now),
            )
            for idx, m in enumerate(ms):
                mid = uuid.uuid4().hex
                self._conn.execute(
                    "INSERT INTO milestones(id, goal_id, title, status, order_index, detail, task_id, created_at, updated_at) "
                    "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    (mid, gid, m.get("title", f"Milestone {idx + 1}"), "pending", idx, m.get("detail", ""), "", now, now),
                )
            self._conn.commit()
        return self.get_goal(gid)  # type: ignore[return-value]

    def get_goal(self, goal_id: str) -> dict[str, Any] | None:
        with self._lock:
            row = self._conn.execute("SELECT * FROM goals WHERE id = ?", (goal_id,)).fetchone()
            if not row:
                return None
            goal = dict(row)
            rows = self._conn.execute(
                "SELECT * FROM milestones WHERE goal_id = ? ORDER BY order_index ASC", (goal_id,)
            ).fetchall()
            goal["milestones"] = [dict(r) for r in rows]
            return goal

    def list_goals(self, workspace: Path | None = None, limit: int = 50) -> list[dict[str, Any]]:
        with self._lock:
            if workspace is not None:
                rows = self._conn.execute(
                    "SELECT * FROM goals WHERE workspace = ? ORDER BY updated_at DESC LIMIT ?",
                    (str(Path(workspace).resolve()), limit),
                ).fetchall()
            else:
                rows = self._conn.execute("SELECT * FROM goals ORDER BY updated_at DESC LIMIT ?", (limit,)).fetchall()
        return [self._with_milestones(r["id"]) for r in rows]

    def _with_milestones(self, goal_id: str) -> dict[str, Any]:
        goal = self.get_goal(goal_id)
        return goal or {"id": goal_id, "milestones": []}

    def update_milestone(self, goal_id: str, milestone_id: str | None, status: str, task_id: str = "", detail: str = "") -> dict[str, Any] | None:
        """Flip a milestone's status and recompute goal progress.

        Passing `milestone_id=None` updates the first pending milestone (used by
        the auto-advancing agent loop).
        """
        with self._lock:
            if milestone_id is None:
                row = self._conn.execute(
                    "SELECT id FROM milestones WHERE goal_id = ? AND status = 'pending' ORDER BY order_index ASC LIMIT 1",
                    (goal_id,),
                ).fetchone()
                if not row:
                    return self.get_goal(goal_id)
                milestone_id = row["id"]
            self._conn.execute(
                "UPDATE milestones SET status = ?, task_id = ?, detail = ?, updated_at = ? WHERE id = ?",
                (status, task_id, detail, time.time(), milestone_id),
            )
            # Recompute progress = done / total.
            total = self._conn.execute("SELECT COUNT(*) AS n FROM milestones WHERE goal_id = ?", (goal_id,)).fetchone()["n"]
            done = self._conn.execute(
                "SELECT COUNT(*) AS n FROM milestones WHERE goal_id = ? AND status = 'done'", (goal_id,)
            ).fetchone()["n"]
            progress = (done / total) if total else 0.0
            new_status = "completed" if (total and done == total) else "active"
            self._conn.execute(
                "UPDATE goals SET progress = ?, status = ?, updated_at = ? WHERE id = ?",
                (progress, new_status, time.time(), goal_id),
            )
            self._conn.commit()
        return self.get_goal(goal_id)

    def set_milestones(self, goal_id: str, milestones: list[dict[str, str]]) -> dict[str, Any] | None:
        """Replace the milestone list (used when a model revises the plan)."""
        now = time.time()
        with self._lock:
            self._conn.execute("DELETE FROM milestones WHERE goal_id = ?", (goal_id,))
            for idx, m in enumerate(milestones):
                mid = uuid.uuid4().hex
                self._conn.execute(
                    "INSERT INTO milestones(id, goal_id, title, status, order_index, detail, task_id, created_at, updated_at) "
                    "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    (mid, goal_id, m.get("title", f"Milestone {idx + 1}"), m.get("status", "pending"), idx, m.get("detail", ""), "", now, now),
                )
            self._conn.execute("UPDATE goals SET updated_at = ? WHERE id = ?", (now, goal_id))
            self._conn.commit()
        return self.get_goal(goal_id)

    def abandon(self, goal_id: str) -> None:
        with self._lock:
            self._conn.execute("UPDATE goals SET status = 'abandoned', updated_at = ? WHERE id = ?", (time.time(), goal_id))
            self._conn.commit()

    def reopen(self, goal_id: str) -> dict[str, Any] | None:
        """Make an abandoned/failed goal active again and reset failed milestones to pending."""
        with self._lock:
            self._conn.execute(
                "UPDATE milestones SET status = 'pending', updated_at = ? WHERE goal_id = ? AND status IN ('failed', 'in_progress')",
                (time.time(), goal_id),
            )
            self._conn.execute("UPDATE goals SET status = 'active', updated_at = ? WHERE id = ?", (time.time(), goal_id))
            self._conn.commit()
        return self.get_goal(goal_id)

    def delete(self, goal_id: str) -> bool:
        with self._lock:
            exists = self._conn.execute("SELECT 1 FROM goals WHERE id = ?", (goal_id,)).fetchone()
            if not exists:
                return False
            self._conn.execute("DELETE FROM milestones WHERE goal_id = ?", (goal_id,))
            self._conn.execute("DELETE FROM goals WHERE id = ?", (goal_id,))
            self._conn.commit()
        return True


def run_goal(
    store: GoalStore,
    goal_id: str,
    run_task: Callable[[str, str], dict[str, Any]],
    stop: Callable[[], bool] | None = None,
) -> dict[str, Any] | None:
    """Drive a goal to completion: run every non-done milestone as a task.

    ``run_task(task_text, milestone_id)`` must block and return a dict with at
    least ``success`` (bool); ``task_id`` and ``summary`` are recorded when
    present. Milestones flip pending → in_progress → done/failed as the
    agent's VERIFY stage reports. The run stops at the first failed milestone
    (or when ``stop()`` returns True) so the user can inspect and resume later
    — resuming simply calls ``run_goal`` again and skips ``done`` milestones.
    """
    goal = store.get_goal(goal_id)
    if not goal:
        return None
    for milestone in goal.get("milestones", []):
        if milestone["status"] == "done":
            continue
        if stop is not None and stop():
            break
        store.update_milestone(goal_id, milestone["id"], "in_progress")
        task_text = f"{goal['instruction']}\nMilestone: {milestone['title']}"
        try:
            outcome = run_task(task_text, milestone["id"]) or {}
        except Exception as exc:  # noqa: BLE001 - record and stop, never raise into the caller thread
            outcome = {"success": False, "summary": f"{exc.__class__.__name__}: {exc}"}
        new_status = "done" if outcome.get("success") else "failed"
        store.update_milestone(
            goal_id,
            milestone["id"],
            new_status,
            task_id=str(outcome.get("task_id") or ""),
            detail=str(outcome.get("summary") or "")[:200],
        )
        if new_status != "done":
            break
    return store.get_goal(goal_id)


def render_goal(goal: dict[str, Any]) -> str:
    """One-screen summary of a goal for the CLI/TUI."""
    lines = [
        f"Goal {goal['id'][:8]}  ·  {goal.get('title') or goal['instruction'][:80]}",
        f"  status: {goal['status']}  progress: {int(goal['progress'] * 100)}%",
    ]
    for m in goal.get("milestones", []):
        mark = {"done": "✓", "in_progress": "▸", "failed": "✗", "pending": "○"}.get(m["status"], "○")
        lines.append(f"    {mark} {m['title']}")
    return "\n".join(lines)
