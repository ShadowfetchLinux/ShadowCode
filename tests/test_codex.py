"""Tests for the 0.4.0 flagship upgrade: Codex-style TUI, slash commands,
context compaction, session branching, doctor auto-fix, and a fully general
model picker (any provider, any model, free-text).
"""

from __future__ import annotations

import time
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from shadow_agent.api.server import create_app
from shadow_agent.commands import CommandRegistry, SlashCommand, load_project_commands
from shadow_agent.config import AppConfig, load_config, save_config
from shadow_agent.health import doctor_fix, doctor_report
from shadow_agent.store import Store
from shadow_agent.tui.theme import DARK, LIGHT, load_theme, system_theme
from shadow_agent.tui.transcript import TranscriptModel


# --- slash commands --------------------------------------------------------


def test_command_registry_parses_and_resolves(workspace: Path):
    reg = CommandRegistry(workspace)
    cmd, name, args = reg.parse("/help")
    assert cmd is not None and name == "help"
    cmd, name, args = reg.parse("just a task")
    assert cmd is None and name == "" and args == "just a task"
    cmd, name, args = reg.parse("/unknown-thing arg")
    assert cmd is None and name == "unknown-thing"


def test_project_commands_loaded_from_shadow_dir(workspace: Path):
    cmd_dir = workspace / ".shadow" / "commands"
    cmd_dir.mkdir(parents=True)
    (cmd_dir / "release.md").write_text(
        "---\n"
        "description: Cut a release\n"
        "alias: rel\n"
        "---\n"
        "Bump the version, update the changelog, tag, and push.\n",
        encoding="utf-8",
    )
    (cmd_dir / "raw.yaml").write_text("Run the raw deploy step.", encoding="utf-8")
    reg = CommandRegistry(workspace)
    rel = reg.get("release")
    assert rel is not None and rel.alias == "rel" and "changelog" in rel.body
    raw = reg.get("raw")
    assert raw is not None and raw.body.startswith("Run the raw")
    # alias resolves to the same command
    assert reg.get("rel") is rel


def test_custom_command_renders_args(workspace: Path):
    cmd_dir = workspace / ".shadow" / "commands"
    cmd_dir.mkdir(parents=True)
    (cmd_dir / "fix.md").write_text("Fix the failing tests.", encoding="utf-8")
    reg = CommandRegistry(workspace)
    cmd = reg.get("fix")
    assert cmd is not None
    rendered = cmd.render("focus on test_filesystem.py")
    assert "Fix the failing tests." in rendered
    assert "focus on test_filesystem.py" in rendered


# --- context compaction ----------------------------------------------------


def test_transcript_compact_summarizes_old_turns(workspace: Path):
    cfg = AppConfig()
    store = Store()
    sid = store.create_session(str(workspace), "mock", title="t")
    model = TranscriptModel(workspace, DARK, cfg, store, sid)
    for i in range(10):
        model.add_user(f"task {i}")
        model.add_agent(f"agent reply {i}")
    model.compact()
    # Compaction keeps the last 4 turns and replaces the rest with one summary.
    assert len(model.turns) <= 5
    assert any("Compacted earlier turns" in t.text for t in model.turns)


def test_transcript_renders_diff_card_with_accept_reject(workspace: Path):
    cfg = AppConfig()
    store = Store()
    sid = store.create_session(str(workspace), "mock", title="t")
    model = TranscriptModel(workspace, DARK, cfg, store, sid)
    model.add_diff_card("app.py", "--- a/app.py\n+++ b/app.py\n@@ -1 +1 @@\n-old\n+new\n")
    rendered = model.render()
    text = "".join(piece for _, piece in rendered)
    assert "propose edit · app.py" in text
    assert "[y] accept  [n] reject" in text
    assert "+new" in text and "-old" in text


def test_transcript_status_line_is_trimmed_to_model_ctx_tokens_stage(workspace: Path):
    """0.18.0: the TUI status line is exactly model · ctx% · tokens · stage."""
    cfg = AppConfig()
    store = Store()
    sid = store.create_session(str(workspace), "mock", title="t")
    model = TranscriptModel(workspace, DARK, cfg, store, sid)
    model.set_status(model="ollama/gpt-oss:20b", context_pct=42, tokens=1234, step=7, busy=True)
    status = model.status_text()
    assert "ollama/gpt-oss:20b" in status
    assert "ctx 42%" in status
    assert "1234 tok" in status
    assert "[ IDLE ] ●" in status  # busy marker rides on the stage chip
    # Trimmed: no step counter, no keybinding hints, no "working" word.
    assert "step" not in status
    assert "Enter send" not in status
    assert "working" not in status
    assert status.count("·") == 3


# --- themes -----------------------------------------------------------------


def test_themes_load_dark_and_light():
    assert load_theme("dark").name == "dark"
    assert load_theme("light").name == "light"
    assert load_theme("bogus").name == "dark"
    assert DARK.accent != LIGHT.accent  # light has a different accent
    assert system_theme() in {"dark", "light"}


# --- session branching + cost + pins --------------------------------------


def test_session_branch_copies_events(workspace: Path):
    store = Store()
    parent = store.create_session(str(workspace), "mock", title="parent")
    store.add_event("agent.started", {"task": "hello"}, session_id=parent)
    store.add_event("tool.completed", {"tool": "exec", "success": True}, session_id=parent)
    child = store.branch_session(parent, "child")
    assert child != parent
    child_row = store.get_session(child)
    assert child_row["parent_id"] == parent
    events = store.list_events(session_id=child)
    types = [e["type"] for e in events]
    assert "agent.started" in types and "tool.completed" in types


def test_session_cost_aggregates_usage(workspace: Path):
    store = Store()
    sid = store.create_session(str(workspace), "mock", title="cost")
    tid = store.create_task(sid, "do thing")
    store.add_usage(sid, tid, {"total_tokens": 100, "prompt_tokens": 80, "completion_tokens": 20})
    store.finish_task(tid, "completed", "ok")
    cost = store.session_cost(sid)
    assert cost["usage"]["total_tokens"] == 100
    assert cost["tasks"][0]["usage"]["total_tokens"] == 100


def test_session_pins_round_trip(workspace: Path):
    store = Store()
    sid = store.create_session(str(workspace), "mock", title="pins")
    pid = store.add_pin(sid, "note", "remember this output")
    pins = store.list_pins(sid)
    assert len(pins) == 1 and pins[0]["body"] == "remember this output"
    store.delete_pin(pid)
    assert store.list_pins(sid) == []


# --- doctor auto-fix --------------------------------------------------------


def test_doctor_fix_repairs_secrets_permissions(isolated):
    from shadow_agent import paths

    paths.secrets_file().write_text("OPENAI_API_KEY=x\n", encoding="utf-8")
    paths.secrets_file().chmod(0o644)
    cfg = load_config()
    report = doctor_report(cfg, None)
    sec = next(c for c in report["checks"] if c["id"] == "secrets-perms")
    assert not sec["ok"]
    applied = doctor_fix(report, None)
    assert any("chmod 600" in a for a in applied)
    assert (paths.secrets_file().stat().st_mode & 0o777) == 0o600


def test_doctor_fix_regenerates_broken_config(isolated):
    from shadow_agent import paths

    paths.config_file().write_text(": not valid yaml: [", encoding="utf-8")
    # doctor_report catches the parse error and reports a failing config check.
    report = doctor_report(AppConfig(), None)
    cfg_check = next(c for c in report["checks"] if c["id"] == "config")
    assert not cfg_check["ok"]
    applied = doctor_fix(report, None)
    assert any("regenerated" in a for a in applied)
    # After fix, config parses again.
    assert load_config().model.default == "mock"


# --- fully general model picker --------------------------------------------


@pytest.fixture
def general_client(isolated, workspace, monkeypatch):
    monkeypatch.setattr("shadow_agent.api.server.detect_providers", lambda: [])
    store = Store()
    app = create_app(store=store, default_workspace=workspace, detect=False)
    return TestClient(app)


def test_select_free_text_model_with_provider(general_client):
    res = general_client.post(
        "/api/models/select",
        json={"id": "gpt-4.1", "provider": "openai_compatible", "endpoint": "https://api.openai.com/v1", "name": "gpt-4.1"},
    )
    assert res.status_code == 200
    cfg = res.json()["model"]
    assert cfg["default"] == "gpt-4.1"
    assert cfg["provider"] == "openai_compatible"
    assert cfg["name"] == "gpt-4.1"


def test_register_custom_model_then_select(general_client):
    res = general_client.post(
        "/api/models/register",
        json={"id": "my-local-model", "provider": "ollama", "name": "my-local-model", "endpoint": "http://127.0.0.1:11434/v1"},
    )
    assert res.status_code == 200
    assert res.json()["model"]["default"] == "my-local-model"


def test_select_unknown_without_provider_still_404(general_client):
    res = general_client.post("/api/models/select", json={"id": "no-such-model"})
    assert res.status_code == 404


def test_cli_models_use_sets_any_model(isolated, workspace, monkeypatch):
    from typer.testing import CliRunner

    from shadow_agent.cli import app

    runner = CliRunner()
    # Set a custom free-text model with an explicit provider.
    res = runner.invoke(app, ["models", "--use", "qwen3:32b", "--provider", "ollama", "--endpoint", "http://127.0.0.1:11434/v1"])
    assert res.exit_code == 0
    assert "qwen3:32b" in res.stdout
    cfg = load_config()
    assert cfg.model.default == "qwen3:32b"
    assert cfg.model.provider == "ollama"


# --- session branch / cost / pins endpoints ---------------------------------


def test_api_session_branch_cost_pins(general_client, workspace):
    # create a session via the API
    created = general_client.post("/api/sessions", json={"workspace": str(workspace), "title": "branch me"})
    sid = created.json()["id"]
    general_client.post("/api/jobs", json={"task": "Create a Python hello-world project", "workspace": str(workspace), "session_id": sid})
    # branch
    branched = general_client.post(f"/api/sessions/{sid}/branch", json={"workspace": str(workspace), "title": "fork"})
    assert branched.status_code == 200
    new_id = branched.json()["id"]
    assert new_id != sid
    # cost
    cost = general_client.get(f"/api/sessions/{new_id}/cost")
    assert cost.status_code == 200
    assert "usage" in cost.json()
    # pins
    pin = general_client.post(f"/api/sessions/{new_id}/pins", json={"name": "note", "content": "remember"})
    assert pin.status_code == 200
    pins = general_client.get(f"/api/sessions/{new_id}/pins").json()["pins"]
    assert len(pins) == 1 and pins[0]["body"] == "remember"
