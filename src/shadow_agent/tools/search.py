from __future__ import annotations

import json
import re
import shutil
import subprocess
from pathlib import Path

from shadow_agent.models.types import ToolCall, ToolResult
from shadow_agent.tools.sandbox import SandboxError, WorkspaceSandbox

SKIP_DIRS = {".git", "__pycache__", "node_modules", ".venv", "dist", "build", ".godot"}


def register_search_tools(registry: object, sandbox: WorkspaceSandbox) -> None:
    from shadow_agent.tools.registry import ToolRegistry

    assert isinstance(registry, ToolRegistry)
    registry.add(
        "search_files",
        "Find filenames matching a glob or substring.",
        {
            "type": "object",
            "properties": {"query": {"type": "string"}, "path": {"type": "string", "default": "."}},
            "required": ["query"],
        },
        lambda call: search_files(sandbox, call),
        read_only=True,
    )
    registry.add(
        "search_text",
        "Search file contents (ripgrep when available).",
        {
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "path": {"type": "string", "default": "."},
                "max_hits": {"type": "integer", "default": 80},
            },
            "required": ["query"],
        },
        lambda call: search_text(sandbox, call),
        read_only=True,
    )
    registry.add(
        "search_symbol",
        "Find likely symbol definitions (def/class/function).",
        {
            "type": "object",
            "properties": {"query": {"type": "string"}, "path": {"type": "string", "default": "."}},
            "required": ["query"],
        },
        lambda call: search_symbol(sandbox, call),
        read_only=True,
    )


def search_files(sandbox: WorkspaceSandbox, call: ToolCall) -> ToolResult:
    query = str(call.arguments.get("query") or "").lower()
    try:
        root = sandbox.resolve(str(call.arguments.get("path") or "."))
    except SandboxError as exc:
        return ToolResult(id=call.id, success=False, error=str(exc))
    hits: list[str] = []
    for path in root.rglob("*"):
        if any(part in SKIP_DIRS for part in path.parts):
            continue
        if query in path.name.lower():
            hits.append(sandbox.relative(path))
        if len(hits) >= 200:
            break
    return ToolResult(id=call.id, success=True, output=json.dumps({"hits": hits}))


def search_text(sandbox: WorkspaceSandbox, call: ToolCall) -> ToolResult:
    query = str(call.arguments.get("query") or "")
    max_hits = int(call.arguments.get("max_hits") or 80)
    try:
        root = sandbox.resolve(str(call.arguments.get("path") or "."))
    except SandboxError as exc:
        return ToolResult(id=call.id, success=False, error=str(exc))
    if shutil.which("rg"):
        proc = subprocess.run(
            ["rg", "-n", "-F", "--no-heading", "-m", str(max_hits), query, str(root)],
            capture_output=True,
            text=True,
            check=False,
        )
        lines = [line for line in proc.stdout.splitlines() if line][:max_hits]
        return ToolResult(id=call.id, success=True, output=json.dumps({"hits": lines, "engine": "rg"}))
    pattern = re.compile(re.escape(query), re.I)
    hits: list[str] = []
    for path in root.rglob("*"):
        if not path.is_file() or any(part in SKIP_DIRS for part in path.parts):
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError):
            continue
        for idx, line in enumerate(text.splitlines(), 1):
            if pattern.search(line):
                hits.append(f"{sandbox.relative(path)}:{idx}:{line.strip()}")
                if len(hits) >= max_hits:
                    return ToolResult(id=call.id, success=True, output=json.dumps({"hits": hits, "engine": "scan"}))
    return ToolResult(id=call.id, success=True, output=json.dumps({"hits": hits, "engine": "scan"}))


def search_symbol(sandbox: WorkspaceSandbox, call: ToolCall) -> ToolResult:
    name = str(call.arguments.get("query") or "")
    try:
        root = sandbox.resolve(str(call.arguments.get("path") or "."))
    except SandboxError as exc:
        return ToolResult(id=call.id, success=False, error=str(exc))
    pattern = re.compile(rf"^\s*(def|class|function|fn|const|let|var)\s+{re.escape(name)}\b")
    hits: list[str] = []
    for path in root.rglob("*"):
        if not path.is_file() or any(part in SKIP_DIRS for part in path.parts):
            continue
        if path.suffix not in {".py", ".js", ".ts", ".tsx", ".rs", ".go", ".java", ".gd"}:
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError):
            continue
        for idx, line in enumerate(text.splitlines(), 1):
            if pattern.search(line):
                hits.append(f"{sandbox.relative(path)}:{idx}:{line.strip()}")
    return ToolResult(id=call.id, success=True, output=json.dumps({"hits": hits}))
