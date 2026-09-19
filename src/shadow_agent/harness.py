"""Self-improving harness — record attempts and learn project preferences.

Every agent attempt is logged with:
  - the task
  - what was tried (the tool sequence)
  - what worked / what failed / why
  - the successful solution (if any)

Future runs consult this memory to avoid repeating failed approaches and to
prefer workflows that worked. Project preferences (pytest, Ruff, GTK4,
Meson, etc.) are stored as key/value pairs and surfaced into the system
prompt via `ProjectSkills`.

Storage: SQLite (`harness_attempts`, `project_preferences`) + a human-readable
`.shadow/memory/lessons.md` file.
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
from shadow_agent.context.memory import MemoryStore


_HARNESS_SCHEMA = """
CREATE TABLE IF NOT EXISTS harness_attempts (
    id TEXT PRIMARY KEY,
    workspace TEXT NOT NULL,
    task TEXT NOT NULL,
    outcome TEXT NOT NULL,
    approach TEXT,
    failure_reason TEXT,
    successful_solution TEXT,
    lessons TEXT,
    created_at REAL NOT NULL
);
CREATE TABLE IF NOT EXISTS project_preferences (
    workspace TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY(workspace, key)
);
"""


class HarnessStore:
    def __init__(self, db_path: Path | None = None) -> None:
        self.path = db_path or paths.state_dir() / "harness.db"
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self._lock = threading.Lock()
        self._conn = sqlite3.connect(str(self.path), check_same_thread=False)
        self._conn.row_factory = sqlite3.Row
        with self._lock:
            self._conn.executescript(_HARNESS_SCHEMA)
            self._conn.commit()

    def record_attempt(
        self,
        workspace: Path,
        task: str,
        outcome: str,  # success | failed
        approach: str = "",
        failure_reason: str = "",
        successful_solution: str = "",
        lessons: list[str] | None = None,
    ) -> str:
        aid = uuid.uuid4().hex
        with self._lock:
            self._conn.execute(
                "INSERT INTO harness_attempts(id, workspace, task, outcome, approach, failure_reason, successful_solution, lessons, created_at) "
                "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                (aid, str(Path(workspace).resolve()), task, outcome, approach, failure_reason, successful_solution, json.dumps(lessons or []), time.time()),
            )
            self._conn.commit()
        # Also append a human-readable lesson to .shadow/memory/lessons.md.
        memory = MemoryStore(workspace, "harness")
        lines = [f"- [{outcome}] {task[:120]}"]
        if approach:
            lines.append(f"  - tried: {approach[:200]}")
        if failure_reason:
            lines.append(f"  - failed because: {failure_reason[:200]}")
        if successful_solution:
            lines.append(f"  - solution: {successful_solution[:200]}")
        for lesson in lessons or []:
            lines.append(f"  - lesson: {lesson}")
        memory.append_project("\n".join(lines))
        return aid

    def list_attempts(self, workspace: Path, limit: int = 50) -> list[dict[str, Any]]:
        with self._lock:
            rows = self._conn.execute(
                "SELECT * FROM harness_attempts WHERE workspace = ? ORDER BY created_at DESC LIMIT ?",
                (str(Path(workspace).resolve()), limit),
            ).fetchall()
        out = []
        for row in rows:
            item = dict(row)
            try:
                item["lessons"] = json.loads(item.get("lessons") or "[]")
            except json.JSONDecodeError:
                item["lessons"] = []
            out.append(item)
        return out

    def similar_attempts(self, workspace: Path, task: str, limit: int = 5) -> list[dict[str, Any]]:
        """Find prior attempts whose task text overlaps the given task."""
        all_attempts = self.list_attempts(workspace, limit=200)
        words = set(task.lower().split())
        scored = []
        for a in all_attempts:
            a_words = set(a["task"].lower().split())
            overlap = len(words & a_words)
            if overlap:
                scored.append((overlap, a))
        scored.sort(key=lambda x: -x[0])
        return [a for _, a in scored[:limit]]

    def set_preference(self, workspace: Path, key: str, value: str) -> None:
        with self._lock:
            self._conn.execute(
                "INSERT INTO project_preferences(workspace, key, value) VALUES (?, ?, ?) "
                "ON CONFLICT(workspace, key) DO UPDATE SET value=excluded.value",
                (str(Path(workspace).resolve()), key, value),
            )
            self._conn.commit()

    def get_preferences(self, workspace: Path) -> dict[str, str]:
        with self._lock:
            rows = self._conn.execute(
                "SELECT key, value FROM project_preferences WHERE workspace = ?", (str(Path(workspace).resolve()),)
            ).fetchall()
        return {row["key"]: row["value"] for row in rows}

    def preferences_block(self, workspace: Path) -> str:
        """Render preferences as a system-prompt block."""
        prefs = self.get_preferences(workspace)
        if not prefs:
            return ""
        lines = ["Project preferences (learned by the harness):"]
        for key, value in sorted(prefs.items()):
            lines.append(f"- {key}: {value}")
        return "\n".join(lines)


def infer_preferences(workspace: Path) -> dict[str, str]:
    """Cheap, deterministic preference inference from the repo.

    Detects test runner, linter, framework, build system, and language — the
    things the harness should remember to avoid re-discovering every run.
    """
    from shadow_agent.understand import detect_stack

    stack = detect_stack(workspace)
    prefs: dict[str, str] = {}
    if stack["test"]:
        prefs["test_runner"] = stack["test"][0]
    if stack["build"]:
        prefs["build_system"] = stack["build"][0]
    if stack["frameworks"]:
        prefs["frameworks"] = ", ".join(stack["frameworks"])
    if stack["languages"]:
        prefs["languages"] = ", ".join(stack["languages"])
    return prefs


def learn_preferences(workspace: Path, store: "HarnessStore | None" = None) -> dict[str, str]:
    """Infer + persist preferences for the workspace."""
    s = store or HarnessStore()
    prefs = infer_preferences(workspace)
    for key, value in prefs.items():
        s.set_preference(workspace, key, value)
    return prefs
