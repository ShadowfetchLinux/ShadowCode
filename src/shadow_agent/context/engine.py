"""Prioritize, compress, truncate, and select context. Never dump a repo."""

from __future__ import annotations

import json

from shadow_agent.models.types import Message, ToolCall, ToolResult
from shadow_agent.planning.plan import Plan


def estimate_tokens(text: str) -> int:
    return max(1, len(text) // 4)


class ContextEngine:
    def __init__(self, limit: int = 128000) -> None:
        self.limit = limit
        self.system: list[Message] = []
        self.plan_text = ""
        self.memory_text = ""
        self.turns: list[Message] = []
        self.compressed_notes: list[str] = []

    def set_system(self, text: str) -> None:
        self.system = [Message(role="system", content=text)]

    def set_plan(self, plan: Plan | str) -> None:
        self.plan_text = plan.to_markdown() if isinstance(plan, Plan) else plan

    def set_memory(self, text: str) -> None:
        self.memory_text = text

    def add_user(self, text: str) -> None:
        self.turns.append(Message(role="user", content=text))

    def add_assistant(self, text: str, tool_calls: list[ToolCall] | None = None) -> None:
        self.turns.append(Message(role="assistant", content=text, tool_calls=tool_calls or []))

    def add_tool_result(self, result: ToolResult, tool_name: str) -> None:
        payload = {
            "id": result.id,
            "success": result.success,
            "output": _truncate(result.output, 12000),
            "error": result.error,
            "metadata": result.metadata,
        }
        # Keep a compact view for the mock / models.
        self.turns.append(
            Message(
                role="tool",
                name=tool_name,
                tool_call_id=result.id,
                content=json.dumps(payload, default=str),
            )
        )

    def messages(self) -> list[Message]:
        prefix: list[Message] = list(self.system)
        extras = []
        if self.memory_text:
            extras.append("Project/task memory:\n" + _truncate(self.memory_text, 4000))
        if self.plan_text:
            extras.append("Current plan:\n" + self.plan_text)
        if self.compressed_notes:
            extras.append("Earlier context (summarized):\n" + "\n".join(self.compressed_notes[-8:]))
        if extras:
            prefix.append(Message(role="system", content="\n\n".join(extras)))
        bundled = prefix + list(self.turns)
        return self.compress(bundled)

    def compress(self, messages: list[Message] | None = None) -> list[Message]:
        rows = list(messages if messages is not None else self.turns)
        budget = int(self.limit * 0.7)
        while _bundle_tokens(rows) > budget and len(self.turns) > 6:
            old = self.turns[:2]
            self.turns = self.turns[2:]
            summary = _summarize(old)
            self.compressed_notes.append(summary)
            extras = []
            if self.memory_text:
                extras.append("Project/task memory:\n" + _truncate(self.memory_text, 4000))
            if self.plan_text:
                extras.append("Current plan:\n" + self.plan_text)
            if self.compressed_notes:
                extras.append("Earlier context (summarized):\n" + "\n".join(self.compressed_notes[-8:]))
            rows = list(self.system)
            if extras:
                rows.append(Message(role="system", content="\n\n".join(extras)))
            rows.extend(self.turns)
        return rows

    def transcript(self) -> list[dict]:
        return [msg.model_dump() for msg in self.turns]


def _bundle_tokens(messages: list[Message]) -> int:
    return sum(estimate_tokens(msg.content) for msg in messages)


def _truncate(text: str, chars: int) -> str:
    if len(text) <= chars:
        return text
    return text[: chars - 20] + "\n…[truncated]…"


def _summarize(messages: list[Message]) -> str:
    bits = []
    for msg in messages:
        bits.append(f"{msg.role}/{msg.name or '-'}: {_truncate(msg.content, 240)}")
    return " | ".join(bits)
