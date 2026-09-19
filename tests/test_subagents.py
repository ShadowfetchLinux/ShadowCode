from __future__ import annotations

from shadow_agent.agent.loop import AgentRunner
from shadow_agent.agent.subagents import SubagentHost, SubagentRole
from shadow_agent.config import AppConfig
from shadow_agent.events import EventBus
from shadow_agent.models.adapters.mock import MockProvider
from shadow_agent.store import Store


def test_subagent_hooks_run_read_only_planner(isolated, workspace, store: Store, bus: EventBus):
    runner = AgentRunner(workspace, config=AppConfig(), store=store, events=bus, model=MockProvider())
    host = SubagentHost(runner)
    result = host.spawn(SubagentRole.PLANNER, "inspect this empty project")
    assert result.role is SubagentRole.PLANNER
    assert result.result.session_id == runner.session_id
