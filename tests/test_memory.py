from __future__ import annotations

from shadow_agent.context.memory import MemoryStore
from shadow_agent.context.project import ProjectSkills


def test_project_and_task_memory(isolated, workspace):
    mem = MemoryStore(workspace, "task1")
    mem.append_project("Uses pytest.")
    mem.append_task("Fix add().")
    assert "Uses pytest" in mem.load_project()
    assert "Fix add" in mem.load_task()
    assert "Project memory" in mem.combined()


def test_project_skills(workspace):
    root = workspace / ".shadow"
    (root / "skills").mkdir(parents=True)
    (root / "instructions.md").write_text("Prefer pytest.\n", encoding="utf-8")
    (root / "skills" / "python.md").write_text("# Python\nUse 3.12\n", encoding="utf-8")
    block = ProjectSkills(workspace).prompt_block()
    assert "Prefer pytest" in block
    assert "Use 3.12" in block
