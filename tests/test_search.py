from __future__ import annotations

from shadow_agent.models.types import ToolCall
from shadow_agent.tools.sandbox import WorkspaceSandbox
from shadow_agent.tools.search import search_files, search_symbol, search_text


def test_filename_text_symbol(workspace):
    (workspace / "pkg").mkdir()
    (workspace / "pkg" / "calc.py").write_text("def add(a, b):\n    return a + b\n", encoding="utf-8")
    box = WorkspaceSandbox(workspace)
    files = search_files(box, ToolCall(id="1", tool_name="search_files", arguments={"query": "calc"}))
    assert "pkg/calc.py" in files.output
    text = search_text(box, ToolCall(id="2", tool_name="search_text", arguments={"query": "return a + b"}))
    assert "calc.py" in text.output
    symbol = search_symbol(box, ToolCall(id="3", tool_name="search_symbol", arguments={"query": "add"}))
    assert "def add" in symbol.output
