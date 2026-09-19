from __future__ import annotations

from collections.abc import Callable
from typing import Any

from pydantic import BaseModel

from shadow_agent.models.types import ToolCall, ToolResult, ToolSpec


class Tool(BaseModel):
    name: str
    description: str
    parameters: dict[str, Any]
    handler: Callable[..., ToolResult]
    read_only: bool = False

    model_config = {"arbitrary_types_allowed": True}

    def spec(self) -> ToolSpec:
        return ToolSpec(name=self.name, description=self.description, parameters=self.parameters)

    def run(self, call: ToolCall) -> ToolResult:
        try:
            return self.handler(call)
        except Exception as exc:
            return ToolResult(id=call.id, success=False, error=str(exc))
