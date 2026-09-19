from __future__ import annotations

import json
import subprocess

from shadow_agent.models.types import ToolCall, ToolResult
from shadow_agent.tools.sandbox import WorkspaceSandbox

DESTRUCTIVE = {"git_reset", "git_clean"}


def register_git_tools(registry: object, sandbox: WorkspaceSandbox) -> None:
    from shadow_agent.tools.registry import ToolRegistry

    assert isinstance(registry, ToolRegistry)
    registry.add("git_status", "Show git status in the workspace.", {"type": "object", "properties": {}}, lambda c: git_status(sandbox, c), read_only=True)
    registry.add("git_diff", "Show git diff (optionally staged).", {"type": "object", "properties": {"staged": {"type": "boolean"}}}, lambda c: git_diff(sandbox, c), read_only=True)
    registry.add("git_log", "Show recent commits.", {"type": "object", "properties": {"limit": {"type": "integer", "default": 12}}}, lambda c: git_log(sandbox, c), read_only=True)
    registry.add("git_branch", "List or create branches. Never deletes branches.", {"type": "object", "properties": {"name": {"type": "string"}, "create": {"type": "boolean"}}}, lambda c: git_branch(sandbox, c))
    registry.add("git_checkout", "Checkout a branch (refuses --force and detached destructive flags).", {"type": "object", "properties": {"ref": {"type": "string"}}, "required": ["ref"]}, lambda c: git_checkout(sandbox, c))
    registry.add("git_add", "Stage files (workspace paths only).", {"type": "object", "properties": {"paths": {"type": "array", "items": {"type": "string"}}}, "required": ["paths"]}, lambda c: git_add(sandbox, c))
    registry.add("git_commit", "Create a commit. Does not push. Never amends unless explicitly asked via message prefix.", {"type": "object", "properties": {"message": {"type": "string"}}, "required": ["message"]}, lambda c: git_commit(sandbox, c))


def git_status(sandbox: WorkspaceSandbox, call: ToolCall) -> ToolResult:
    return _run(sandbox, call, ["status", "--porcelain=v1", "-b"])


def _run(sandbox: WorkspaceSandbox, call: ToolCall, args: list[str]) -> ToolResult:
    try:
        proc = subprocess.run(
            ["git", *args],
            cwd=str(sandbox.root),
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        return ToolResult(id=call.id, success=False, error=str(exc))
    payload = {"args": args, "stdout": proc.stdout, "stderr": proc.stderr, "exit_code": proc.returncode}
    return ToolResult(
        id=call.id,
        success=proc.returncode == 0,
        output=json.dumps(payload),
        error=proc.stderr if proc.returncode != 0 else "",
        metadata={"exit_code": proc.returncode},
    )


def git_diff(sandbox: WorkspaceSandbox, call: ToolCall) -> ToolResult:
    args = ["diff"]
    if call.arguments.get("staged"):
        args.append("--cached")
    return _run(sandbox, call, args)


def git_log(sandbox: WorkspaceSandbox, call: ToolCall) -> ToolResult:
    limit = int(call.arguments.get("limit") or 12)
    return _run(sandbox, call, ["log", f"-{limit}", "--oneline", "--decorate"])


def git_branch(sandbox: WorkspaceSandbox, call: ToolCall) -> ToolResult:
    name = call.arguments.get("name")
    if name and call.arguments.get("create"):
        return _run(sandbox, call, ["checkout", "-b", str(name)])
    return _run(sandbox, call, ["branch", "-v"])


def git_checkout(sandbox: WorkspaceSandbox, call: ToolCall) -> ToolResult:
    ref = str(call.arguments.get("ref") or "")
    if not ref or ref.startswith("-"):
        return ToolResult(id=call.id, success=False, error="invalid ref")
    if "--force" in ref or "-f" == ref:
        return ToolResult(id=call.id, success=False, error="refusing force checkout")
    return _run(sandbox, call, ["checkout", ref])


def git_add(sandbox: WorkspaceSandbox, call: ToolCall) -> ToolResult:
    paths = call.arguments.get("paths") or []
    if not isinstance(paths, list) or not paths:
        return ToolResult(id=call.id, success=False, error="paths required")
    safe: list[str] = []
    for item in paths:
        resolved = sandbox.resolve(str(item))
        safe.append(sandbox.relative(resolved) if resolved.exists() else str(item))
    return _run(sandbox, call, ["add", "--", *safe])


def git_commit(sandbox: WorkspaceSandbox, call: ToolCall) -> ToolResult:
    message = str(call.arguments.get("message") or "").strip()
    if not message:
        return ToolResult(id=call.id, success=False, error="commit message required")
    if message.startswith("--"):
        return ToolResult(id=call.id, success=False, error="invalid commit message")
    return _run(sandbox, call, ["commit", "-m", message])
