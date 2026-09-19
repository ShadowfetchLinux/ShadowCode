"""Transcript export for sessions (Markdown / JSON)."""

from __future__ import annotations

import json
from typing import Any

from shadow_agent.store import Store


def export_session(store: Store, session_id: str, fmt: str = "md") -> tuple[str, str]:
    session = store.get_session(session_id)
    if not session:
        raise KeyError(session_id)
    tasks = store.list_tasks(session_id, limit=200)
    events = store.list_events(session_id=session_id, limit=2000)
    if fmt == "json":
        body = json.dumps({"session": session, "tasks": tasks, "events": events}, indent=2, default=str)
        return body, "application/json"
    lines = [
        f"# ShadowCode session",
        "",
        f"- id: `{session_id}`",
        f"- workspace: `{session.get('workspace')}`",
        f"- status: {session.get('status')}",
        f"- title: {session.get('title') or ''}",
        "",
        "## Tasks",
        "",
    ]
    for task in tasks:
        lines.append(f"### {task.get('status')} — {task.get('prompt', '')[:120]}")
        if task.get("summary"):
            lines.append("")
            lines.append(task["summary"])
            lines.append("")
    lines += ["## Event log", ""]
    for event in events:
        payload = event.get("payload") or {}
        preview = json.dumps(payload, default=str)
        if len(preview) > 240:
            preview = preview[:237] + "…"
        lines.append(f"- `{event.get('type')}` {preview}")
    return "\n".join(lines) + "\n", "text/markdown"
