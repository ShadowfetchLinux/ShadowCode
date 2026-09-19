"""/why — explainability for a change.

Given a change (a file path or a recent task), produce:
  - which files changed
  - the reason (derived from the agent's plan + tool history)
  - related issues/commits (from git log -- <path>)

This is a deterministic explainer: it reads the SQLite event log for the
session/task, the plan, and `git log` for the touched paths. A model can
refine the explanation, but the harness always produces a baseline.
"""

from __future__ import annotations

import json
import re
import subprocess
from pathlib import Path
from typing import Any

from shadow_agent.store import Store


def _git_log(workspace: Path, path: str, limit: int = 5) -> list[dict[str, str]]:
    if not (workspace / ".git").is_dir():
        return []
    proc = subprocess.run(
        ["git", "log", f"-{limit}", "--pretty=format:%h|%an|%ad|%s", "--date=short", "--", path],
        cwd=workspace, capture_output=True, text=True, check=False,
    )
    if proc.returncode != 0:
        return []
    out: list[dict[str, str]] = []
    for line in proc.stdout.splitlines():
        parts = line.split("|", 3)
        if len(parts) == 4:
            out.append({"hash": parts[0], "author": parts[1], "date": parts[2], "subject": parts[3]})
    return out


def _git_diff_names(workspace: Path) -> list[str]:
    if not (workspace / ".git").is_dir():
        return []
    proc = subprocess.run(["git", "diff", "--name-only"], cwd=workspace, capture_output=True, text=True, check=False)
    if proc.returncode != 0:
        return []
    return [line for line in proc.stdout.splitlines() if line]


def explain_change(workspace: Path, target: str | None = None, session_id: str | None = None, task_id: str | None = None) -> dict[str, Any]:
    """Build the /why explanation for a change.

    `target` is either a file path (the change to explain) or a task id.
    If `target` is None, explain the most recent task in the session.
    """
    store = Store()
    # Resolve the target task.
    if task_id is None and target and not target.startswith("/") and not Path(target).exists():
        # Treat as a task id prefix.
        rows = store.list_tasks(limit=50)
        match = next((r for r in rows if r["id"].startswith(target)), None)
        if match:
            task_id = match["id"]
            target = None
    if task_id is None:
        # Most recent task in this session (or most recent overall).
        rows = store.list_tasks(session_id=session_id, limit=1) if session_id else store.list_tasks(limit=1)
        if rows:
            task_id = rows[0]["id"]
    if task_id is None:
        return {"ok": False, "error": "no task found to explain"}

    task = store.get_task(task_id)
    if not task:
        return {"ok": False, "error": f"task {task_id} not found"}

    events = store.list_events(session_id=session_id, limit=500)
    # Filter to this task's events.
    task_events = [e for e in events if e.get("task_id") == task_id]
    # Collect tool calls (write/edit/exec) and their arguments.
    tool_calls: list[dict[str, Any]] = []
    plan_snapshot: dict[str, Any] = {}
    for e in task_events:
        etype = e["type"]
        payload = e.get("payload") or {}
        if etype == "tool.started" and payload.get("tool") in {"write_file", "edit_file", "apply_patch", "exec", "delete_file", "move_file"}:
            tool_calls.append({"tool": payload["tool"], "arguments": payload.get("arguments", {})})
        if etype == "agent.plan" or etype == "agent.planning":
            plan_snapshot = payload.get("plan", {}) or {}
        if etype == "plan.updated":
            plan_snapshot = payload.get("plan", {}) or {}

    # Determine the changed paths.
    if target:
        changed_paths = [target]
    else:
        changed_paths = sorted({str(c["arguments"].get("path") or "") for c in tool_calls if c["arguments"].get("path")})
        if not changed_paths:
            changed_paths = _git_diff_names(workspace)

    # Reason: derive from the plan steps + tool sequence.
    plan_steps = []
    if isinstance(plan_snapshot, dict):
        for step in plan_snapshot.get("steps", []) or []:
            plan_steps.append(f"{step.get('id', '?')}: {step.get('title', '')} [{step.get('status', '')}]")
    reason = (
        f"Task: {task['prompt'][:160]}\n"
        f"Plan: {'; '.join(plan_steps[:6]) or '(no plan recorded)'}\n"
        f"Tools used: {', '.join(sorted({c['tool'] for c in tool_calls})) or '(none)'}"
    )

    # Related commits per path.
    related: list[dict[str, Any]] = []
    for p in changed_paths[:8]:
        related.append({"path": p, "commits": _git_log(workspace, p)})

    return {
        "ok": True,
        "task_id": task_id,
        "task_prompt": task["prompt"],
        "task_status": task["status"],
        "task_summary": task.get("summary") or "",
        "changed_paths": changed_paths,
        "reason": reason,
        "plan": plan_snapshot,
        "tool_calls": tool_calls,
        "related": related,
    }


def render_explanation(expl: dict[str, Any]) -> str:
    if not expl.get("ok"):
        return f"/why: {expl.get('error', 'no explanation')}"
    lines = [f"/why  ·  task {expl['task_id'][:8]}", ""]
    lines.append(f"Task: {expl['task_prompt'][:160]}")
    lines.append(f"Status: {expl['task_status']}  ·  {expl.get('task_summary', '')[:120]}")
    lines.append("")
    lines.append("Changed files:")
    for p in expl.get("changed_paths", [])[:12]:
        lines.append(f"  • {p}")
    lines.append("")
    lines.append("Reason:")
    for line in expl["reason"].splitlines():
        lines.append(f"  {line}")
    lines.append("")
    lines.append("Related commits:")
    any_commit = False
    for r in expl.get("related", []):
        if not r.get("commits"):
            continue
        any_commit = True
        lines.append(f"  {r['path']}:")
        for c in r["commits"][:3]:
            lines.append(f"    {c['hash']} {c['date']} {c['subject'][:80]}")
    if not any_commit:
        lines.append("  (no related git history)")
    return "\n".join(lines)
