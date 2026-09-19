from __future__ import annotations

from shadow_agent.agent.loop import AgentRunner
from shadow_agent.config import AppConfig
from shadow_agent.events import EventBus
from shadow_agent.models.adapters.mock import MockProvider
from shadow_agent.store import Store


def test_hello_world_inspect_plan_create_run_verify(isolated, workspace, store: Store, bus: EventBus):
    runner = AgentRunner(workspace, config=AppConfig(), store=store, events=bus, model=MockProvider())
    result = runner.run("Create a Python hello-world project")
    assert result.success, result.summary
    assert (workspace / "hello.py").is_file()
    assert "Hello, World!" in (workspace / "hello.py").read_text(encoding="utf-8")
    types = [e["type"] for e in bus.history()]
    assert "agent.planning" in types
    assert "tool.started" in types
    assert "agent.completed" in types
    assert any(e["type"] == "tool.started" and e["payload"].get("tool") == "exec" for e in bus.history())
