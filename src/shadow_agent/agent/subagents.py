"""Subagent hooks. V1 is a clean interface, not a swarm runtime."""

from __future__ import annotations

from enum import Enum

from pydantic import BaseModel, Field

from shadow_agent.agent.loop import AgentResult, AgentRunner
from shadow_agent.config import AppConfig
from shadow_agent.models.routing import ModelRouter


class SubagentRole(str, Enum):
    PLANNER = "planner"
    CODER = "coder"
    REVIEWER = "reviewer"
    TESTER = "tester"
    RESEARCHER = "researcher"
    DEBUGGER = "debugger"


class SubagentResult(BaseModel):
    role: SubagentRole
    result: AgentResult
    notes: str = ""


class SubagentSpec(BaseModel):
    role: SubagentRole
    tools: list[str] = Field(default_factory=list)
    read_only: bool = False


DEFAULT_SPECS = {
    SubagentRole.PLANNER: SubagentSpec(role=SubagentRole.PLANNER, tools=["list_files", "read_file", "search_text", "update_plan"], read_only=True),
    SubagentRole.CODER: SubagentSpec(role=SubagentRole.CODER),
    SubagentRole.REVIEWER: SubagentSpec(role=SubagentRole.REVIEWER, tools=["list_files", "read_file", "search_text", "git_diff", "git_status"], read_only=True),
    SubagentRole.TESTER: SubagentSpec(role=SubagentRole.TESTER, tools=["list_files", "read_file", "exec", "search_files"]),
    SubagentRole.RESEARCHER: SubagentSpec(role=SubagentRole.RESEARCHER, tools=["list_files", "read_file", "search_text", "search_symbol"], read_only=True),
    SubagentRole.DEBUGGER: SubagentSpec(role=SubagentRole.DEBUGGER),
}


class SubagentHost:
    def __init__(self, runner: AgentRunner, config: AppConfig | None = None) -> None:
        self.runner = runner
        self.config = config or runner.config
        self.router = ModelRouter(self.config)

    def spawn(self, role: SubagentRole, task: str) -> SubagentResult:
        spec = DEFAULT_SPECS[role]
        model = self.router.provider_for(role.value)
        child = AgentRunner(
            self.runner.workspace,
            config=self.config,
            store=self.runner.store,
            events=self.runner.events,
            model=model,
            tools=_filter_tools(self.runner.tools, spec),
            gate=self.runner.gate,
            session_id=self.runner.session_id,
        )
        result = child.run(f"[{role.value}] {task}", purpose=role.value)
        return SubagentResult(role=role, result=result, notes=result.summary)


def _filter_tools(registry, spec: SubagentSpec):
    if not spec.tools:
        return registry
    from shadow_agent.tools.registry import ToolRegistry

    filtered = ToolRegistry()
    for name in spec.tools:
        tool = registry.get(name)
        if tool:
            filtered._tools[name] = tool
    return filtered
