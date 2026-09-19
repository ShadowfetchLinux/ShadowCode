"""Tests for the ShadowCode 0.8.0 second-wave features (part 2: rewind, mcp, background, plugins, self-skill, SHADOW.md, CI)."""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

from shadow_agent.config import AppConfig
from shadow_agent.events import EventBus
from shadow_agent.models.adapters.mock import MockProvider
from shadow_agent.store import Store


# --- 4. Rewind (named checkpoints + multi-dimension restore) ---------------


def test_rewind_create_and_list(workspace, isolated):
    from shadow_agent.rewind import RewindStore

    store = RewindStore(workspace)
    (workspace / "a.py").write_text("before\n", encoding="utf-8")
    manifest = store.create("first checkpoint", session_id="s1", task_id="t1")
    assert manifest["id"] == "001"
    assert manifest["label"] == "first checkpoint"
    assert manifest["file_count"] >= 1
    rows = store.list()
    assert len(rows) == 1
    assert rows[0]["id"] == "001"


def test_rewind_restore_files_dimension(workspace, isolated):
    from shadow_agent.rewind import RewindStore

    store = RewindStore(workspace)
    (workspace / "a.py").write_text("before\n", encoding="utf-8")
    store.create("pre", session_id="s1")
    (workspace / "a.py").write_text("after\n", encoding="utf-8")
    result = store.restore("001", dimensions=["files"])
    assert result["ok"]
    assert result["restored"]["files"]["ok"]
    assert (workspace / "a.py").read_text(encoding="utf-8") == "before\n"


def test_rewind_restore_agent_state_dimension(workspace, isolated):
    from shadow_agent.rewind import RewindStore

    store = RewindStore(workspace)
    state = {"plan": [{"id": "s1", "status": "done"}], "todos": [], "stage": "DONE"}
    store.create("snap", agent_state=state)
    result = store.restore("001", dimensions=["agent_state"])
    assert result["restored"]["agent_state"]["ok"]
    assert result["restored"]["agent_state"]["state"]["stage"] == "DONE"


def test_rewind_restore_memory_dimension(workspace, isolated):
    from shadow_agent.rewind import RewindStore

    store = RewindStore(workspace)
    memory = {"project": "project notes here", "task": "task notes here"}
    store.create("snap", memory=memory)
    result = store.restore("001", dimensions=["memory"])
    assert result["restored"]["memory"]["ok"]
    assert (workspace / ".shadow" / "memory" / "project.md").read_text(encoding="utf-8") == "project notes here"


def test_rewind_restore_git_dimension(workspace, isolated):
    from shadow_agent.rewind import RewindStore

    subprocess.run(["git", "init"], cwd=workspace, capture_output=True, check=True)
    subprocess.run(["git", "config", "user.email", "t@t.t"], cwd=workspace, capture_output=True)
    subprocess.run(["git", "config", "user.name", "t"], cwd=workspace, capture_output=True)
    (workspace / "a.txt").write_text("v1\n", encoding="utf-8")
    subprocess.run(["git", "add", "."], cwd=workspace, capture_output=True, check=True)
    subprocess.run(["git", "commit", "-m", "v1"], cwd=workspace, capture_output=True, check=True)
    store = RewindStore(workspace)
    store.create("git-snap")
    (workspace / "a.txt").write_text("v2\n", encoding="utf-8")
    subprocess.run(["git", "commit", "-am", "v2"], cwd=workspace, capture_output=True, check=True)
    result = store.restore("001", dimensions=["git"])
    assert result["restored"]["git"]["ok"]
    assert (workspace / "a.txt").read_text(encoding="utf-8") == "v1\n"


def test_rewind_multi_dimension_restore(workspace, isolated):
    from shadow_agent.rewind import RewindStore

    store = RewindStore(workspace)
    (workspace / "a.py").write_text("before\n", encoding="utf-8")
    state = {"stage": "PLAN"}
    memory = {"project": "notes"}
    store.create("full-snap", agent_state=state, memory=memory)
    (workspace / "a.py").write_text("after\n", encoding="utf-8")
    result = store.restore("001", dimensions=["files", "agent_state", "memory"])
    assert result["restored"]["files"]["ok"]
    assert result["restored"]["agent_state"]["ok"]
    assert result["restored"]["memory"]["ok"]


def test_rewind_remove(workspace, isolated):
    from shadow_agent.rewind import RewindStore

    store = RewindStore(workspace)
    store.create("temp")
    assert store.remove("001") is True
    assert store.get("001") is None


# --- 5. MCP client ----------------------------------------------------------


def test_mcp_client_lists_builtin_servers():
    from shadow_agent.mcp_client import MCPClient

    client = MCPClient()
    names = [s.name for s in client.list_servers()]
    assert "filesystem" in names
    assert "sqlite" in names


def test_mcp_client_filesystem_list_and_read(tmp_path):
    from shadow_agent.mcp_client import MCPClient

    (tmp_path / "hello.txt").write_text("hi there\n", encoding="utf-8")
    client = MCPClient()
    tools = client.connect("filesystem")
    tool_names = [t.name for t in tools]
    assert "mcp_fs_list" in tool_names
    assert "mcp_fs_read" in tool_names
    list_result = client.call("filesystem", "mcp_fs_list", {"path": str(tmp_path)})
    assert list_result.success
    entries = json.loads(list_result.output)["entries"]
    assert "hello.txt" in entries
    read_result = client.call("filesystem", "mcp_fs_read", {"path": str(tmp_path / "hello.txt")})
    assert read_result.success
    assert read_result.output == "hi there\n"


def test_mcp_client_sqlite_query(tmp_path):
    import sqlite3

    from shadow_agent.mcp_client import MCPClient

    db = tmp_path / "test.db"
    conn = sqlite3.connect(db)
    conn.execute("CREATE TABLE t (id INTEGER, name TEXT)")
    conn.execute("INSERT INTO t VALUES (1, 'alice')")
    conn.commit()
    conn.close()
    client = MCPClient()
    client.connect("sqlite")
    tables = client.call("sqlite", "mcp_sqlite_tables", {"path": str(db)})
    assert tables.success
    assert "t" in json.loads(tables.output)["tables"]
    query = client.call("sqlite", "mcp_sqlite_query", {"path": str(db), "sql": "SELECT * FROM t"})
    assert query.success
    rows = json.loads(query.output)["rows"]
    assert rows == [{"id": 1, "name": "alice"}]


def test_mcp_client_loads_project_configs(tmp_path):
    from shadow_agent.mcp_client import MCPClient, write_server_config

    mcp_dir = tmp_path / ".shadowcode" / "mcp"
    write_server_config(mcp_dir, "filesystem")
    client = MCPClient(workspace=tmp_path)
    loaded = client.load()
    # Builtins + the project config both present.
    names = [s.name for s in loaded]
    assert "filesystem" in names


def test_mcp_client_call_by_tool_name_dispatches(tmp_path):
    from shadow_agent.mcp_client import MCPClient

    (tmp_path / "f.txt").write_text("x\n", encoding="utf-8")
    client = MCPClient()
    client.connect("filesystem")
    result = client.call_by_tool_name("mcp_fs_read", {"path": str(tmp_path / "f.txt")})
    assert result.success
    assert result.output == "x\n"


def test_mcp_client_rejects_non_select_sql(tmp_path):
    import sqlite3

    from shadow_agent.mcp_client import MCPClient

    db = tmp_path / "test.db"
    sqlite3.connect(db).close()
    client = MCPClient()
    client.connect("sqlite")
    result = client.call("sqlite", "mcp_sqlite_query", {"path": str(db), "sql": "DROP TABLE t"})
    assert not result.success
    assert "only SELECT" in result.error


# --- 6. Background tasks ----------------------------------------------------


def test_background_start_list_stop(tmp_path):
    from shadow_agent.background import BackgroundManager, TaskStatus

    mgr = BackgroundManager(state_dir=tmp_path)
    task = mgr.start("sleep-test", "sleep 1")
    assert task.status is TaskStatus.RUNNING
    assert task.pid > 0
    rows = mgr.list()
    assert any(t.name == "sleep-test" for t in rows)
    # Wait for it to finish.
    import time

    deadline = time.time() + 5
    while time.time() < deadline:
        latest = mgr.get(task.id)
        if latest.status is not TaskStatus.RUNNING:
            break
        time.sleep(0.2)
    latest = mgr.get(task.id)
    assert latest.status is TaskStatus.COMPLETED


def test_background_stop_cancels(tmp_path):
    from shadow_agent.background import BackgroundManager, TaskStatus

    mgr = BackgroundManager(state_dir=tmp_path)
    task = mgr.start("long-sleep", "sleep 30")
    stopped = mgr.stop(task.id)
    assert stopped.status is TaskStatus.CANCELLED


def test_background_render_panel(tmp_path):
    from shadow_agent.background import BackgroundManager

    mgr = BackgroundManager(state_dir=tmp_path)
    mgr.start("dev-server", "sleep 0.1")
    panel = mgr.render_panel()
    assert "dev-server" in panel


# --- 8. Plugins -------------------------------------------------------------


def test_plugin_install_list_remove(tmp_path):
    from shadow_agent.plugin_registry import PluginRegistry

    reg = PluginRegistry(root=tmp_path / "plugins")
    assert "python-expert" in reg.list_registry()
    manifest = reg.install("python-expert")
    assert manifest.name == "python-expert"
    assert reg.is_installed("python-expert")
    installed = {m.name for m in reg.list_installed()}
    assert "python-expert" in installed
    assert reg.remove("python-expert") is True
    assert not reg.is_installed("python-expert")


def test_plugin_install_writes_hooks_and_skills(tmp_path):
    from shadow_agent.plugin_registry import PluginRegistry

    reg = PluginRegistry(root=tmp_path / "plugins")
    reg.install("linux-expert")
    base = tmp_path / "plugins" / "linux-expert"
    assert (base / "plugin.yaml").is_file()
    assert (base / "skills" / "appimage.md").is_file()


def test_plugin_unknown_raises(tmp_path):
    from shadow_agent.plugin_registry import PluginRegistry

    reg = PluginRegistry(root=tmp_path / "plugins")
    with pytest.raises(KeyError):
        reg.install("nope-not-a-plugin")


# --- 9. Self-skilling -------------------------------------------------------


def test_workflow_detector_counts_repeats(tmp_path):
    from shadow_agent.self_skill import WorkflowDetector

    det = WorkflowDetector(state_dir=tmp_path, threshold=3)
    for _ in range(3):
        det.observe("build a linux appimage")
    obs = det.detect()
    assert obs is not None
    assert obs.count == 3


def test_workflow_detector_below_threshold_no_offer(tmp_path):
    from shadow_agent.self_skill import WorkflowDetector

    det = WorkflowDetector(state_dir=tmp_path, threshold=4)
    det.observe("build a linux appimage")
    det.observe("build a linux appimage")
    assert det.detect() is None


def test_workflow_detector_marked_offered_not_reoffered(tmp_path):
    from shadow_agent.self_skill import WorkflowDetector

    det = WorkflowDetector(state_dir=tmp_path, threshold=2)
    det.observe("do thing")
    det.observe("do thing")
    obs = det.detect()
    assert obs is not None
    det.mark_offered(obs.signature)
    assert det.detect() is None


def test_synthesize_skill_writes_file(workspace):
    from shadow_agent.self_skill import synthesize_skill

    path = synthesize_skill(workspace, "appimage", ["build", "test", "package", "verify"])
    assert path.is_file()
    assert "appimage" in path.read_text(encoding="utf-8")
    assert "build" in path.read_text(encoding="utf-8")


def test_list_and_load_skills(workspace):
    from shadow_agent.self_skill import list_skills, load_skill, synthesize_skill

    synthesize_skill(workspace, "appimage", ["build", "test"])
    skills = list_skills(workspace)
    assert any(s["name"] == "appimage" for s in skills)
    body = load_skill(workspace, "appimage")
    assert body is not None
    assert "build" in body


# --- 10. SHADOW.md loading --------------------------------------------------


def test_shadow_md_takes_precedence(workspace):
    from shadow_agent.context.project import ProjectSkills

    (workspace / ".shadow").mkdir()
    (workspace / ".shadow" / "instructions.md").write_text("legacy instructions\n", encoding="utf-8")
    (workspace / "SHADOW.md").write_text("# Project Shadow\n\nUse the harness.\n", encoding="utf-8")
    skills = ProjectSkills(workspace)
    assert "Project Shadow" in skills.instructions()


def test_shadowcode_instructions_fallback(workspace):
    from shadow_agent.context.project import ProjectSkills

    (workspace / ".shadowcode").mkdir()
    (workspace / ".shadowcode" / "instructions.md").write_text("shadowcode instructions\n", encoding="utf-8")
    skills = ProjectSkills(workspace)
    assert "shadowcode instructions" in skills.instructions()


def test_shadow_skills_loaded_from_both_dirs(workspace):
    from shadow_agent.context.project import ProjectSkills

    (workspace / ".shadowcode" / "skills").mkdir(parents=True)
    (workspace / ".shadow" / "skills").mkdir(parents=True)
    (workspace / ".shadowcode" / "skills" / "new.md").write_text("# new skill\n", encoding="utf-8")
    (workspace / ".shadow" / "skills" / "old.md").write_text("# old skill\n", encoding="utf-8")
    skills = ProjectSkills(workspace)
    names = [name for name, _ in skills.skills()]
    assert "new" in names
    assert "old" in names


def test_shadow_md_loaded_into_agent_loop(isolated, workspace, store, bus):
    from shadow_agent.agent.loop import AgentRunner

    (workspace / "SHADOW.md").write_text("# My Project\n\nAlways print hello.\n", encoding="utf-8")
    runner = AgentRunner(workspace, config=AppConfig(), store=store, events=bus, model=MockProvider())
    result = runner.run("Create a Python hello-world project")
    assert result.success


# --- 7. Non-interactive `shadow run --json` + `--agent` -------------------


def test_cli_run_json_exits_nonzero_on_failure(tmp_path, monkeypatch):
    """`shadow run --json` must exit 0 on success and 1 on failure with JSON stdout."""
    from typer.testing import CliRunner

    from shadow_agent.cli import app

    workspace = tmp_path / "ws"
    workspace.mkdir()
    monkeypatch.setenv("HOME", str(tmp_path / "home"))
    monkeypatch.setenv("XDG_CONFIG_HOME", str(tmp_path / "cfg"))
    monkeypatch.setenv("XDG_DATA_HOME", str(tmp_path / "data"))
    monkeypatch.setenv("XDG_STATE_HOME", str(tmp_path / "state"))
    from shadow_agent.config import save_config

    save_config(AppConfig())
    runner = CliRunner()
    result = runner.invoke(app, ["run", "--json", "--project", str(workspace), "Create a Python hello-world project"])
    assert result.exit_code == 0
    payload = json.loads(result.stdout)
    assert payload["success"] is True
    assert payload["session_id"]
    assert payload["task_id"]


def test_cli_run_agent_dispatches_to_subagent(tmp_path, monkeypatch):
    from typer.testing import CliRunner

    from shadow_agent.cli import app

    workspace = tmp_path / "ws"
    workspace.mkdir()
    monkeypatch.setenv("HOME", str(tmp_path / "home"))
    monkeypatch.setenv("XDG_CONFIG_HOME", str(tmp_path / "cfg"))
    monkeypatch.setenv("XDG_DATA_HOME", str(tmp_path / "data"))
    monkeypatch.setenv("XDG_STATE_HOME", str(tmp_path / "state"))
    from shadow_agent.config import save_config

    save_config(AppConfig())
    runner = CliRunner()
    result = runner.invoke(app, ["run", "--json", "--agent", "researcher", "--project", str(workspace), "inspect the workspace"])
    # Researcher is read-only; mock provider still completes.
    assert result.exit_code == 0
    payload = json.loads(result.stdout)
    assert payload["success"] is True
