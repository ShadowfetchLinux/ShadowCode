from __future__ import annotations

from shadow_agent.models.types import ToolCall
from shadow_agent.tools.filesystem import delete_file, edit_file, list_files, read_file, write_file
from shadow_agent.tools.sandbox import WorkspaceSandbox


def _call(name: str, **kwargs) -> ToolCall:
    return ToolCall(id="t1", tool_name=name, arguments=kwargs)


def test_write_read_edit_list_delete(workspace):
    box = WorkspaceSandbox(workspace)
    assert write_file(box, _call("write_file", path="src/app.py", content="print(1)\n")).success
    listed = list_files(box, _call("list_files", path="."))
    assert "src/app.py" in listed.output
    read = read_file(box, _call("read_file", path="src/app.py"))
    assert "print(1)" in read.output
    edited = edit_file(box, _call("edit_file", path="src/app.py", old_string="print(1)", new_string="print(2)"))
    assert edited.success
    assert "print(2)" in (workspace / "src" / "app.py").read_text(encoding="utf-8")
    dup = edit_file(box, _call("edit_file", path="src/app.py", old_string="missing", new_string="x"))
    assert not dup.success
    assert delete_file(box, _call("delete_file", path="src/app.py")).success
    assert not (workspace / "src" / "app.py").exists()


def test_edit_hunks(workspace):
    box = WorkspaceSandbox(workspace)
    (workspace / "f.py").write_text("a\nb\nc\n", encoding="utf-8")
    result = edit_file(
        box,
        _call("edit_file", path="f.py", hunks=[{"start_line": 2, "end_line": 2, "replacement": "B\n"}]),
    )
    assert result.success
    assert (workspace / "f.py").read_text(encoding="utf-8") == "a\nB\nc\n"
