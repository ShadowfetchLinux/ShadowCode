from __future__ import annotations

import re
from typing import Literal

from pydantic import BaseModel, Field

Status = Literal["pending", "in_progress", "done", "failed", "skipped"]

# Real models invent statuses ("complete", "finished", "working"...). Normalize.
_STATUS_ALIASES = {
    "complete": "done",
    "completed": "done",
    "finished": "done",
    "success": "done",
    "ok": "done",
    "doing": "in_progress",
    "working": "in_progress",
    "running": "in_progress",
    "active": "in_progress",
    "current": "in_progress",
    "error": "failed",
    "blocked": "failed",
    "cancelled": "skipped",
    "canceled": "skipped",
    "todo": "pending",
    "waiting": "pending",
}


def normalize_status(value: object) -> Status:
    raw = str(value or "").strip().lower()
    if raw in {"pending", "in_progress", "done", "failed", "skipped"}:
        return raw  # type: ignore[return-value]
    return _STATUS_ALIASES.get(raw, "pending")  # type: ignore[return-value]


class PlanStep(BaseModel):
    id: str
    title: str
    status: Status = "pending"
    detail: str = ""


class Plan(BaseModel):
    goal: str
    steps: list[PlanStep] = Field(default_factory=list)
    current: str | None = None

    def to_markdown(self) -> str:
        marks = {
            "pending": "[ ]",
            "in_progress": "[~]",
            "done": "[x]",
            "failed": "[!]",
            "skipped": "[-]",
        }
        lines = [f"# Plan: {self.goal}", ""]
        for step in self.steps:
            mark = marks.get(step.status, "[ ]")
            extra = f" — {step.detail}" if step.detail else ""
            current = "  ← current" if step.id == self.current else ""
            lines.append(f"- {mark} {step.id}: {step.title}{extra}{current}")
        return "\n".join(lines)

    def update(self, step_id: str | None = None, status: Status | None = None, title: str | None = None, detail: str | None = None) -> None:
        normalized = normalize_status(status) if status else None
        if title and not step_id:
            step_id = f"s{len(self.steps) + 1}"
            self.steps.append(PlanStep(id=step_id, title=title, status=normalized or "pending", detail=detail or ""))
            return
        for step in self.steps:
            if step.id == step_id:
                if normalized:
                    step.status = normalized
                if title:
                    step.title = title
                if detail is not None:
                    step.detail = detail
                if normalized == "in_progress":
                    self.current = step.id
                return

    def mark_next(self) -> None:
        for step in self.steps:
            if step.status == "pending":
                step.status = "in_progress"
                self.current = step.id
                return


def initial_plan(task: str) -> Plan:
    lowered = task.lower()
    if re.search(r"hello[- ]?world", lowered):
        titles = [
            "Inspect workspace",
            "Create hello-world files",
            "Run python hello.py",
            "Verify output",
        ]
    elif re.search(r"test|pytest|failing", lowered):
        titles = [
            "Inspect project",
            "Run tests",
            "Diagnose failures",
            "Apply structured fix",
            "Re-run tests",
            "Summarize",
        ]
    else:
        titles = [
            "Understand the task",
            "Inspect relevant files",
            "Implement changes",
            "Verify with commands/tests",
            "Summarize",
        ]
    steps = [PlanStep(id=f"s{i+1}", title=title) for i, title in enumerate(titles)]
    plan = Plan(goal=task, steps=steps)
    plan.mark_next()
    return plan
