"""Agent Teams — LEAD dispatches to ARCHITECT/CODER/TESTER/SECURITY/REVIEWER.

Built on top of the existing SubagentHost. The LEAD runs a real (minimal)
multi-agent pass: each role runs the agent loop with a role-specific system
prompt and tool subset, then the LEAD combines the results into a single
report.

This is intentionally a *minimal* multi-agent runtime: roles run sequentially
(or in parallel for read-only roles), share the same workspace + session,
and report back through the existing event bus. No external orchestrator.
"""

from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor
from enum import Enum
from pathlib import Path
from typing import Any

from pydantic import BaseModel, Field

from shadow_agent.agent.loop import AgentResult, AgentRunner
from shadow_agent.config import AppConfig
from shadow_agent.events import EventBus
from shadow_agent.models.interface import ModelProvider
from shadow_agent.store import Store


class TeamRole(str, Enum):
    LEAD = "lead"
    ARCHITECT = "architect"
    CODER = "coder"
    TESTER = "tester"
    SECURITY = "security"
    REVIEWER = "reviewer"


ROLE_PROMPTS = {
    TeamRole.LEAD: "You are the LEAD. Break the task into role-specific subtasks, dispatch to ARCHITECT/CODER/TESTER/SECURITY/REVIEWER, then combine.",
    TeamRole.ARCHITECT: "You are the ARCHITECT. Inspect the codebase, propose the structure and interfaces, and identify risks. Read-only.",
    TeamRole.CODER: "You are the CODER. Implement the smallest correct change for the assigned subtask. Write tests if asked.",
    TeamRole.TESTER: "You are the TESTER. Run the test suite, report failures with file:line, and propose minimal repros.",
    TeamRole.SECURITY: "You are SECURITY. Review for injection, secrets in code, unsafe deserialization, and permission escalation. Read-only.",
    TeamRole.REVIEWER: "You are the REVIEWER. Diff the change, check style/coverage/edge cases, and approve or request changes. Read-only.",
}

# Read-only roles can run in parallel; CODER mutates and runs alone.
READ_ONLY_ROLES = {TeamRole.ARCHITECT, TeamRole.SECURITY, TeamRole.REVIEWER, TeamRole.TESTER}


class TeamReport(BaseModel):
    task: str
    lead_summary: str
    role_results: dict[str, str] = Field(default_factory=dict)
    success: bool
    combined: str


class AgentTeam:
    """A minimal multi-agent team coordinator.

    The LEAD is the user-facing runner; ARCHITECT/CODER/TESTER/SECURITY/REVIEWER
    are spawned as child AgentRunner instances with role-scoped tools and
    prompts. Results are combined into a single TeamReport.
    """

    def __init__(
        self,
        workspace: Path,
        config: AppConfig | None = None,
        store: Store | None = None,
        events: EventBus | None = None,
        model: ModelProvider | None = None,
        session_id: str | None = None,
    ) -> None:
        self.workspace = Path(workspace).resolve()
        self.config = config or AppConfig()
        self.store = store or Store()
        self.events = events or EventBus()
        self.model = model
        self.session_id = session_id or self.store.create_session(str(self.workspace), self.config.model.default, title="team")

    def run(self, task: str, roles: list[TeamRole] | None = None) -> TeamReport:
        roles = roles or [TeamRole.ARCHITECT, TeamRole.CODER, TeamRole.TESTER, TeamRole.SECURITY, TeamRole.REVIEWER]
        self.events.emit("team.started", {"task": task, "roles": [r.value for r in roles]}, session_id=self.session_id)
        # Phase 1: read-only roles in parallel.
        read_roles = [r for r in roles if r in READ_ONLY_ROLES]
        write_roles = [r for r in roles if r not in READ_ONLY_ROLES]
        role_results: dict[str, str] = {}

        def _spawn(role: TeamRole) -> tuple[TeamRole, str]:
            runner = AgentRunner(
                self.workspace,
                config=self.config,
                store=self.store,
                events=self.events,
                model=self.model,
                session_id=self.session_id,
            )
            prompt = f"{ROLE_PROMPTS[role]}\n\nTask: {task}"
            result = runner.run(prompt, purpose=role.value)
            return role, result.summary

        if read_roles:
            with ThreadPoolExecutor(max_workers=min(4, len(read_roles))) as pool:
                for role, summary in pool.map(_spawn, read_roles):
                    role_results[role.value] = summary
                    self.events.emit("team.role.done", {"role": role.value, "summary": summary}, session_id=self.session_id)

        # Phase 2: write roles sequentially (they mutate the workspace).
        for role in write_roles:
            role, summary = _spawn(role)
            role_results[role.value] = summary
            self.events.emit("team.role.done", {"role": role.value, "summary": summary}, session_id=self.session_id)

        # Phase 3: LEAD combines.
        combined = self._combine(task, role_results)
        success = all("fail" not in s.lower() and "error" not in s.lower() for s in role_results.values())
        self.events.emit("team.completed", {"success": success, "roles": list(role_results)}, session_id=self.session_id)
        return TeamReport(
            task=task,
            lead_summary=combined.splitlines()[0] if combined else "",
            role_results=role_results,
            success=success,
            combined=combined,
        )

    @staticmethod
    def _combine(task: str, role_results: dict[str, str]) -> str:
        lines = [f"# Team report: {task}", ""]
        for role, summary in role_results.items():
            lines.append(f"## {role.upper()}")
            lines.append(summary.strip() or "(no output)")
            lines.append("")
        lines.append("## LEAD")
        lines.append("Combined the role outputs above. Verify before declaring done.")
        return "\n".join(lines)
