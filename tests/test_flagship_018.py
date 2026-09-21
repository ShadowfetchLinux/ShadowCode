"""Tests for the 0.18.0 flagship upgrade.

- one canonical version everywhere (pyproject, package, README, desktop entry, API)
- test isolation: nothing in the suite may touch the developer's real XDG dirs
- sessions: search · rename · delete (store, API, CLI)
- goals: milestone checklist, run/resume through the job manager, progress %
- rewind from an op card: per-task checkpoint restore
- `shadow update` self-updater (mocked GitHub + real git checkout)
- doctor: UI-build check and `--fix` coverage
- Markdown export reads like a transcript
- background processes, hooks, MCP servers, plugins behind the drawer / settings
"""
from __future__ import annotations

import json
import os
import re
import subprocess
import time
from pathlib import Path

import pytest
from fastapi.testclient import TestClient
from typer.testing import CliRunner

from shadow_agent import __version__, paths
from shadow_agent.api.server import create_app
from shadow_agent.store import Store

ROOT = Path(__file__).resolve().parents[1]
CANONICAL = "0.21.0"


@pytest.fixture
def client(isolated, workspace):
    app = create_app(store=Store(), default_workspace=workspace, detect=False)
    return TestClient(app)


# --- version -------------------------------------------------------------------


def test_version_is_canonical_everywhere() -> None:
    assert __version__ == CANONICAL
    pyproject = (ROOT / "pyproject.toml").read_text(encoding="utf-8")
    assert re.search(rf'^version = "{re.escape(CANONICAL)}"$', pyproject, re.M)
    readme = (ROOT / "README.md").read_text(encoding="utf-8")
    assert CANONICAL in readme
    desktop = (ROOT / "packaging" / "shadow-agent.desktop").read_text(encoding="utf-8")
    assert f"X-ShadowCode-Version={CANONICAL}" in desktop
    assert "Icon=shadow-agent" in desktop and "Name=ShadowCode" in desktop
    # No stray older version strings left in the package or README.
    for stale in ("0.20.0", "0.19.0", "0.17.0", "0.16.0", "0.15.0", "0.13.0"):
        assert stale not in pyproject
        assert stale not in readme


def test_api_reports_canonical_version(client) -> None:
    assert client.get("/api/version").json() == {"name": "ShadowCode", "version": CANONICAL, "binary": "shadow", "desktop_id": "shadow-agent"}
    assert client.get("/api/health").json()["version"] == CANONICAL


def test_cli_version_flag() -> None:
    from shadow_agent.cli import app

    result = CliRunner().invoke(app, ["--version"])
    assert result.exit_code == 0
    assert CANONICAL in result.output


# --- isolation ---------------------------------------------------------------------


def test_suite_never_touches_real_xdg_dirs() -> None:
    real_home = Path(os.path.expanduser("~"))
    for p in (paths.config_file(), paths.db_file(), paths.state_dir()):
        assert not str(p).startswith(str(real_home / ".config")), p
        assert not str(p).startswith(str(real_home / ".local")), p
    assert os.environ.get("SHADOW_AGENT_NO_NOTIFY") == "1"
    assert os.environ.get("SHADOW_AGENT_DOCTOR_NO_INSTALL") == "1"


# --- sessions: search / rename / delete --------------------------------------------


def test_store_search_rename_delete_sessions(isolated, workspace) -> None:
    store = Store()
    a = store.create_session(str(workspace), "mock", title="Fix the login bug")
    b = store.create_session(str(workspace), "mock", title="Docs sweep")
    store.create_task(b, "rewrite the README intro")
    assert [s["id"] for s in store.search_sessions("login")] == [a]
    assert [s["id"] for s in store.search_sessions("readme")] == [b]  # matches a task prompt, case-insensitive
    assert len(store.search_sessions("")) == 2
    store.set_session_title(a, "Login: session cookie fix")
    assert store.get_session(a)["title"] == "Login: session cookie fix"
    assert store.delete_session(a) is True
    assert store.get_session(a) is None
    assert store.list_tasks(a) == []
    assert store.delete_session(a) is False


def test_api_sessions_search_rename_delete(client, workspace) -> None:
    s1 = client.post("/api/sessions", json={"workspace": str(workspace), "title": "alpha task"}).json()["id"]
    s2 = client.post("/api/sessions", json={"workspace": str(workspace), "title": "beta task"}).json()["id"]
    hits = client.get("/api/sessions", params={"q": "alpha"}).json()["sessions"]
    assert [h["id"] for h in hits] == [s1]
    renamed = client.patch(f"/api/sessions/{s1}", json={"workspace": "", "title": "alpha renamed"}).json()
    assert renamed["title"] == "alpha renamed"
    assert client.get(f"/api/sessions/{s1}").json()["title"] == "alpha renamed"
    assert client.delete(f"/api/sessions/{s2}").json()["ok"] is True
    assert client.get(f"/api/sessions/{s2}").status_code == 404
    assert client.delete(f"/api/sessions/{s2}").status_code == 404


def test_cli_sessions_search_rename_delete(isolated, workspace) -> None:
    from shadow_agent.cli import app

    store = Store()
    sid = store.create_session(str(workspace), "mock", title="cli session")
    runner = CliRunner()
    out = runner.invoke(app, ["sessions", "cli"])
    assert out.exit_code == 0 and "cli session" in out.output
    out = runner.invoke(app, ["sessions", sid[:8], "--rename", "renamed by cli"])
    assert out.exit_code == 0
    assert Store().get_session(sid)["title"] == "renamed by cli"
    out = runner.invoke(app, ["sessions", sid[:8], "--delete"])
    assert out.exit_code == 0
    assert Store().get_session(sid) is None


# --- export --------------------------------------------------------------------------


def test_markdown_export_reads_like_a_transcript(isolated, workspace) -> None:
    from shadow_agent.export import export_session

    store = Store()
    sid = store.create_session(str(workspace), "gpt-oss:20b", title="hello run")
    tid = store.create_task(sid, "Create hello.py and run it")
    store.add_event("tool.completed", {"tool": "write_file", "success": True, "icon": "✎", "headline": "wrote hello.py 30 bytes"}, session_id=sid, task_id=tid)
    store.finish_task(tid, "completed", "Created hello.py and ran it.")
    body, media = export_session(store, sid, "md")
    assert media == "text/markdown"
    assert "# ShadowCode session — hello run" in body
    assert "- model: `gpt-oss:20b`" in body
    assert "**You**" in body and "> Create hello.py and run it" in body
    assert "- ✎ wrote hello.py 30 bytes" in body
    assert "**Result**" in body and "Created hello.py and ran it." in body
    assert body.index("**You**") < body.index("**Agent actions**") < body.index("**Result**") < body.index("## Event log")


# --- goals -----------------------------------------------------------------------------


def test_goal_store_reopen_and_delete(isolated, workspace) -> None:
    from shadow_agent.goal import GoalStore

    gs = GoalStore()
    g = gs.create_goal(workspace, "add tests")
    first = g["milestones"][0]["id"]
    gs.update_milestone(g["id"], first, "failed")
    gs.abandon(g["id"])
    reopened = gs.reopen(g["id"])
    assert reopened["status"] == "active"
    assert reopened["milestones"][0]["status"] == "pending"
    assert gs.delete(g["id"]) is True
    assert gs.get_goal(g["id"]) is None


def test_run_goal_marks_milestones_and_stops_on_failure(isolated, workspace) -> None:
    from shadow_agent.goal import GoalStore, run_goal

    gs = GoalStore()
    g = gs.create_goal(workspace, "ship it", [{"title": "one"}, {"title": "two"}, {"title": "three"}])
    calls: list[str] = []

    def runner(task_text: str, milestone_id: str) -> dict:
        calls.append(task_text.splitlines()[-1])
        return {"success": "two" not in task_text, "task_id": "t-" + milestone_id[:4], "summary": "did " + task_text[-3:]}

    final = run_goal(gs, g["id"], runner)
    assert calls == ["Milestone: one", "Milestone: two"]  # stops at the first failure
    statuses = [m["status"] for m in final["milestones"]]
    assert statuses == ["done", "failed", "pending"]
    assert final["progress"] == pytest.approx(1 / 3)
    # Resume: reopen failed → pending, run again with a runner that succeeds.
    gs.reopen(g["id"])
    final = run_goal(gs, g["id"], lambda text, mid: {"success": True})
    assert [m["status"] for m in final["milestones"]] == ["done", "done", "done"]
    assert final["status"] == "completed" and final["progress"] == 1.0


def test_api_goals_checklist_and_resume(client, workspace) -> None:
    created = client.post("/api/goals", json={"instruction": "Create a Python hello-world project", "run": False}).json()
    assert created["progress_pct"] == 0 and created["running"] is False
    assert len(created["milestones"]) >= 3
    gid = created["id"]
    assert [g["id"] for g in client.get("/api/goals").json()["goals"]] == [gid]
    # Manually tick a milestone → progress moves.
    mid = created["milestones"][0]["id"]
    ticked = client.post(f"/api/goals/{gid}/milestones/{mid}", json={"status": "done"}).json()
    assert ticked["progress_pct"] > 0
    assert client.post(f"/api/goals/{gid}/milestones/{mid}", json={"status": "bogus"}).status_code == 400
    # Run (resume) through the job manager with the mock provider.
    started = client.post(f"/api/goals/{gid}/run", json={}).json()
    assert started["running"] is True or started["status"] in {"active", "completed"}
    deadline = time.time() + 60
    while time.time() < deadline:
        goal = client.get(f"/api/goals/{gid}").json()
        if not goal["running"]:
            break
        time.sleep(0.2)
    assert goal["running"] is False
    assert all(m["status"] in {"done", "failed"} for m in goal["milestones"][:2])
    # Milestone tasks recorded a task id and landed in the store as tasks.
    assert any(m["task_id"] for m in goal["milestones"])
    abandoned = client.post(f"/api/goals/{gid}/abandon").json()
    assert abandoned["status"] == "abandoned"
    assert client.delete(f"/api/goals/{gid}").json()["ok"] is True
    assert client.get(f"/api/goals/{gid}").status_code == 404
    assert client.post("/api/goals", json={"instruction": "   ", "run": False}).status_code == 400


# --- rewind from a card ------------------------------------------------------------------


def test_restore_task_rewinds_exactly_that_task(isolated, workspace) -> None:
    from shadow_agent.checkpoints import CheckpointStore, restore_task, task_checkpoint

    (workspace / "a.txt").write_text("original\n", encoding="utf-8")
    cp = CheckpointStore(workspace, "task-1")
    cp.record_call("write_file", {"path": "a.txt", "content": "new"})
    cp.record_call("write_file", {"path": "b.txt", "content": "created"})
    (workspace / "a.txt").write_text("changed by agent\n", encoding="utf-8")
    (workspace / "b.txt").write_text("created by agent\n", encoding="utf-8")
    summary = task_checkpoint(workspace, "task-1")
    assert summary["changes"] == 2 and set(summary["paths"]) == {"a.txt", "b.txt"}
    assert task_checkpoint(workspace, "nope") is None
    result = restore_task(workspace, "task-1")
    assert result["ok"] and set(result["restored"]) == {"a.txt", "b.txt"}
    assert (workspace / "a.txt").read_text(encoding="utf-8") == "original\n"
    assert not (workspace / "b.txt").exists()
    assert restore_task(workspace, "missing")["ok"] is False
    assert restore_task(workspace, "")["ok"] is False


def test_api_task_checkpoint_endpoints(client, workspace) -> None:
    from shadow_agent.checkpoints import CheckpointStore

    (workspace / "x.py").write_text("v1\n", encoding="utf-8")
    CheckpointStore(workspace, "task-9").record_call("edit_file", {"path": "x.py"})
    (workspace / "x.py").write_text("v2\n", encoding="utf-8")
    info = client.get("/api/checkpoints/tasks/task-9").json()
    assert info["rewindable"] is True and info["checkpoint"]["changes"] == 1
    assert client.get("/api/checkpoints/tasks/unknown").json()["rewindable"] is False
    restored = client.post("/api/checkpoints/tasks/task-9/restore").json()
    assert restored["restored"] == ["x.py"]
    assert (workspace / "x.py").read_text(encoding="utf-8") == "v1\n"
    assert client.post("/api/checkpoints/tasks/unknown/restore").status_code == 400
    events = client.get("/api/events").json()["events"]
    assert any(e["type"] == "checkpoint.rewound" and e["task_id"] == "task-9" for e in events)


# --- self-updater ---------------------------------------------------------------------------


def test_version_tuple_and_check_for_update() -> None:
    from shadow_agent.updater import check_for_update, version_tuple

    assert version_tuple("v0.18.0") == (0, 18, 0)
    assert version_tuple("0.18.1-rc1") == (0, 18, 1)
    assert version_tuple("1.2") > version_tuple("0.99.99")

    def fetch_release(url: str):
        assert url.endswith("/releases/latest")
        return {"tag_name": "v0.19.0", "html_url": "https://example.invalid/releases/v0.19.0"}

    info = check_for_update(current="0.18.0", fetch=fetch_release)
    assert info["update_available"] is True and info["latest"] == "0.19.0" and info["source"] == "release"
    assert info["url"].endswith("v0.19.0")

    def fetch_tags_only(url: str):
        if url.endswith("/releases/latest"):
            raise RuntimeError("404")
        return [{"name": "v0.17.0"}, {"name": "v0.18.0"}, {"name": "v0.9.9"}]

    info = check_for_update(current="0.18.0", fetch=fetch_tags_only)
    assert info["update_available"] is False and info["latest"] == "0.18.0" and info["source"] == "tag"

    def fetch_fail(url: str):
        raise ConnectionError("offline")

    info = check_for_update(current="0.18.0", fetch=fetch_fail)
    assert info["update_available"] is False and "offline" in info["error"]


def _git(cwd: Path, *args: str) -> str:
    env = {**os.environ, "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@example.invalid", "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@example.invalid"}
    return subprocess.run(["git", *args], cwd=cwd, capture_output=True, text=True, check=True, env=env).stdout.strip()


def test_apply_update_checks_out_release_tag(tmp_path: Path) -> None:
    from shadow_agent.updater import apply_update

    origin = tmp_path / "origin.git"
    _git(tmp_path, "init", "--bare", "-q", "-b", "main", str(origin))
    work = tmp_path / "work"
    _git(tmp_path, "clone", "-q", str(origin), str(work))
    (work / "VERSION").write_text("0.18.0\n", encoding="utf-8")
    _git(work, "add", "VERSION")
    _git(work, "commit", "-q", "-m", "0.18.0")
    _git(work, "push", "-q", "-u", "origin", "HEAD:main")
    # A second clone is "the installed copy"; origin then moves ahead with a tag.
    installed = tmp_path / "installed"
    _git(tmp_path, "clone", "-q", str(origin), str(installed))
    (work / "VERSION").write_text("0.19.0\n", encoding="utf-8")
    _git(work, "commit", "-q", "-am", "0.19.0")
    _git(work, "tag", "v0.19.0")
    _git(work, "push", "-q", "origin", "HEAD:main", "--tags")

    result = apply_update(tag="v0.19.0", root=installed, reinstall=False)
    assert result["ok"], result
    assert (installed / "VERSION").read_text(encoding="utf-8").strip() == "0.19.0"
    assert result["before"] != result["after"]
    assert "git checkout v0.19.0" in result["steps"]

    # Dirty tree is refused, not clobbered.
    (installed / "VERSION").write_text("local edit\n", encoding="utf-8")
    refused = apply_update(tag="v0.19.0", root=installed, reinstall=False)
    assert refused["ok"] is False and "uncommitted" in refused["error"]
    # Not a git checkout at all.
    assert apply_update(root=tmp_path / "nowhere", reinstall=False)["ok"] is False


def test_cli_update_check_uses_updater(monkeypatch) -> None:
    from shadow_agent.cli import app

    monkeypatch.setattr("shadow_agent.updater.check_for_update", lambda **kw: {"current": "0.18.0", "latest": "0.18.0", "tag": "v0.18.0", "update_available": False, "url": "", "source": "release", "error": ""})
    result = CliRunner().invoke(app, ["update", "--check"])
    assert result.exit_code == 0
    assert "up to date" in result.output


def test_api_update_check_is_offline_safe(client, monkeypatch) -> None:
    monkeypatch.setattr("shadow_agent.updater._default_fetch", lambda url: (_ for _ in ()).throw(ConnectionError("no network")))
    info = client.get("/api/update/check").json()
    assert info["current"] == CANONICAL and info["update_available"] is False


# --- doctor: UI build + fix ----------------------------------------------------------------


def test_ui_build_state_detects_missing_and_stale(tmp_path: Path) -> None:
    from shadow_agent.health import ui_build_state

    root = tmp_path
    ui = root / "ui"
    (ui / "src").mkdir(parents=True)
    assert ui_build_state(root)["ok"] is True  # no package.json → headless install
    (ui / "package.json").write_text("{}", encoding="utf-8")
    assert ui_build_state(root)["ok"] is False and "missing" in ui_build_state(root)["detail"]
    (ui / "dist" / "assets").mkdir(parents=True)
    (ui / "dist" / "index.html").write_text("<html>", encoding="utf-8")
    bundle = ui / "dist" / "assets" / "index-abc.js"
    bundle.write_text("js", encoding="utf-8")
    src = ui / "src" / "App.tsx"
    src.write_text("x", encoding="utf-8")
    old = time.time() - 600
    os.utime(bundle, (old, old))
    state = ui_build_state(root)
    assert state["ok"] is False and state["stale"] is True
    now = time.time() + 5
    os.utime(bundle, (now, now))
    assert ui_build_state(root)["ok"] is True


def test_doctor_report_has_ui_dist_check_and_fix_is_guarded(isolated) -> None:
    from shadow_agent.config import AppConfig
    from shadow_agent.health import doctor_fix, doctor_report

    report = doctor_report(AppConfig(), None)
    ids = {c["id"] for c in report["checks"]}
    assert "ui-dist" in ids
    # Force the UI check to fail and confirm --fix routes to the UI rebuild step
    # (guarded by SHADOW_AGENT_DOCTOR_NO_INSTALL in the suite).
    for check in report["checks"]:
        if check["id"] == "ui-dist":
            check["ok"] = False
    applied = doctor_fix(report, None)
    assert any("UI rebuild" in a for a in applied)


def test_api_doctor_fix_endpoint(client) -> None:
    body = client.post("/api/doctor/fix").json()
    assert "applied" in body and "report" in body
    assert {c["id"] for c in body["report"]["checks"]} >= {"python", "config", "ui-dist"}


# --- drawer / settings back-ends ------------------------------------------------------------


def test_api_routing_table_visible_and_toggle(client) -> None:
    table = client.get("/api/routing").json()
    assert table["enabled"] is False
    assert set(table["table"]) >= {"planning", "coding", "review"}
    assert all(v == table["default"] for v in table["table"].values())
    toggled = client.put("/api/routing", json={"values": {"enabled": True, "coder": "mock"}, "api_key": "", "api_key_env": ""}).json()
    assert toggled["enabled"] is True


def test_api_background_start_and_stop(client) -> None:
    assert client.get("/api/background").json()["tasks"] == []
    task = client.post("/api/background", json={"name": "sleeper", "command": "sleep 30"}).json()
    assert task["status"] == "RUNNING" and task["pid"] > 0
    listed = client.get("/api/background").json()["tasks"]
    assert any(t["id"] == task["id"] for t in listed)
    stopped = client.post(f"/api/background/{task['id']}/stop").json()
    assert stopped["status"] != "RUNNING"
    assert client.post("/api/background/nope/stop").status_code == 404
    assert client.post("/api/background", json={"name": "x", "command": "  "}).status_code == 400


def test_api_hooks_mcp_plugins(client) -> None:
    hooks = client.get("/api/hooks").json()
    names = {h["name"] for h in hooks["hooks"]}
    assert names >= {"block-dangerous-commands", "pytest-after-test"}
    assert all(h["builtin"] for h in hooks["hooks"] if h["name"] in {"block-dangerous-commands", "pytest-after-test"})
    assert hooks["dirs"]
    assert client.get("/api/mcp/servers").json()["servers"] == []
    saved = client.put("/api/mcp/servers", json={"values": {"servers": [{"name": "self", "url": "http://127.0.0.1:7431/sse"}]}, "api_key": "", "api_key_env": ""}).json()
    assert saved["servers"][0]["name"] == "self"
    assert client.get("/api/mcp/servers").json()["servers"][0]["url"] == "http://127.0.0.1:7431/sse"
    assert client.put("/api/mcp/servers", json={"values": {"servers": "nope"}, "api_key": "", "api_key_env": ""}).status_code == 400
    plugins = client.get("/api/plugins").json()
    available = {p["name"] for p in plugins["available"]}
    assert "python-expert" in available
    installed = client.post("/api/plugins/python-expert/install").json()
    assert installed["name"] == "python-expert"
    assert any(p["name"] == "python-expert" for p in client.get("/api/plugins").json()["installed"])
    assert client.post("/api/plugins/python-expert/remove").json()["ok"] is True
    assert client.post("/api/plugins/does-not-exist/install").status_code == 404


def test_verifier_refuses_no_op_completion_for_change_tasks(workspace) -> None:
    """A model that narrates "done" without writing or running anything must
    fail VERIFY (so FIX nudges it) — but read-only questions still pass."""
    from shadow_agent.models.types import ToolCall, ToolResult
    from shadow_agent.verification.verifier import Verifier

    v = Verifier(workspace)
    verdict = v.verify("Create app.py that prints the current date, then run it")
    assert verdict.ok is False and "no files were written" in verdict.reason
    assert Verifier(workspace).verify("Explain how the config loader works").ok is True
    assert Verifier(workspace).verify("What does hooks.py do?").ok is True
    # After a real write the same task verifies.
    v.observe(ToolCall(id="1", tool_name="write_file", arguments={"path": "app.py"}), ToolResult(id="1", success=True, output="ok"))
    assert v.verify("Create app.py that prints the current date, then run it").ok is True
    # A failed write does not count.
    v2 = Verifier(workspace)
    v2.observe(ToolCall(id="2", tool_name="write_file", arguments={"path": "x"}), ToolResult(id="2", success=False, output="", error="denied"))
    assert v2.verify("Add a README").ok is False


def test_tui_status_line_contract(isolated, workspace) -> None:
    from shadow_agent.config import AppConfig
    from shadow_agent.tui.theme import load_theme
    from shadow_agent.tui.transcript import TranscriptModel

    store = Store()
    sid = store.create_session(str(workspace), "mock", title="t")
    model = TranscriptModel(workspace, load_theme("light"), AppConfig(), store, sid)
    model.set_status(model="ollama/gpt-oss:20b", context_pct=7, tokens=512, step=3, busy=False)
    line = model.status_text()
    assert line.split("·") and [part.strip() for part in line.split("·")] == ["ollama/gpt-oss:20b", "ctx 7%", "512 tok", "[ IDLE ]"]
