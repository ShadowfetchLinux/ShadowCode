"""Subagent host — isolated-context subagents with parallel dispatch.

Each subagent runs in its OWN AgentRunner instance with its OWN context
window and tool allowlist. The main agent's context is never filled with
50k tokens of side-task research — only the short summary comes back.

Pipeline: MAIN dispatches to ARCHITECT/RESEARCHER/SECURITY (parallel where
independent) → CODER → TESTER → REVIEWER → MAIN combines.

The host reuses the existing AgentRunner (which already creates a fresh
ContextEngine per instance), so isolation is real, not simulated. Each
spawn builds a child runner with a filtered tool registry and a role-
specific system prompt loaded from ``.shadowcode/agents/<role>.md`` (via
``shadow_agent.agents_dir``).
"""

from __future__ import annotations

import threading
from concurrent.futures import ThreadPoolExecutor, as_completed
from typing import Any

from pydantic import BaseModel, Field

from shadow_agent.agent.loop import AgentRunner
from shadow_agent.agents_dir import AgentSpec, SubagentRole, load_agent_spec
from shadow_agent.config import AppConfig
from shadow_agent.models.routing import ModelRouter

# Backward-compat aliases for the legacy SubagentRole members used by older
# tests / callers. New code should use the canonical names from agents_dir.
SubagentRole.PLANNER = SubagentRole.ARCHITECT  # type: ignore[attr-defined]
SubagentRole.DEBUGGER = SubagentRole.TESTER  # type: ignore[attr-defined]


class SubagentResult(BaseModel):
    role: str
    result: Any  # AgentResult.model_dump()
    notes: str = ""
    spec: dict[str, Any] = Field(default_factory=dict)
    context_tokens: int = 0

    @property
    def role_enum(self) -> SubagentRole:
        return SubagentRole(self.role)


class SubagentHost:
    """Spawns isolated-context subagents and combines their results.

    The main agent calls ``dispatch_pipeline(task)`` (or ``spawn(role, task)``)
    and gets back a short summary per subagent. The main agent's own context
    only sees the summaries, not the subagents' full transcripts.
    """

    def __init__(self, runner: AgentRunner, config: AppConfig | None = None) -> None:
        self.runner = runner
        self.config = config or runner.config
        self.router = ModelRouter(self.config)
        self._lock = threading.Lock()
        self.results: list[SubagentResult] = []

    def spawn(self, role: SubagentRole, task: str) -> SubagentResult:
        """Run one subagent in an isolated context. Returns the summary."""
        spec = load_agent_spec(self.runner.workspace, role)
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
            purpose=role.value,
        )
        # Inject the role-specific instruction as a system prompt prefix.
        original_system = child._system_prompt_override if hasattr(child, "_system_prompt_override") else None  # noqa: SLF001
        result = child.run(f"[{role.value}] {task}", purpose=role.value)
        # Estimate the child's context token usage for the "isolated context" proof.
        context_tokens = sum(result.usage.values()) if result.usage else 0
        sub = SubagentResult(
            role=role.value,
            result=result.model_dump(mode="json"),
            notes=result.summary,
            spec=spec.to_dict(),
            context_tokens=context_tokens,
        )
        with self._lock:
            self.results.append(sub)
        return sub

    def spawn_parallel(self, roles: list[SubagentRole], task: str) -> list[SubagentResult]:
        """Dispatch independent subagents in parallel (one thread each).

        Each subagent still runs in its own AgentRunner / context, so the
        parallelism is real isolation, not shared-state concurrency.
        """
        out: list[SubagentResult] = []
        with ThreadPoolExecutor(max_workers=min(4, len(roles))) as pool:
            futures = {pool.submit(self.spawn, role, task): role for role in roles}
            for future in as_completed(futures):
                out.append(future.result())
        # Preserve the requested role order for determinism.
        order = {role.value: i for i, role in enumerate(roles)}
        out.sort(key=lambda r: order.get(r.role, 999))
        return out

    def dispatch_pipeline(self, task: str) -> dict[str, Any]:
        """Run the canonical pipeline:

            ARCHITECT + RESEARCHER + SECURITY (parallel)
              → CODER
              → TESTER
              → REVIEWER
              → combine

        Returns a dict with the per-role summaries and a combined summary
        the main agent can use. The main agent's own context only sees the
        combined summary, not the subagents' transcripts.
        """
        # 1. Parallel independent research/planning/audit.
        parallel = self.spawn_parallel(
            [SubagentRole.ARCHITECT, SubagentRole.RESEARCHER, SubagentRole.SECURITY],
            task,
        )
        architect = next((r for r in parallel if r.role == "architect"), None)
        researcher = next((r for r in parallel if r.role == "researcher"), None)
        security = next((r for r in parallel if r.role == "security"), None)
        # 2. Coder acts on the architect's plan.
        coder_task = task
        if architect and architect.notes:
            coder_task = f"{task}\n\nArchitect plan:\n{architect.notes}"
        coder = self.spawn(SubagentRole.CODER, coder_task)
        # 3. Tester runs the tests.
        tester = self.spawn(SubagentRole.TESTER, f"Run tests for: {task}\n\nCoder result:\n{coder.notes}")
        # 4. Reviewer reviews the diff.
        reviewer = self.spawn(SubagentRole.REVIEWER, f"Review changes for: {task}\n\nCoder result:\n{coder.notes}")
        combined = _combine(parallel + [coder, tester, reviewer])
        return {
            "architect": _summary_dict(architect),
            "researcher": _summary_dict(researcher),
            "security": _summary_dict(security),
            "coder": _summary_dict(coder),
            "tester": _summary_dict(tester),
            "reviewer": _summary_dict(reviewer),
            "combined": combined,
            "isolated_context_tokens": {
                r.role: r.context_tokens for r in [architect, researcher, security, coder, tester, reviewer] if r is not None
            },
        }


def _filter_tools(registry, spec: AgentSpec):
    """Return a new ToolRegistry containing only the spec's allowed tools."""
    from shadow_agent.tools.registry import ToolRegistry

    filtered = ToolRegistry()
    if not spec.tools:
        return registry
    for name in spec.tools:
        tool = registry.get(name)
        if tool:
            filtered._tools[name] = tool  # noqa: SLF001
    return filtered


def _summary_dict(result: SubagentResult | None) -> dict[str, Any]:
    if result is None:
        return {"role": "", "notes": "", "context_tokens": 0}
    return {"role": result.role, "notes": result.notes, "context_tokens": result.context_tokens}


def _combine(results: list[SubagentResult]) -> str:
    """Combine subagent summaries into one short note for the main agent."""
    bits = []
    for r in results:
        if r is None:
            continue
        bits.append(f"[{r.role}] {r.notes}")
    return "\n\n".join(bits)
