"""MCP resources exposed by the ShadowCode server.

Schemes:
  - shadow://project/{path}   — project map for the given workspace path
  - shadow://sessions          — recent sessions (JSON)
  - shadow://plan              — current plan (text; workspace query param)
  - shadow://memory            — project memory (.shadow/memory/project.md)
"""

from __future__ import annotations

import json
import subprocess
from pathlib import Path
from typing import Any

from shadow_agent.config import load_config
from shadow_agent.context.memory import MemoryStore
from shadow_agent.planning.plan import initial_plan
from shadow_agent.store import Store


def _split_uri(uri: str) -> tuple[str, str]:
    """Return (scheme, rest) for a shadow:// URI."""
    if "://" not in uri:
        return "", uri
    scheme, rest = uri.split("://", 1)
    return scheme, rest


def list_resources(workspace: Path) -> list[dict[str, Any]]:
    """Return the static resource catalog (no path templating for now)."""
    base = "shadow://"
    return [
        {"uri": f"{base}project/{Path(workspace).resolve().as_posix().lstrip('/')}",
         "name": "project",
         "title": f"Project map for {workspace.name}",
         "mimeType": "application/json",
         "description": "Auto-generated project map (stack, modules, tech debt) for the active workspace."},
        {"uri": f"{base}sessions", "name": "sessions", "title": "Sessions",
         "mimeType": "application/json",
         "description": "Recent ShadowCode sessions (id, workspace, status, title, model_id)."},
        {"uri": f"{base}plan", "name": "plan", "title": "Plan",
         "mimeType": "text/markdown",
         "description": "Current plan + todos for the active workspace."},
        {"uri": f"{base}memory", "name": "memory", "title": "Project memory",
         "mimeType": "text/markdown",
         "description": "Project memory (.shadow/memory/project.md) for the active workspace."},
    ]


def read_resource(uri: str, workspace: Path, store: Store) -> tuple[str, str]:
    """Read a shadow:// resource. Returns (mimeType, text)."""
    scheme, rest = _split_uri(uri)
    if scheme != "shadow":
        raise ValueError(f"unsupported scheme: {scheme}")
    parts = rest.split("/", 1)
    kind = parts[0]
    arg = parts[1] if len(parts) > 1 else ""
    if kind == "project":
        from shadow_agent.understand import build_project_map
        target = Path("/" + arg) if arg and not arg.startswith("/") else Path(arg or str(workspace))
        mp = build_project_map(target if target.exists() else workspace)
        return "application/json", json.dumps(mp, indent=2, default=str)
    if kind == "sessions":
        rows = store.list_sessions(limit=50)
        return "application/json", json.dumps(rows, indent=2, default=str)
    if kind == "plan":
        # We do not persist a "current plan" outside a running task; return the
        # initial plan for the workspace's last task if any, else a stub.
        cfg = load_config(workspace)
        plan = initial_plan("(no active task — open a session and run a task to populate the plan)")
        return "text/markdown", plan.to_markdown()
    if kind == "memory":
        memory = MemoryStore(workspace, task_id="mcp-resource")
        return "text/markdown", memory.load_project() or "(no project memory yet)"
    raise ValueError(f"unknown shadow resource: {kind}")


__all__ = ["list_resources", "read_resource"]
