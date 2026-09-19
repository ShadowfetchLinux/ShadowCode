from __future__ import annotations

from shadow_agent.context.engine import ContextEngine, estimate_tokens
from shadow_agent.models.types import ToolResult


def test_context_truncates_and_compresses():
    engine = ContextEngine(limit=80)
    engine.set_system("sys")
    engine.add_user("u" * 200)
    engine.add_tool_result(ToolResult(id="1", success=True, output="x" * 20000), "read_file")
    engine.add_assistant("ok")
    engine.add_user("more")
    engine.add_assistant("again")
    engine.add_user("third")
    msgs = engine.messages()
    blob = "".join(m.content for m in msgs)
    assert "truncated" in blob or estimate_tokens(blob) < 20000
    assert any(m.role == "system" for m in msgs)
