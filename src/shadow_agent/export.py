"""Transcript export for sessions (Markdown / JSON).

The Markdown export reads like the desktop transcript: each task is a
heading with the prompt, the collapsed op-card one-liners the agent produced,
and the final result. A compact event appendix follows for auditing.
"""

from __future__ import annotations

import json
import time
from typing import Any

from shadow_agent.store import Store

_CARD_EVENTS = {"tool.completed"}
_STAGE_EVENTS = {"agent.understand", "agent.plan", "agent.inspect", "agent.act", "agent.observe", "agent.verify", "agent.fix"}


def _stamp(ts: float | None) -> str:
    if not ts:
        return ""
    return time.strftime("%Y-%m-%d %H:%M", time.localtime(float(ts)))


def export_session(store: Store, session_id: str, fmt: str = "md") -> tuple[str, str]:
    session = store.get_session(session_id)
    if not session:
        raise KeyError(session_id)
    tasks = store.list_tasks(session_id, limit=200)
    events = store.list_events(session_id=session_id, limit=2000)
    if fmt == "json":
        body = json.dumps({"session": session, "tasks": tasks, "events": events}, indent=2, default=str)
        return body, "application/json"

    title = session.get("title") or "Untitled session"
    lines = [
        f"# ShadowCode session — {title}",
        "",
        f"- id: `{session_id}`",
        f"- workspace: `{session.get('workspace')}`",
        f"- model: `{session.get('model_id') or ''}`",
        f"- status: {session.get('status')}",
        f"- created: {_stamp(session.get('created_at'))}",
        "",
        "## Tasks",
        "",
    ]
    by_task: dict[str, list[dict[str, Any]]] = {}
    for event in events:
        by_task.setdefault(str(event.get("task_id") or ""), []).append(event)

    # Oldest first so the transcript reads top-to-bottom.
    for task in reversed(tasks):
        prompt = str(task.get("prompt") or "").strip()
        lines.append(f"### {_stamp(task.get('created_at'))} · {task.get('status')}")
        lines.append("")
        lines.append("**You**")
        lines.append("")
        lines.append("> " + prompt.replace("\n", "\n> "))
        lines.append("")
        cards = [e for e in by_task.get(str(task.get("id")), []) if e.get("type") in _CARD_EVENTS]
        if cards:
            lines.append("**Agent actions**")
            lines.append("")
            for e in cards:
                payload = e.get("payload") or {}
                icon = payload.get("icon") or ("✓" if payload.get("success") else "✗")
                headline = payload.get("headline") or payload.get("tool") or "tool"
                lines.append(f"- {icon} {headline}")
            lines.append("")
        if task.get("summary"):
            lines.append("**Result**")
            lines.append("")
            lines.append(str(task["summary"]).strip())
            lines.append("")
    if not tasks:
        lines.append("_No tasks yet._")
        lines.append("")

    lines += ["## Event log", ""]
    for event in events:
        payload = event.get("payload") or {}
        preview = json.dumps(payload, default=str)
        if len(preview) > 240:
            preview = preview[:237] + "…"
        lines.append(f"- `{event.get('type')}` {preview}")
    return "\n".join(lines) + "\n", "text/markdown"
