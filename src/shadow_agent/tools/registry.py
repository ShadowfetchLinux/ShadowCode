from __future__ import annotations

from collections.abc import Callable

from shadow_agent.models.types import ToolCall, ToolResult, ToolSpec
from shadow_agent.permissions import PermissionGate
from shadow_agent.tools.base import Tool
from shadow_agent.tools.filesystem import register_fs_tools
from shadow_agent.tools.git import register_git_tools
from shadow_agent.tools.sandbox import WorkspaceSandbox
from shadow_agent.tools.search import register_search_tools
from shadow_agent.tools.terminal import register_terminal_tools


class ToolRegistry:
    def __init__(self) -> None:
        self._tools: dict[str, Tool] = {}

    def add(
        self,
        name: str,
        description: str,
        parameters: dict,
        handler: Callable[[ToolCall], ToolResult],
        read_only: bool = False,
    ) -> None:
        self._tools[name] = Tool(
            name=name,
            description=description,
            parameters=parameters,
            handler=handler,
            read_only=read_only,
        )

    def get(self, name: str) -> Tool | None:
        return self._tools.get(name)

    def specs(self) -> list[ToolSpec]:
        return [tool.spec() for tool in self._tools.values()]

    def names(self) -> list[str]:
        return list(self._tools)

    def execute(self, call: ToolCall, gate: PermissionGate | None = None) -> ToolResult:
        if gate is not None:
            decision = gate.check_tool(call.tool_name)
            if not decision.allowed:
                return ToolResult(id=call.id, success=False, error=decision.reason, metadata={"permission": False})
        tool = self._tools.get(call.tool_name)
        if tool is None:
            return ToolResult(id=call.id, success=False, error=f"unknown tool: {call.tool_name}")
        return tool.run(call)


def default_tools(
    sandbox: WorkspaceSandbox,
    gate: PermissionGate,
    timeout_sec: int = 60,
) -> ToolRegistry:
    registry = ToolRegistry()
    register_fs_tools(registry, sandbox)
    register_terminal_tools(registry, sandbox, gate, timeout_sec=timeout_sec)
    register_git_tools(registry, sandbox)
    register_search_tools(registry, sandbox)
    registry.add(
        "update_plan",
        "Update an observable plan step status or add a step.",
        {
            "type": "object",
            "properties": {
                "step_id": {"type": "string"},
                "status": {"type": "string"},
                "title": {"type": "string"},
                "detail": {"type": "string"},
            },
        },
        lambda call: ToolResult(id=call.id, success=True, output="plan-hook", metadata=dict(call.arguments)),
    )
    return registry
