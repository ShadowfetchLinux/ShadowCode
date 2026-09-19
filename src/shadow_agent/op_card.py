"""Compact operation-card model for tool/operation output.

Renders each tool/operation as a single summary line with an icon:

    ● run     ▸ read     ✎ edit     ✓ pass     ✗ fail     → next step

The line summarizes the operation (command name or file path) and the
headline result (e.g. "49 passed · 2 failed", "edited src/app.py",
"12 results"). Full output (stdout/stderr/diff/exit code) lives in the
expandable section, never in the collapsed view.

This is the Codex-style concise transcript: agent reasoning stays as plain
text blocks; only tool/operation cards collapse.
"""

from __future__ import annotations

import json
import re
from dataclasses import dataclass
from typing import Any

# --- icons ------------------------------------------------------------------

ICON_RUN = "●"      # exec / shell running or succeeded
ICON_READ = "▸"     # read-only tools (read_file, list_files, search, git inspect)
ICON_EDIT = "✎"     # write/edit tools
ICON_PASS = "✓"     # explicit pass result
ICON_FAIL = "✗"     # explicit failure
ICON_NEXT = "→"     # next step / continuation hint

# --- pytest / generic test-summary parser -----------------------------------

_PASS_RE = re.compile(r"(\d+)\s+passed", re.I)
_FAIL_RE = re.compile(r"(\d+)\s+failed", re.I)
_ERROR_RE = re.compile(r"(\d+)\s+error", re.I)
_SKIP_RE = re.compile(r"(\d+)\s+(?:skipped|deselected)", re.I)


def parse_pytest_summary(text: str) -> str:
    """Return a compact 'N passed · M failed · K error' headline, or ''.

    Works on pytest, unittest, and most nose/pytest-compatible runners
    that print a final summary line. Returns '' when no pass/fail token is
    found so callers can fall back to a generic exit-code headline.
    """
    if not text:
        return ""
    parts: list[str] = []
    m = _PASS_RE.search(text)
    if m:
        parts.append(f"{m.group(1)} passed")
    m = _FAIL_RE.search(text)
    if m:
        parts.append(f"{m.group(1)} failed")
    m = _ERROR_RE.search(text)
    if m:
        parts.append(f"{m.group(1)} error")
    m = _SKIP_RE.search(text)
    if m:
        parts.append(f"{m.group(1)} skipped")
    return " · ".join(parts)


# --- card model ------------------------------------------------------------


@dataclass
class OpCard:
    """A single tool/operation card in the transcript."""

    tool: str
    icon: str
    headline: str          # one-line summary (command name + headline result)
    full_output: str = ""  # expandable: stdout/stderr/diff/exit code, truncated
    ok: bool | None = None
    collapsed: bool = True

    def summary_line(self) -> str:
        return f"{self.icon} {self.headline}".strip()


def summarize(tool: str, arguments: dict[str, Any], result: Any) -> OpCard:
    """Build an OpCard from a tool name, its arguments, and a ToolResult.

    `result` is a ToolResult-like object with `.output` (JSON string),
    `.error`, and `.success`. The headline is derived automatically per
    tool family; the full output is preserved (truncated) for the expand view.
    """
    output = getattr(result, "output", "") or ""
    error = getattr(result, "error", "") or ""
    ok = bool(getattr(result, "success", False))
    full = _full_output(output, error)
    icon, headline = _derive(tool, arguments, output, error, ok)
    return OpCard(tool=tool, icon=icon, headline=headline, full_output=full, ok=ok)


# --- per-tool derivations ---------------------------------------------------


def _derive(
    tool: str,
    arguments: dict[str, Any],
    output: str,
    error: str,
    ok: bool,
) -> tuple[str, str]:
    if tool == "exec":
        return _exec_card(arguments, output, error, ok)
    if tool == "list_files":
        return _list_card(arguments, output)
    if tool == "read_file":
        return _read_card(arguments, output)
    if tool in {"write_file", "edit_file", "apply_patch"}:
        return _edit_card(tool, arguments, output)
    if tool.startswith("git_"):
        return _git_card(tool, arguments, output, ok)
    if tool.startswith("search_"):
        return _search_card(tool, arguments, output)
    if tool == "kill":
        return _kill_card(arguments, output, ok)
    return _generic_card(tool, error, ok)


def _exec_card(
    arguments: dict[str, Any],
    output: str,
    error: str,
    ok: bool,
) -> tuple[str, str]:
    cmd = str(arguments.get("command") or "")
    name = _cmd_name(cmd)
    payload = _try_json(output) or {}
    stdout = str(payload.get("stdout") or "")
    stderr = str(payload.get("stderr") or "")
    exit_code = payload.get("exit_code")
    text = stdout or stderr or error or ""
    pytest = parse_pytest_summary(text)
    if pytest:
        # pytest summary is the headline; icon reflects pass/fail.
        return (ICON_PASS if ok else ICON_FAIL), f"{name}  {pytest}"
    last = _last_line(text)
    if exit_code is not None and exit_code != 0:
        head = f"{name}  exit {exit_code}"
        if last:
            head += f"  {last[:60]}"
    elif last:
        head = f"{name}  {last[:80]}"
    elif ok:
        head = f"{name}  ok"
    else:
        head = f"{name}  {(error or 'failed')[:80]}"
    return (ICON_RUN if ok else ICON_FAIL), head


def _cmd_name(cmd: str) -> str:
    if not cmd:
        return "exec"
    first = cmd.strip().split()[0] if cmd.strip() else ""
    base = first.split("/")[-1] or first
    return base or "exec"


def _list_card(arguments: dict[str, Any], output: str) -> tuple[str, str]:
    path = str(arguments.get("path") or ".")
    payload = _try_json(output) or {}
    entries = payload.get("entries") or []
    n = len(entries) if isinstance(entries, list) else 0
    return ICON_READ, f"list {path}  {n} entries"


def _read_card(arguments: dict[str, Any], output: str) -> tuple[str, str]:
    path = str(arguments.get("path") or "")
    payload = _try_json(output) or {}
    total = payload.get("total_lines")
    if total is not None:
        return ICON_READ, f"read {path}  {total} lines"
    return ICON_READ, f"read {path}"


def _edit_card(
    tool: str,
    arguments: dict[str, Any],
    output: str,
) -> tuple[str, str]:
    path = str(arguments.get("path") or "")
    if tool == "write_file":
        payload = _try_json(output) or {}
        size = payload.get("bytes")
        if size is not None:
            return ICON_EDIT, f"wrote {path}  {size} bytes"
        return ICON_EDIT, f"wrote {path}"
    if tool == "edit_file":
        hunk = _hunk_summary(arguments)
        head = f"edited {path}"
        if hunk:
            head += f"  {hunk}"
        return ICON_EDIT, head
    if tool == "apply_patch":
        hunk = _patch_summary(arguments)
        head = f"patched {path}" if path else "patched"
        if hunk:
            head += f"  {hunk}"
        return ICON_EDIT, head
    return ICON_EDIT, f"edited {path}"


def _hunk_summary(arguments: dict[str, Any]) -> str:
    hunks = arguments.get("hunks")
    if isinstance(hunks, list) and hunks:
        plural = "s" if len(hunks) != 1 else ""
        return f"{len(hunks)} hunk{plural}"
    old = arguments.get("old_string")
    new = arguments.get("new_string")
    if isinstance(old, str) and isinstance(new, str):
        adds = len(new.splitlines())
        dels = len(old.splitlines())
        return f"+{adds} -{dels}"
    return ""


def _patch_summary(arguments: dict[str, Any]) -> str:
    diff = str(arguments.get("patch") or arguments.get("diff") or "")
    if not diff:
        return ""
    adds = sum(1 for line in diff.splitlines() if line.startswith("+") and not line.startswith("+++"))
    dels = sum(1 for line in diff.splitlines() if line.startswith("-") and not line.startswith("---"))
    return f"+{adds} -{dels}"


def _git_card(
    tool: str,
    arguments: dict[str, Any],
    output: str,
    ok: bool,
) -> tuple[str, str]:
    payload = _try_json(output) or {}
    stdout = str(payload.get("stdout") or "")
    if tool == "git_status":
        files = [line for line in stdout.splitlines() if line.strip() and not line.startswith("##")]
        return (ICON_PASS if ok else ICON_FAIL), f"git status  {len(files)} changed"
    if tool == "git_log":
        count = len([line for line in stdout.splitlines() if line.strip()])
        return ICON_READ, f"git log  {count} commits"
    if tool == "git_diff":
        return ICON_READ, "git diff"
    if tool == "git_commit":
        return (ICON_PASS if ok else ICON_FAIL), "git commit"
    if tool == "git_add":
        return (ICON_PASS if ok else ICON_FAIL), "git add"
    if tool == "git_branch":
        return (ICON_PASS if ok else ICON_FAIL), "git branch"
    if tool == "git_checkout":
        ref = str(arguments.get("ref") or "")
        return (ICON_PASS if ok else ICON_FAIL), f"git checkout {ref}".strip()
    return (ICON_PASS if ok else ICON_FAIL), tool


def _search_card(
    tool: str,
    arguments: dict[str, Any],
    output: str,
) -> tuple[str, str]:
    payload = _try_json(output) or {}
    hits = payload.get("hits")
    n = len(hits) if isinstance(hits, list) else 0
    query = str(arguments.get("query") or "")
    label = tool.replace("search_", "search ")
    if query:
        return ICON_READ, f"{label} '{query}'  {n} results"
    return ICON_READ, f"{label}  {n} results"


def _kill_card(
    arguments: dict[str, Any],
    output: str,
    ok: bool,
) -> tuple[str, str]:
    pid = arguments.get("pid")
    return (ICON_PASS if ok else ICON_FAIL), f"kill {pid}" if pid else ("kill", "kill")[1]


def _generic_card(tool: str, error: str, ok: bool) -> tuple[str, str]:
    if ok:
        return ICON_PASS, tool
    return ICON_FAIL, f"{tool}  {(error or 'failed')[:80]}"


# --- helpers ----------------------------------------------------------------


def _try_json(s: str) -> dict[str, Any] | None:
    if not s:
        return None
    try:
        data = json.loads(s)
    except (json.JSONDecodeError, ValueError):
        return None
    return data if isinstance(data, dict) else None


def _last_line(text: str) -> str:
    for line in reversed(text.splitlines()):
        line = line.strip()
        if line:
            return line
    return ""


def _full_output(output: str, error: str) -> str:
    """Pretty-print the structured payload for the expand view, truncated."""
    payload = _try_json(output)
    if isinstance(payload, dict):
        text = json.dumps(payload, indent=2)
    else:
        text = output or error or ""
    if len(text) > 8000:
        return text[:8000] + "\n… (truncated; show full for the rest)"
    return text


def headline_for_event(payload: dict[str, Any]) -> tuple[str, str, str]:
    """Best-effort (icon, headline, full_output) for a tool.completed event.

    Used by UIs that ingest events directly (TUI transcript, React app) when
    the agent loop has already attached a precomputed card. Falls back to
    deriving from `output_preview` so older event streams still render.
    """
    if payload.get("icon") and payload.get("headline"):
        return str(payload["icon"]), str(payload["headline"]), str(payload.get("output_full") or "")
    tool = str(payload.get("tool") or "tool")
    preview = str(payload.get("output_preview") or payload.get("error") or "")
    ok = bool(payload.get("success"))
    # Reconstruct a minimal arguments dict from the event when available.
    arguments = payload.get("arguments") or {}
    if not isinstance(arguments, dict):
        arguments = {}
    card = summarize(tool, arguments, _MiniResult(output=preview, error=str(payload.get("error") or ""), success=ok))
    return card.icon, card.headline, card.full_output


@dataclass
class _MiniResult:
    output: str = ""
    error: str = ""
    success: bool = False
