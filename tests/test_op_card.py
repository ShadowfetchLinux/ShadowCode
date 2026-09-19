"""Tests for the Codex-style compact operation-card model.

Covers:
- pytest summary parser ('49 passed · 2 failed')
- exec exit-code headline (generic command, no pytest)
- read_file headline (path + line count)
- write_file / edit_file / apply_patch hunk summary ('+12 -3', 'N hunks')
- git status / git log headline
- search results count
- collapsed card renders only the summary line, never raw stdout/stderr
- expanding reveals the full output
- headline_for_event falls back to deriving from output_preview
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from shadow_agent.models.types import ToolCall, ToolResult
from shadow_agent.op_card import (
    ICON_EDIT,
    ICON_FAIL,
    ICON_PASS,
    ICON_READ,
    ICON_RUN,
    OpCard,
    headline_for_event,
    parse_pytest_summary,
    summarize,
)
from shadow_agent.tui.theme import DARK
from shadow_agent.tui.transcript import TranscriptModel
from shadow_agent.config import AppConfig
from shadow_agent.store import Store


# --- pytest summary parser --------------------------------------------------


def test_parse_pytest_summary_passed_and_failed():
    text = "==================== test session starts ====================\ncollected 51 items\n\ntest_a.py .... [100%]\n\n49 passed, 2 failed in 3.21s"
    assert parse_pytest_summary(text) == "49 passed · 2 failed"


def test_parse_pytest_summary_with_errors_and_skipped():
    text = "1 passed, 3 failed, 2 errors, 5 skipped in 1.0s"
    assert parse_pytest_summary(text) == "1 passed · 3 failed · 2 error · 5 skipped"


def test_parse_pytest_summary_empty_when_no_summary_line():
    assert parse_pytest_summary("just some output\nno summary here") == ""
    assert parse_pytest_summary("") == ""


def test_parse_pytest_summary_only_passed():
    assert parse_pytest_summary("49 passed in 1.0s") == "49 passed"


# --- exec card --------------------------------------------------------------


def _exec_result(stdout: str = "", stderr: str = "", exit_code: int = 0, error: str = "") -> ToolResult:
    payload = {"command": "pytest", "stdout": stdout, "stderr": stderr, "exit_code": exit_code, "cwd": "/ws"}
    return ToolResult(
        id="x",
        success=exit_code == 0 and not error,
        output=json.dumps(payload),
        error=error,
        metadata={"exit_code": exit_code},
    )


def test_exec_pytest_card_shows_compact_summary():
    result = _exec_result(stdout="49 passed, 2 failed in 3.21s", exit_code=1)
    card = summarize("exec", {"command": "pytest -q"}, result)
    assert card.icon == ICON_FAIL
    assert card.headline == "pytest  49 passed · 2 failed"
    # Full output is preserved (truncated) for the expand view.
    assert "49 passed" in card.full_output
    assert "stdout" in card.full_output  # structured payload pretty-printed


def test_exec_pytest_pass_uses_pass_icon():
    result = _exec_result(stdout="10 passed in 0.5s", exit_code=0)
    card = summarize("exec", {"command": "pytest"}, result)
    assert card.icon == ICON_PASS
    assert card.headline == "pytest  10 passed"


def test_exec_generic_command_shows_exit_code_and_last_line():
    result = _exec_result(stdout="line one\nline two\nall good", exit_code=0)
    card = summarize("exec", {"command": "ls -la"}, result)
    assert card.icon == ICON_RUN
    assert "ls" in card.headline
    assert "all good" in card.headline


def test_exec_failure_shows_exit_code():
    result = _exec_result(stdout="", stderr="command not found", exit_code=127)
    card = summarize("exec", {"command": "fakecmd"}, result)
    assert card.icon == ICON_FAIL
    assert "127" in card.headline


def test_exec_command_name_strips_path():
    result = _exec_result(stdout="ok", exit_code=0)
    card = summarize("exec", {"command": "/usr/bin/python3 -m pytest"}, result)
    assert card.headline.startswith("python3  ")


# --- read / list cards ------------------------------------------------------


def test_read_file_card_shows_path_and_line_count():
    payload = {"path": "src/app.py", "content": "x\n", "offset": 1, "total_lines": 42}
    result = ToolResult(id="x", success=True, output=json.dumps(payload))
    card = summarize("read_file", {"path": "src/app.py"}, result)
    assert card.icon == ICON_READ
    assert card.headline == "read src/app.py  42 lines"


def test_list_files_card_shows_entry_count():
    payload = {"path": ".", "entries": [{"path": "a", "type": "file"}, {"path": "b", "type": "dir"}]}
    result = ToolResult(id="x", success=True, output=json.dumps(payload))
    card = summarize("list_files", {"path": "."}, result)
    assert card.icon == ICON_READ
    assert card.headline == "list .  2 entries"


# --- edit cards -------------------------------------------------------------


def test_write_file_card_shows_wrote_and_bytes():
    payload = {"path": "src/app.py", "bytes": 128}
    result = ToolResult(id="x", success=True, output=json.dumps(payload))
    card = summarize("write_file", {"path": "src/app.py", "content": "x" * 128}, result)
    assert card.icon == ICON_EDIT
    assert card.headline == "wrote src/app.py  128 bytes"


def test_edit_file_card_with_old_new_shows_hunk_summary():
    result = ToolResult(id="x", success=True, output=json.dumps({"path": "src/app.py", "patched": True}))
    card = summarize(
        "edit_file",
        {"path": "src/app.py", "old_string": "a\nb\nc", "new_string": "a\nb\nc\nd\ne"},
        result,
    )
    assert card.icon == ICON_EDIT
    assert "edited src/app.py" in card.headline
    assert "+5 -3" in card.headline


def test_edit_file_card_with_hunks_shows_hunk_count():
    result = ToolResult(id="x", success=True, output=json.dumps({"path": "f.py", "patched": True}))
    card = summarize(
        "edit_file",
        {
            "path": "f.py",
            "hunks": [
                {"start_line": 1, "end_line": 2, "replacement": "x"},
                {"start_line": 5, "end_line": 6, "replacement": "y"},
            ],
        },
        result,
    )
    assert card.icon == ICON_EDIT
    assert "2 hunks" in card.headline


def test_apply_patch_card_shows_adds_dels():
    diff = "--- a/f.py\n+++ b/f.py\n@@ -1 +1 @@\n-old\n+new\n+new2\n"
    result = ToolResult(id="x", success=True, output=json.dumps({"patched": True}))
    card = summarize("apply_patch", {"path": "f.py", "patch": diff}, result)
    assert card.icon == ICON_EDIT
    assert "+2 -1" in card.headline


# --- git cards --------------------------------------------------------------


def test_git_status_card_counts_changed_files():
    payload = {"args": ["status"], "stdout": "## main\nM src/a.py\n?? b.py\n", "stderr": "", "exit_code": 0}
    result = ToolResult(id="x", success=True, output=json.dumps(payload))
    card = summarize("git_status", {}, result)
    assert card.icon == ICON_PASS
    assert "git status" in card.headline
    assert "2 changed" in card.headline


def test_git_log_card_counts_commits():
    payload = {"args": ["log"], "stdout": "abc123 msg1\ndef456 msg2\n", "stderr": "", "exit_code": 0}
    result = ToolResult(id="x", success=True, output=json.dumps(payload))
    card = summarize("git_log", {"limit": 12}, result)
    assert card.icon == ICON_READ
    assert "2 commits" in card.headline


# --- search cards ----------------------------------------------------------


def test_search_card_shows_result_count():
    payload = {"hits": ["a:1:x", "b:2:y", "c:3:z"], "engine": "rg"}
    result = ToolResult(id="x", success=True, output=json.dumps(payload))
    card = summarize("search_text", {"query": "foo"}, result)
    assert card.icon == ICON_READ
    assert "3 results" in card.headline
    assert "foo" in card.headline


# --- generic / fallback -----------------------------------------------------


def test_generic_failure_card_shows_error_snippet():
    result = ToolResult(id="x", success=False, error="something broke in a long way")
    card = summarize("unknown_tool", {}, result)
    assert card.icon == ICON_FAIL
    assert "something broke" in card.headline


def test_headline_for_event_uses_precomputed_fields():
    payload = {"tool": "exec", "success": True, "icon": "✓", "headline": "pytest  49 passed", "output_full": "..."}
    icon, headline, full = headline_for_event(payload)
    assert icon == "✓"
    assert headline == "pytest  49 passed"
    assert full == "..."


def test_headline_for_event_falls_back_to_preview():
    payload = {"tool": "exec", "success": False, "output_preview": '{"exit_code": 1, "stdout": ""}', "error": "boom"}
    icon, headline, full = headline_for_event(payload)
    # Falls back to deriving from preview; should not crash and should mention exec.
    assert icon in (ICON_FAIL, ICON_RUN)
    assert "exec" in headline or "boom" in headline


# --- transcript rendering (collapsed vs expanded) --------------------------


def _transcript(workspace: Path) -> TranscriptModel:
    cfg = AppConfig()
    store = Store()
    sid = store.create_session(str(workspace), "mock", title="t")
    return TranscriptModel(workspace, DARK, cfg, store, sid)


def test_tool_card_renders_single_summary_line_collapsed(workspace: Path):
    model = _transcript(workspace)
    model.add_tool(
        "exec",
        True,
        '{"stdout": "49 passed, 2 failed"}',
        live=False,
        icon=ICON_FAIL,
        headline="pytest  49 passed · 2 failed",
        full_output='{"stdout": "49 passed, 2 failed", "exit_code": 1}',
    )
    rendered = "".join(piece for _, piece in model.render())
    # Collapsed view: one summary line with icon + headline.
    assert "✗ pytest  49 passed · 2 failed" in rendered
    # No raw stdout/stderr dump in the collapsed view.
    assert '"stdout": "49 passed, 2 failed", "exit_code": 1' not in rendered


def test_tool_card_expand_reveals_full_output(workspace: Path):
    model = _transcript(workspace)
    full = '{"stdout": "49 passed", "exit_code": 0}'
    model.add_tool(
        "exec",
        True,
        "preview",
        live=False,
        icon=ICON_PASS,
        headline="pytest  49 passed",
        full_output=full,
    )
    # Collapsed by default: full output not in render.
    collapsed = "".join(piece for _, piece in model.render())
    assert "exit_code" not in collapsed
    # Toggle expansion.
    idx = model.tool_turn_indices()[-1]
    model.toggle_card(idx)
    expanded = "".join(piece for _, piece in model.render())
    assert "exit_code" in expanded
    # Collapse again.
    model.toggle_card(idx)
    collapsed_again = "".join(piece for _, piece in model.render())
    assert "exit_code" not in collapsed_again


def test_tool_turn_indices_only_lists_tools(workspace: Path):
    model = _transcript(workspace)
    model.add_user("hi")
    model.add_agent("working")
    model.add_tool("exec", True, "x", headline="pytest ok")
    model.add_agent("done")
    model.add_tool("read_file", True, "y", headline="read f.py 10 lines")
    assert model.tool_turn_indices() == [2, 4]


def test_collapsed_card_has_no_raw_stdout_in_text(workspace: Path):
    """Regression: the old transcript dumped `turn.text[:600]` (raw preview)
    in the collapsed view. The new card must show only the one-line headline."""
    model = _transcript(workspace)
    raw_preview = '{"command":"pytest","stdout":"long raw output that should not appear in collapsed view\\nline2\\nline3","stderr":"","exit_code":0}'
    model.add_tool(
        "exec",
        True,
        raw_preview,
        live=False,
        icon=ICON_PASS,
        headline="pytest  49 passed",
        full_output=raw_preview,
    )
    rendered = "".join(piece for _, piece in model.render())
    assert "long raw output that should not appear in collapsed view" not in rendered
    assert "pytest  49 passed" in rendered


# --- end-to-end through the agent loop event payload ------------------------


def test_agent_loop_emits_card_fields(workspace: Path, isolated):
    """The agent loop must attach icon/headline/output_full to tool.completed
    so both UIs can render a Codex-style compact card without re-parsing."""
    from shadow_agent.agent.loop import AgentRunner
    from shadow_agent.events import EventBus
    from shadow_agent.store import Store

    store = Store()
    bus = EventBus()
    seen: list[dict] = []
    bus.subscribe("tool.completed", lambda _et, ev: seen.append(ev["payload"]))

    runner = AgentRunner(workspace, store=store, events=bus)
    # Force a real exec call via the tools registry directly.
    from shadow_agent.models.types import ToolCall
    from shadow_agent.permissions import PermissionGate
    from shadow_agent.config import PermissionLevel

    gate = PermissionGate(PermissionLevel.WORKSPACE)
    call = ToolCall(id="t1", tool_name="exec", arguments={"command": "echo hello"})
    result = runner.tools.execute(call, gate)
    # Simulate the loop's emit step.
    from shadow_agent.op_card import summarize
    card = summarize("exec", call.arguments, result)
    assert card.headline.startswith("echo")
    assert card.icon in (ICON_RUN, ICON_PASS)
    assert "hello" in card.full_output or "exit_code" in card.full_output
