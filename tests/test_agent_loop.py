from __future__ import annotations

from shadow_agent.agent.loop import AgentRunner
from shadow_agent.config import AppConfig
from shadow_agent.events import EventBus
from shadow_agent.models.adapters.mock import MockProvider
from shadow_agent.store import Store


def test_loop_emits_core_events(isolated, workspace, store: Store, bus: EventBus):
    runner = AgentRunner(workspace, config=AppConfig(), store=store, events=bus, model=MockProvider())
    result = runner.run("summarize this empty project")
    types = {e["type"] for e in bus.history()}
    assert "agent.started" in types
    assert "agent.plan" in types
    assert "model.request" in types
    assert "agent.completed" in types
    assert result.session_id
    assert store.get_task(result.task_id) is not None
