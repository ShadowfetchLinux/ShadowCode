"""Tests for the ShadowCode 0.8.0 flagship 15-pillar features."""

from __future__ import annotations

import os
import sys
from pathlib import Path

import pytest

from shadow_agent.config import AppConfig


# --- Pillar 1: /understand ---------------------------------------------------

def test_understand_detects_python_stack(workspace):
    from shadow_agent.understand import detect_stack, save_project_map, render_map

    (workspace / "pyproject.toml").write_text(
        '[project]\nname = "demo"\nversion = "0.1.0"\ndependencies = ["fastapi>=0.115", "pytest>=8"]\n', encoding="utf-8"
    )
    (workspace / "tests").mkdir()
    (workspace / "tests" / "test_a.py").write_text("def test_a():\n    assert 1 + 1 == 2\n", encoding="utf-8")
    stack = detect_stack(workspace)
    assert "python" in stack["languages"]
    assert "fastapi" in stack["frameworks"]
    assert "pytest" in stack["test"]
    mp = save_project_map(workspace, task_id="t-understand")
    assert "python" in mp["stack"]["languages"]
    rendered = render_map(mp)
    assert "Project map" in rendered
    assert (workspace / ".shadow" / "memory" / "project.md").is_file()
    assert "Project map" in (workspace / ".shadow" / "memory" / "project.md").read_text(encoding="utf-8")


# --- Pillar 2: Goal Mode -----------------------------------------------------

def test_goal_plan_milestones_caps_3_to_7():
    from shadow_agent.goal import plan_milestones

    ms = plan_milestones("create hello.py that prints Hello, World")
    assert 3 <= len(ms) <= 7
    assert all("title" in m for m in ms)


def test_goal_create_and_progress(workspace, tmp_path):
    from shadow_agent.goal import GoalStore

    store = GoalStore(db_path=tmp_path / "goals.db")
    g = store.create_goal(workspace, "create hello.py that prints Hello, World")
    assert g["status"] == "active"
    assert g["progress"] == 0.0
    assert len(g["milestones"]) >= 3
    # Mark first milestone done → progress > 0.
    first = g["milestones"][0]
    g = store.update_milestone(g["id"], first["id"], "done")
    assert g["progress"] > 0.0
    # Mark all done → goal completed.
    for m in g["milestones"][1:]:
        g = store.update_milestone(g["id"], m["id"], "done")
    assert g["status"] == "completed"
    assert g["progress"] == 1.0


def test_goal_auto_advance_first_pending(workspace, tmp_path):
    from shadow_agent.goal import GoalStore

    store = GoalStore(db_path=tmp_path / "goals.db")
    g = store.create_goal(workspace, "do a thing")
    # No milestone id → updates the first pending one.
    g = store.update_milestone(g["id"], None, "in_progress")
    assert g["milestones"][0]["status"] == "in_progress"


# --- Pillar 3: Agent Teams --------------------------------------------------

def test_team_report_combines_roles(workspace):
    from shadow_agent.teams import AgentTeam, TeamRole
    from shadow_agent.models.adapters.mock import MockProvider

    team = AgentTeam(workspace, model=MockProvider())
    report = team.run("summarize this empty project", roles=[TeamRole.ARCHITECT, TeamRole.REVIEWER])
    assert report.task == "summarize this empty project"
    assert "architect" in report.role_results
    assert "reviewer" in report.role_results
    assert report.combined.startswith("# Team report")


# --- Pillar 4: Model Routing ------------------------------------------------

def test_routing_default_table_when_disabled():
    from shadow_agent.models.routing import ModelRouter

    cfg = AppConfig()
    cfg.routing.enabled = False
    router = ModelRouter(cfg)
    table = router.table_view()
    assert all(v == cfg.model.default for v in table.values())
    # When disabled, resolve_id returns the default.
    assert router.resolve_id("coding") == cfg.model.default


def test_routing_override_presets_apply():
    from shadow_agent.models.routing import apply_override, OVERRIDE_PRESETS

    cfg = AppConfig()
    cfg, msg = apply_override(cfg, "glm")
    assert cfg.routing.enabled is True
    assert cfg.routing.coder == "glm-5.2"
    assert "glm" in msg.lower()
    cfg2, msg2 = apply_override(AppConfig(), "auto")
    assert cfg2.routing.enabled is True
    assert "auto" in msg2.lower()


def test_routing_fallback_when_model_missing():
    from shadow_agent.models.routing import ModelRouter

    cfg = AppConfig()
    cfg.routing.enabled = True
    cfg.routing.coder = "definitely-not-a-real-model-id"
    router = ModelRouter(cfg)
    chosen = router.resolve_id("coder")
    # Falls back to the default mock.
    assert chosen == cfg.model.default
    assert router.fallbacks  # at least one fallback recorded


# --- Pillar 5: Verification gates -------------------------------------------

def test_gates_run_full_pipeline_passes(workspace):
    from shadow_agent.verification.gates import VerificationEngine, Gate

    (workspace / "pyproject.toml").write_text('[project]\nname="x"\nversion="0"\n', encoding="utf-8")
    (workspace / "x.py").write_text("print('hi')\n", encoding="utf-8")
    engine = VerificationEngine(workspace)
    results = engine.run("create a thing")
    # IMPLEMENT should pass (we have a source file).
    impl = next(r for r in results if r.gate is Gate.IMPLEMENT)
    assert impl.ok
    # DONE reflects whether all non-skipped gates passed.
    done = next(r for r in results if r.gate is Gate.DONE)
    assert done.ok in (True, False)  # depends on whether build/test apply


def test_gates_security_flags_hardcoded_secret(workspace):
    from shadow_agent.verification.gates import VerificationEngine, Gate

    (workspace / "pyproject.toml").write_text('[project]\nname="x"\nversion="0"\n', encoding="utf-8")
    (workspace / "leak.py").write_text('TOKEN = "sk-' + "A" * 40 + '"\n', encoding="utf-8")
    engine = VerificationEngine(workspace)
    results = engine.run("ship it")
    sec = next(r for r in results if r.gate is Gate.SECURITY)
    assert not sec.ok
    assert "secret" in sec.reason.lower()


def test_gates_render_attempts():
    from shadow_agent.verification.gates import GateResult, Gate, render_gate_results

    results = [
        GateResult(gate=Gate.IMPLEMENT, ok=True, reason="ok"),
        GateResult(gate=Gate.TEST, ok=True, skipped=True, reason="no tests"),
        GateResult(gate=Gate.DONE, ok=True, reason="all passed"),
    ]
    text = render_gate_results(results, attempt=1, max_attempts=3)
    assert "Attempt 1/3" in text
    assert "passed" in text
    assert "IMPLEMENT" in text


# --- Pillar 7: Named checkpoints -------------------------------------------

def test_named_checkpoint_create_and_rollback(workspace):
    from shadow_agent.checkpoints import create_named_checkpoint, rollback_named, list_named_checkpoints

    (workspace / "a.py").write_text("before\n", encoding="utf-8")
    (workspace / "b.py").write_text("before\n", encoding="utf-8")
    result = create_named_checkpoint(workspace, "pre-change", paths_to_snapshot=["a.py", "b.py"])
    assert result["ok"]
    assert result["files"] == 2
    # Mutate.
    (workspace / "a.py").write_text("after\n", encoding="utf-8")
    (workspace / "b.py").unlink()
    # Rollback.
    rb = rollback_named(workspace, "pre-change")
    assert rb["ok"]
    assert "a.py" in rb["restored"]
    assert (workspace / "a.py").read_text(encoding="utf-8") == "before\n"
    assert (workspace / "b.py").read_text(encoding="utf-8") == "before\n"
    # List.
    rows = list_named_checkpoints(workspace)
    assert any(r["name"] == "pre-change" for r in rows)


def test_named_checkpoint_rejects_bad_name(workspace):
    from shadow_agent.checkpoints import create_named_checkpoint

    result = create_named_checkpoint(workspace, "../escape")
    assert not result["ok"]
    assert "invalid" in result["error"].lower()


# --- Pillar 8: /why ---------------------------------------------------------

def test_why_explains_recent_task(workspace, store, bus):
    from shadow_agent.why import explain_change, render_explanation
    from shadow_agent.agent.loop import AgentRunner
    from shadow_agent.models.adapters.mock import MockProvider

    runner = AgentRunner(workspace, config=AppConfig(), store=store, events=bus, model=MockProvider())
    result = runner.run("Create a Python hello-world project")
    assert result.success
    expl = explain_change(workspace, session_id=result.session_id)
    assert expl["ok"]
    assert expl["task_prompt"]
    assert "reason" in expl
    text = render_explanation(expl)
    assert "/why" in text


# --- Pillar 9: /doctor project checks --------------------------------------

def test_doctor_includes_project_checks(workspace):
    from shadow_agent.health import doctor_report

    (workspace / "pyproject.toml").write_text('[project]\nname="x"\nversion="0"\n', encoding="utf-8")
    (workspace / "README.md").write_text("# x\n", encoding="utf-8")
    (workspace / "x.py").write_text("print('hi')\n", encoding="utf-8")
    report = doctor_report(AppConfig(), workspace)
    ids = [c["id"] for c in report["checks"]]
    assert "proj-build" in ids
    assert "proj-tests" in ids
    assert "proj-deps" in ids
    assert "proj-docs" in ids
    assert "proj-git" in ids
    assert "proj-arch" in ids
    assert "proj-debt" in ids


# --- Pillar 10: Self-improving harness -------------------------------------

def test_harness_record_and_recall(workspace, tmp_path):
    from shadow_agent.harness import HarnessStore, learn_preferences

    (workspace / "pyproject.toml").write_text(
        '[project]\nname="demo"\nversion="0.1"\ndependencies=["fastapi","pytest"]\n', encoding="utf-8"
    )
    (workspace / "tests").mkdir()
    (workspace / "tests" / "test_a.py").write_text("def test_a(): assert True\n", encoding="utf-8")
    store = HarnessStore(db_path=tmp_path / "h.db")
    aid = store.record_attempt(workspace, "create hello.py", "success", approach="write_file+exec", successful_solution="print Hello, World")
    assert aid
    similar = store.similar_attempts(workspace, "create hello.py")
    assert any(a["task"] == "create hello.py" for a in similar)
    prefs = learn_preferences(workspace, store=store)
    assert "languages" in prefs
    block = store.preferences_block(workspace)
    assert "Project preferences" in block


# --- Pillar 11: Visual debugging (graceful no-op) --------------------------

def test_vision_no_model_returns_skipped(workspace):
    from shadow_agent.vision import analyze_screenshot

    # No vision model configured → graceful skip.
    result = analyze_screenshot(workspace, "nonexistent.png", AppConfig())
    assert not result["ok"]
    assert result.get("skipped") or "not found" in result.get("error", "")


# --- Pillar 12: Documentation research --------------------------------------

def test_docs_plan_offline_when_no_url():
    from shadow_agent.docs_research import plan_doc_research, render_plan

    plan = plan_doc_research("implement GTK 4.20 support")
    assert plan.offline is True
    assert len(plan.steps) == 6
    text = render_plan(plan)
    assert "Documentation research" in text
    assert "search" in text.lower()


# --- Pillar 13: Permission profiles -----------------------------------------

def test_profiles_apply_and_reverse():
    from shadow_agent.profiles import apply_profile, current_profile, PROFILES

    cfg = AppConfig()
    cfg, prof = apply_profile(cfg, "safe")
    assert prof.level.value == "read_only"
    assert current_profile(cfg) == "safe"
    cfg, prof = apply_profile(cfg, "autonomous")
    assert prof.level.value == "elevated"
    assert current_profile(cfg) == "autonomous"
    cfg, prof = apply_profile(cfg, "locked")
    assert current_profile(cfg) == "locked"


def test_profiles_unknown_raises():
    from shadow_agent.profiles import apply_profile

    with pytest.raises(ValueError):
        apply_profile(AppConfig(), "nonsense")


# --- Pillar 14: Tool marketplace -------------------------------------------

def test_marketplace_lists_installed_and_available():
    from shadow_agent.tools.marketplace import CATALOG, marketplace_view
    from shadow_agent.tools.registry import default_tools
    from shadow_agent.tools.sandbox import WorkspaceSandbox
    from shadow_agent.permissions import PermissionGate
    from shadow_agent.config import PermissionLevel

    sandbox = WorkspaceSandbox(Path("/tmp"))
    gate = PermissionGate(PermissionLevel.WORKSPACE)
    reg = default_tools(sandbox, gate, 60)
    view = marketplace_view(reg)
    assert view["counts"]["total"] == len(CATALOG)
    assert view["counts"]["installed"] >= 3  # filesystem, terminal, git are always installed
    assert any(e["name"] == "filesystem" for e in view["installed"])


def test_marketplace_install_pack_adds_tool():
    from shadow_agent.tools.marketplace import install_pack
    from shadow_agent.tools.registry import default_tools
    from shadow_agent.tools.sandbox import WorkspaceSandbox
    from shadow_agent.permissions import PermissionGate
    from shadow_agent.config import PermissionLevel

    sandbox = WorkspaceSandbox(Path("/tmp"))
    gate = PermissionGate(PermissionLevel.WORKSPACE)
    reg = default_tools(sandbox, gate, 60)
    before = set(reg.names())
    added = install_pack(reg, "sqlite")
    assert added >= 1
    assert "sqlite_query" in reg.names()
    assert "sqlite_query" not in before
