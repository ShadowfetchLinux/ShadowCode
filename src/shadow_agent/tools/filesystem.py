from __future__ import annotations

import json

from shadow_agent.models.types import ToolCall, ToolResult
from shadow_agent.tools.sandbox import SandboxError, WorkspaceSandbox


def register_fs_tools(registry: object, sandbox: WorkspaceSandbox) -> None:
    from shadow_agent.tools.registry import ToolRegistry

    assert isinstance(registry, ToolRegistry)
    registry.add(
        "list_files",
        "List files under a workspace-relative path.",
        {
            "type": "object",
            "properties": {
                "path": {"type": "string", "default": "."},
                "max_entries": {"type": "integer", "default": 400},
            },
        },
        lambda call: list_files(sandbox, call),
        read_only=True,
    )
    registry.add(
        "read_file",
        "Read a text file. Optional 1-based offset/limit for large files.",
        {
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "offset": {"type": "integer", "default": 1},
                "limit": {"type": "integer", "default": 400},
            },
            "required": ["path"],
        },
        lambda call: read_file(sandbox, call),
        read_only=True,
    )
    registry.add(
        "write_file",
        "Create or overwrite a text file in the workspace.",
        {
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "content": {"type": "string"},
            },
            "required": ["path", "content"],
        },
        lambda call: write_file(sandbox, call),
    )
    registry.add(
        "apply_patch",
        "Apply a unified diff or *** Begin Patch block to workspace files. Prefer this for multi-hunk edits.",
        {
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Optional path when the patch has no file headers."},
                "patch": {"type": "string"},
                "diff": {"type": "string"},
            },
        },
        lambda call: _apply_patch(sandbox, call),
    )
    registry.add(
        "edit_file",
        "Structured patch: replace a unique old_string, or apply line hunks. Never a blind full rewrite.",
        {
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "old_string": {"type": "string"},
                "new_string": {"type": "string"},
                "hunks": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "start_line": {"type": "integer"},
                            "end_line": {"type": "integer"},
                            "replacement": {"type": "string"},
                        },
                        "required": ["start_line", "end_line", "replacement"],
                    },
                },
            },
            "required": ["path"],
        },
        lambda call: edit_file(sandbox, call),
    )
    registry.add(
        "create_directory",
        "Create a directory (and parents) inside the workspace.",
        {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]},
        lambda call: create_directory(sandbox, call),
    )
    registry.add(
        "move_file",
        "Move or rename a file inside the workspace.",
        {
            "type": "object",
            "properties": {"src": {"type": "string"}, "dest": {"type": "string"}},
            "required": ["src", "dest"],
        },
        lambda call: move_file(sandbox, call),
    )
    registry.add(
        "delete_file",
        "Delete a file (not a directory) inside the workspace.",
        {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]},
        lambda call: delete_file(sandbox, call),
    )


def _fail(call: ToolCall, error: str) -> ToolResult:
    return ToolResult(id=call.id, success=False, error=error)


def list_files(sandbox: WorkspaceSandbox, call: ToolCall) -> ToolResult:
    rel = str(call.arguments.get("path") or ".")
    max_entries = int(call.arguments.get("max_entries") or 400)
    try:
        root = sandbox.resolve(rel)
    except SandboxError as exc:
        return _fail(call, str(exc))
    if not root.exists():
        return _fail(call, f"not found: {rel}")
    entries: list[dict[str, str]] = []
    if root.is_file():
        entries.append({"path": sandbox.relative(root), "type": "file"})
    else:
        for path in sorted(root.rglob("*")):
            if any(part in {".git", "__pycache__", "node_modules", ".venv"} for part in path.parts):
                continue
            entries.append({"path": sandbox.relative(path), "type": "dir" if path.is_dir() else "file"})
            if len(entries) >= max_entries:
                break
    return ToolResult(
        id=call.id,
        success=True,
        output=json.dumps({"path": rel, "entries": entries}),
        metadata={"count": len(entries)},
    )


def read_file(sandbox: WorkspaceSandbox, call: ToolCall) -> ToolResult:
    rel = str(call.arguments.get("path") or "")
    offset = max(1, int(call.arguments.get("offset") or 1))
    limit = max(1, int(call.arguments.get("limit") or 400))
    try:
        path = sandbox.resolve(rel, must_exist=True)
    except SandboxError as exc:
        return _fail(call, str(exc))
    if not path.is_file():
        return _fail(call, f"not a file: {rel}")
    try:
        text = path.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        return _fail(call, "binary file; refusing to read")
    lines = text.splitlines(keepends=True)
    chunk = "".join(lines[offset - 1 : offset - 1 + limit])
    payload = {
        "path": sandbox.relative(path),
        "content": chunk,
        "offset": offset,
        "total_lines": len(lines),
    }
    return ToolResult(id=call.id, success=True, output=json.dumps(payload), metadata={"path": sandbox.relative(path)})


def write_file(sandbox: WorkspaceSandbox, call: ToolCall) -> ToolResult:
    rel = str(call.arguments.get("path") or "")
    content = call.arguments.get("content")
    if content is None:
        return _fail(call, "content is required")
    try:
        path = sandbox.resolve(rel)
    except SandboxError as exc:
        return _fail(call, str(exc))
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(str(content), encoding="utf-8")
    return ToolResult(id=call.id, success=True, output=json.dumps({"path": sandbox.relative(path), "bytes": path.stat().st_size}))


def edit_file(sandbox: WorkspaceSandbox, call: ToolCall) -> ToolResult:
    rel = str(call.arguments.get("path") or "")
    try:
        path = sandbox.resolve(rel, must_exist=True)
    except SandboxError as exc:
        return _fail(call, str(exc))
    if not path.is_file():
        return _fail(call, f"not a file: {rel}")
    text = path.read_text(encoding="utf-8")
    old = call.arguments.get("old_string")
    new = call.arguments.get("new_string")
    hunks = call.arguments.get("hunks")
    if old is not None:
        if new is None:
            return _fail(call, "new_string is required with old_string")
        updated, err = _replace_unique(text, str(old), str(new))
        if updated is None:
            return _fail(call, err)
    elif hunks:
        updated = _apply_hunks(text, hunks)
        if updated is None:
            return _fail(call, "invalid hunks")
    else:
        return _fail(call, "provide old_string/new_string or hunks")
    path.write_text(updated, encoding="utf-8")
    return ToolResult(id=call.id, success=True, output=json.dumps({"path": sandbox.relative(path), "patched": True}))


def _apply_patch(sandbox: WorkspaceSandbox, call: ToolCall) -> ToolResult:
    from shadow_agent.tools.patch import apply_patch

    return apply_patch(sandbox, call)


def _replace_unique(text: str, old: str, new: str) -> tuple[str | None, str]:
    if old in text:
        count = text.count(old)
        if count == 0:
            return None, "old_string not found"
        if count > 1:
            return None, "old_string is not unique"
        return text.replace(old, new, 1), ""
    # Whitespace-tolerant unique match (trailing spaces / newline style).
    old_lines = old.splitlines()
    hay = text.splitlines(keepends=True)
    needle = [line.strip() for line in old_lines]
    if not needle:
        return None, "old_string not found"
    hits: list[int] = []
    for idx in range(0, len(hay) - len(needle) + 1):
        window = [hay[idx + offset].rstrip("\n").strip() for offset in range(len(needle))]
        if window == needle:
            hits.append(idx)
    if len(hits) == 1:
        start = hits[0]
        indent = hay[start][: len(hay[start]) - len(hay[start].lstrip())]
        replacement = new.splitlines(keepends=True)
        if not replacement and new == "":
            replacement = []
        elif replacement:
            if not replacement[0].startswith((" ", "\t")) and indent:
                replacement[0] = indent + replacement[0].lstrip()
            if new and not new.endswith("\n"):
                replacement[-1] = replacement[-1] + "\n" if not replacement[-1].endswith("\n") else replacement[-1]
        updated = hay[:start] + replacement + hay[start + len(needle) :]
        return "".join(updated), ""
    if not hits:
        return None, "old_string not found"
    return None, "old_string is not unique"


def _apply_hunks(text: str, hunks: object) -> str | None:
    if not isinstance(hunks, list):
        return None
    lines = text.splitlines(keepends=True)
    # Apply from the bottom so earlier line numbers stay valid.
    ordered = sorted(hunks, key=lambda h: int(h.get("start_line", 0)), reverse=True)
    for hunk in ordered:
        try:
            start = int(hunk["start_line"])
            end = int(hunk["end_line"])
            replacement = str(hunk["replacement"])
        except (KeyError, TypeError, ValueError):
            return None
        if start < 1 or end < start - 1 or end > len(lines):
            return None
        repl_lines = replacement.splitlines(keepends=True)
        if replacement and not replacement.endswith("\n"):
            if repl_lines:
                repl_lines[-1] = repl_lines[-1] + "\n" if not repl_lines[-1].endswith("\n") else repl_lines[-1]
        lines[start - 1 : end] = repl_lines
    return "".join(lines)


def create_directory(sandbox: WorkspaceSandbox, call: ToolCall) -> ToolResult:
    rel = str(call.arguments.get("path") or "")
    try:
        path = sandbox.resolve(rel)
    except SandboxError as exc:
        return _fail(call, str(exc))
    path.mkdir(parents=True, exist_ok=True)
    return ToolResult(id=call.id, success=True, output=json.dumps({"path": sandbox.relative(path)}))


def move_file(sandbox: WorkspaceSandbox, call: ToolCall) -> ToolResult:
    try:
        src = sandbox.resolve(str(call.arguments.get("src") or ""), must_exist=True)
        dest = sandbox.resolve(str(call.arguments.get("dest") or ""))
    except SandboxError as exc:
        return _fail(call, str(exc))
    dest.parent.mkdir(parents=True, exist_ok=True)
    src.rename(dest)
    return ToolResult(id=call.id, success=True, output=json.dumps({"src": sandbox.relative(src), "dest": sandbox.relative(dest)}))


def delete_file(sandbox: WorkspaceSandbox, call: ToolCall) -> ToolResult:
    try:
        path = sandbox.resolve(str(call.arguments.get("path") or ""), must_exist=True)
    except SandboxError as exc:
        return _fail(call, str(exc))
    if path.is_dir():
        return _fail(call, "refusing to delete a directory")
    path.unlink()
    return ToolResult(id=call.id, success=True, output=json.dumps({"deleted": sandbox.relative(path)}))
