"""Reliable apply_patch / unified-diff application inside the workspace."""

from __future__ import annotations

import json
from dataclasses import dataclass

from shadow_agent.models.types import ToolCall, ToolResult
from shadow_agent.tools.sandbox import SandboxError, WorkspaceSandbox


def apply_patch(sandbox: WorkspaceSandbox, call: ToolCall) -> ToolResult:
    patch = str(call.arguments.get("patch") or call.arguments.get("diff") or "")
    explicit = str(call.arguments.get("path") or "")
    if not patch.strip():
        return ToolResult(id=call.id, success=False, error="patch is required")
    try:
        hunks = parse_patch(patch, default_path=explicit)
    except ValueError as exc:
        return ToolResult(id=call.id, success=False, error=str(exc))
    if not hunks:
        return ToolResult(id=call.id, success=False, error="could not parse any file hunks from the patch")
    applied: list[dict[str, str]] = []
    for path, file_hunks in _group(hunks).items():
        try:
            target = sandbox.resolve(path, must_exist=True)
        except SandboxError as exc:
            return ToolResult(id=call.id, success=False, error=str(exc))
        if not target.is_file():
            return ToolResult(id=call.id, success=False, error=f"not a file: {path}")
        text = target.read_text(encoding="utf-8")
        updated = text
        for hunk in file_hunks:
            nxt, err = apply_hunk(updated, hunk)
            if nxt is None:
                return ToolResult(id=call.id, success=False, error=f"{path}: {err}")
            updated = nxt
        if updated != text:
            target.write_text(updated, encoding="utf-8")
        applied.append({"path": sandbox.relative(target), "hunks": str(len(file_hunks))})
    return ToolResult(id=call.id, success=True, output=json.dumps({"applied": applied, "patched": True}))


@dataclass
class Hunk:
    path: str
    old_lines: list[str]
    new_lines: list[str]


def parse_patch(patch: str, default_path: str = "") -> list[Hunk]:
    text = patch.replace("\r\n", "\n")
    if "*** Begin Patch" in text or "*** Update File:" in text:
        return _parse_begin_patch(text, default_path)
    if "--- " in text and "+++ " in text:
        return _parse_unified(text, default_path)
    if default_path and ("\n-" in text or text.startswith("-")):
        return _parse_naked_hunk(text, default_path)
    raise ValueError("unrecognized patch format; use a unified diff or *** Begin Patch")


def apply_hunk(text: str, hunk: Hunk) -> tuple[str | None, str]:
    old = "".join(hunk.old_lines)
    new = "".join(hunk.new_lines)
    if not old.strip():
        if text.endswith("\n") or not text:
            return text + new, ""
        return text + "\n" + new, ""
    if old in text:
        if text.count(old) > 1:
            return None, "hunk context is not unique; add more nearby lines"
        return text.replace(old, new, 1), ""
    fuzzy = _fuzzy_replace(text, hunk.old_lines, hunk.new_lines)
    if fuzzy is not None:
        return fuzzy, ""
    preview = _preview(text, hunk.old_lines)
    return None, f"hunk did not match.{preview}"


def _parse_unified(text: str, default_path: str) -> list[Hunk]:
    hunks: list[Hunk] = []
    current_path = default_path
    old: list[str] = []
    new: list[str] = []
    in_hunk = False

    def flush() -> None:
        nonlocal old, new, in_hunk
        if in_hunk and current_path:
            hunks.append(Hunk(current_path, old, new))
        old, new, in_hunk = [], [], False

    for line in text.splitlines(keepends=True):
        if line.startswith("--- "):
            flush()
            current_path = _strip_ab(line[4:].strip()) or current_path
            continue
        if line.startswith("+++ "):
            current_path = _strip_ab(line[4:].strip()) or current_path
            continue
        if line.startswith("@@"):
            flush()
            in_hunk = True
            continue
        if not in_hunk:
            continue
        if line.startswith("+"):
            new.append(line[1:] if line.endswith("\n") else line[1:] + "\n")
        elif line.startswith("-"):
            old.append(line[1:] if line.endswith("\n") else line[1:] + "\n")
        elif line.startswith("\\"):
            continue
        else:
            body = line[1:] if line.startswith(" ") else line
            if not body.endswith("\n"):
                body += "\n"
            old.append(body)
            new.append(body)
    flush()
    return hunks


def _parse_begin_patch(text: str, default_path: str) -> list[Hunk]:
    hunks: list[Hunk] = []
    current_path = default_path
    body: list[str] = []

    def flush() -> None:
        nonlocal body
        if current_path and body:
            hunks.extend(_parse_unified("".join(body), current_path) or _parse_naked_hunk("".join(body), current_path))
        body = []

    for line in text.splitlines(keepends=True):
        if line.startswith("*** Update File:") or line.startswith("*** Add File:"):
            flush()
            current_path = line.split(":", 1)[1].strip() or current_path
            continue
        if line.startswith("*** End Patch") or line.startswith("*** Begin Patch"):
            flush()
            continue
        body.append(line)
    flush()
    return hunks


def _parse_naked_hunk(text: str, path: str) -> list[Hunk]:
    old: list[str] = []
    new: list[str] = []
    for line in text.splitlines(keepends=True):
        if line.startswith("+"):
            new.append(line[1:] if line.endswith("\n") else line[1:] + "\n")
        elif line.startswith("-"):
            old.append(line[1:] if line.endswith("\n") else line[1:] + "\n")
        elif line.startswith("@@") or line.startswith("---") or line.startswith("+++"):
            continue
        else:
            body = line[1:] if line.startswith(" ") else line
            if not body.endswith("\n"):
                body += "\n"
            old.append(body)
            new.append(body)
    if not old and not new:
        return []
    return [Hunk(path, old, new)]


def _strip_ab(value: str) -> str:
    value = value.split("\t", 1)[0].strip()
    if value.startswith("a/") or value.startswith("b/"):
        return value[2:]
    if value == "/dev/null":
        return ""
    return value


def _group(hunks: list[Hunk]) -> dict[str, list[Hunk]]:
    grouped: dict[str, list[Hunk]] = {}
    for hunk in hunks:
        grouped.setdefault(hunk.path, []).append(hunk)
    return grouped


def _fuzzy_replace(text: str, old_lines: list[str], new_lines: list[str]) -> str | None:
    hay = text.splitlines(keepends=True)
    needle = [line.rstrip("\n").strip() for line in old_lines]
    if not needle:
        return None
    matches: list[int] = []
    for idx in range(0, len(hay) - len(needle) + 1):
        window = [hay[idx + offset].rstrip("\n").strip() for offset in range(len(needle))]
        if window == needle:
            matches.append(idx)
    if len(matches) != 1:
        return None
    start = matches[0]
    rebuilt = hay[:start] + [line if line.endswith("\n") else line + "\n" for line in _strip_keep(new_lines)] + hay[start + len(needle) :]
    return "".join(rebuilt)


def _strip_keep(lines: list[str]) -> list[str]:
    return [line if line.endswith("\n") else line + "\n" for line in lines]


def _preview(text: str, old_lines: list[str]) -> str:
    first = next((line.strip() for line in old_lines if line.strip()), "")
    if not first:
        return ""
    for idx, line in enumerate(text.splitlines(), start=1):
        if first[:40] and first[:40] in line:
            return f" Nearby line {idx}: {line.strip()[:120]}"
    return ""

