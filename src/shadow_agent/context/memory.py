"""Project, task, and session memory — local, human-readable, provider-agnostic."""

from __future__ import annotations

from pathlib import Path

from shadow_agent import paths


class MemoryStore:
    def __init__(self, workspace: Path, task_id: str) -> None:
        self.workspace = Path(workspace)
        self.task_id = task_id
        self.project_dir = self.workspace / ".shadow" / "memory"
        self.task_dir = paths.task_dir(task_id)
        self.project_dir.mkdir(parents=True, exist_ok=True)

    @property
    def project_file(self) -> Path:
        return self.project_dir / "project.md"

    @property
    def task_file(self) -> Path:
        return self.task_dir / "memory.md"

    def load_project(self) -> str:
        if self.project_file.is_file():
            return self.project_file.read_text(encoding="utf-8")
        return ""

    def load_task(self) -> str:
        if self.task_file.is_file():
            return self.task_file.read_text(encoding="utf-8")
        return ""

    def append_project(self, note: str) -> None:
        existing = self.load_project()
        blob = (existing + "\n" if existing and not existing.endswith("\n") else existing) + note.rstrip() + "\n"
        self.project_file.write_text(blob, encoding="utf-8")

    def append_task(self, note: str) -> None:
        existing = self.load_task()
        blob = (existing + "\n" if existing and not existing.endswith("\n") else existing) + note.rstrip() + "\n"
        self.task_file.write_text(blob, encoding="utf-8")

    def combined(self) -> str:
        parts = []
        project = self.load_project()
        task = self.load_task()
        if project:
            parts.append("## Project memory\n" + project)
        if task:
            parts.append("## Task memory\n" + task)
        return "\n\n".join(parts)
