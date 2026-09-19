from __future__ import annotations

from shadow_agent.agent.loop import AgentRunner
from shadow_agent.config import AppConfig
from shadow_agent.events import EventBus
from shadow_agent.models.adapters.mock import MockProvider
from shadow_agent.store import Store


def test_analyze_fix_rerun_summarize(isolated, workspace, store: Store, bus: EventBus):
    (workspace / "mathy.py").write_text("def add(a, b):\n    return a - b\n", encoding="utf-8")
    tests = workspace / "tests"
    tests.mkdir()
    (tests / "test_add.py").write_text(
        "from mathy import add\n\n\ndef test_add():\n    assert add(1, 2) == 3\n    assert add(0, 0) == 0\n",
        encoding="utf-8",
    )
    (tests / "__init__.py").write_text("", encoding="utf-8")
    runner = AgentRunner(workspace, config=AppConfig(), store=store, events=bus, model=MockProvider())
    result = runner.run("analyze, find failing tests, fix, rerun, summarize")
    assert result.success, result.summary
    assert "return a + b" in (workspace / "mathy.py").read_text(encoding="utf-8")
    types = [e["type"] for e in bus.history()]
    assert "test.failed" in types
    assert "test.passed" in types
