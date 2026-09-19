"""Tests for the ShadowCode 0.8.0 second-wave features (part 1: subagents, hooks, permissions)."""

from __future__ import annotations

from pathlib import Path

from shadow_agent.config import AppConfig
from shadow_agent.events import EventBus
from shadow_agent.models.adapters.mock import MockProvider
from shadow_agent.store import Store


# --- 1. Subagents with isolated context ------------------------------------


def test_subagent_runs_in_isolated_context(isolated, workspace, store, bus):
    from shadow_agent.agent.loop import AgentRunner
    from shadow_agent.agent.subagents import SubagentHost
    from shadow_agent.agents_dir import SubagentRole

    runner = AgentRunner(workspace, config=AppConfig(), store=store, events=bus, model=MockProvider())
    host = SubagentHost(runner)
    result = host.spawn(SubagentRole.RESEARCHER, "inspect this empty project")
    assert result.role == "researcher"
    assert result.context_tokens >= 0
    # Main runner's own usage untouched by the child's run.
    assert runner.usage == {"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0}


def test_subagent_parallel_dispatch_returns_all_roles(isolated, workspace, store, bus):
    from shadow_agent.agent.loop import AgentRunner
    from shadow_agent.agent.subagents import SubagentHost
    from shadow_agent.agents_dir import SubagentRole

    runner = AgentRunner(workspace, config=AppConfig(), store=store, events=bus, model=MockProvider())
    host = SubagentHost(runner)
    results = host.spawn_parallel(
        [SubagentRole.ARCHITECT, SubagentRole.RESEARCHER, SubagentRole.SECURITY],
        "audit the workspace",
    )
    assert [r.role for r in results] == ["architect", "researcher", "security"]


def test_subagent_pipeline_combines_roles(isolated, workspace, store, bus):
    from shadow_agent.agent.loop import AgentRunner
    from shadow_agent.agent.subagents import SubagentHost

    runner = AgentRunner(workspace, config=AppConfig(), store=store, events=bus, model=MockProvider())
    host = SubagentHost(runner)
    report = host.dispatch_pipeline("ship a hello-world project")
    assert set(report.keys()) >= {"architect", "researcher", "security", "coder", "tester", "reviewer", "combined"}
    assert report["combined"]


def test_subagent_tool_allowlist_filters_registry(isolated, workspace, store, bus):
    from shadow_agent.agent.loop import AgentRunner
    from shadow_agent.agents_dir import SubagentRole, load_agent_spec

    runner = AgentRunner(workspace, config=AppConfig(), store=store, events=bus, model=MockProvider())
    spec = load_agent_spec(workspace, SubagentRole.RESEARCHER)
    assert "write_file" not in spec.tools
    assert "read_file" in spec.tools


def test_project_agent_file_overrides_defaults(workspace):
    from shadow_agent.agents_dir import SubagentRole, load_agent_spec

    agents_dir = workspace / ".shadowcode" / "agents"
    agents_dir.mkdir(parents=True)
    (agents_dir / "researcher.md").write_text(
        "---\ntools: [list_files, read_file]\nread_only: true\n---\nYou are a custom researcher.\n",
        encoding="utf-8",
    )
    spec = load_agent_spec(workspace, SubagentRole.RESEARCHER)
    assert spec.tools == ["list_files", "read_file"]
    assert spec.read_only is True
    assert "custom researcher" in spec.instructions
    assert spec.source == "project"


# --- 2. Hooks ---------------------------------------------------------------


def test_hook_registry_fire_returns_outcomes():
    from shadow_agent.hooks import HookContext, HookOutcome, HookRegistry

    registry = HookRegistry()

    @registry.on("after_edit", "my-hook")
    def my_hook(ctx):
        return HookOutcome(name="my-hook", event=ctx.event, success=True, message="ran")

    outcomes = registry.fire("after_edit", HookContext(workspace=Path("/tmp")))
    assert len(outcomes) == 1
    assert outcomes[0].name == "my-hook"
    assert outcomes[0].success


def test_hook_before_command_can_block():
    from shadow_agent.hooks import HookContext, HookOutcome, HookRegistry

    registry = HookRegistry()

    @registry.on("before_command", "block-rm-rf")
    def block(ctx):
        if "rm -rf" in ctx.command:
            return HookOutcome(name="block-rm-rf", event=ctx.event, block=True, message="blocked rm -rf")
        return None

    blocked = registry.fire("before_command", HookContext(workspace=Path("/tmp"), command="rm -rf build/"))
    assert any(o.block for o in blocked)
    ok = registry.fire("before_command", HookContext(workspace=Path("/tmp"), command="ls -la"))
    assert not any(o.block for o in ok)


def test_builtin_block_dangerous_hook_fires():
    from shadow_agent.hooks import HookContext, builtin_hooks

    block = next(h for h in builtin_hooks() if h.name == "block-dangerous-commands")
    outcomes = block.handler(HookContext(workspace=Path("/tmp"), command="sudo rm -rf /"))
    assert outcomes is not None
    assert outcomes.block


def test_builtin_hooks_loaded_by_default_registry(workspace):
    from shadow_agent.hooks import default_registry

    registry = default_registry(workspace)
    names = registry.names()
    assert "ruff-format" in names
    assert "block-dangerous-commands" in names
    assert "pytest-after-test" in names
    assert "notify-on-complete" in names


def test_project_hooks_loaded_from_shadowcode(workspace):
    from shadow_agent.hooks import HookContext, default_registry

    hooks_dir = workspace / ".shadowcode" / "hooks"
    hooks_dir.mkdir(parents=True)
    (hooks_dir / "myhook.py").write_text(
        "def register(registry):\n"
        "    @registry.on('after_edit', 'project-hook')\n"
        "    def h(ctx):\n"
        "        from shadow_agent.hooks import HookOutcome\n"
        "        return HookOutcome(name='project-hook', event=ctx.event, success=True, message='project hook ran')\n",
        encoding="utf-8",
    )
    registry = default_registry(workspace)
    assert "project-hook" in registry.names()
    outcomes = registry.fire("after_edit", HookContext(workspace=workspace))
    assert any(o.name == "project-hook" for o in outcomes)


def test_hook_fires_in_agent_loop_after_edit(isolated, workspace, store, bus):
    from shadow_agent.agent.loop import AgentRunner

    runner = AgentRunner(workspace, config=AppConfig(), store=store, events=bus, model=MockProvider())
    result = runner.run("Create a Python hello-world project")
    assert result.success
    events = [e["type"] for e in bus.history()]
    assert any(t == "hook.fired" for t in events)


# --- 3. Permission modes + destructive card --------------------------------


def test_permission_modes_grant_matrix():
    from shadow_agent.permission_modes import PermissionMode, grant_for

    assert grant_for(PermissionMode.PLAN, "filesystem.write") is False
    assert grant_for(PermissionMode.READ, "terminal.safe") is True
    assert grant_for(PermissionMode.EDIT, "git.commit") == "ask"
    assert grant_for(PermissionMode.DEVELOPER, "terminal.destructive") == "ask"
    assert grant_for(PermissionMode.AUTONOMOUS, "git.push") is True
    assert grant_for(PermissionMode.LOCKED, "filesystem.read") is False


def test_permission_mode_for_level_maps_legacy_levels():
    from shadow_agent.permission_modes import PermissionMode, mode_for_level

    assert mode_for_level("read_only") is PermissionMode.READ
    assert mode_for_level("workspace") is PermissionMode.EDIT
    assert mode_for_level("elevated") is PermissionMode.DEVELOPER


def test_destructive_card_renders():
    from shadow_agent.permission_modes import DestructiveCard, render_destructive_card

    card = DestructiveCard(action="rm -rf build/", reason="Agent requested cleanup", permission="terminal.destructive")
    text = render_destructive_card(card)
    assert "🔴 DESTRUCTIVE ACTION" in text
    assert "rm -rf build/" in text
    assert "Reason: Agent requested cleanup" in text
    assert "[y] Allow once" in text
    assert "[a] Always allow" in text
    assert "[n] Deny" in text


def test_destructive_card_parse_decision():
    from shadow_agent.permission_modes import parse_decision

    assert parse_decision("y") == "allow_once"
    assert parse_decision("a") == "always_allow"
    assert parse_decision("n") == "deny"
    assert parse_decision("garbage") == "deny"


def test_all_modes_have_grant_matrix_entries():
    from shadow_agent.permission_modes import GRANT_MATRIX, PermissionMode

    for mode in PermissionMode:
        assert mode in GRANT_MATRIX
        for key in ("filesystem.read", "filesystem.write", "terminal.safe", "terminal.destructive", "git.commit", "git.push", "network"):
            assert key in GRANT_MATRIX[mode]
