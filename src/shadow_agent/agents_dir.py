"""Subagent role definitions + .shadowcode/agents/ loading.

Each subagent role has:
  - a default instruction block (used if no .shadowcode/agents/<role>.md exists)
  - a default tool allowlist
  - a read-only flag

Project overrides live at ``.shadowcode/agents/<role>.md`` and may begin with
YAML front matter:

    ---
    tools: [list_files, read_file, search_text]
    read_only: true
    ---
    You are the architect. Read the codebase and propose a plan...

The body after the front matter is the agent's instruction block.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from enum import Enum
from pathlib import Path
from typing import Any

import yaml


class SubagentRole(str, Enum):
    ARCHITECT = "architect"
    CODER = "coder"
    SECURITY = "security"
    TESTER = "tester"
    RESEARCHER = "researcher"
    LINUX_EXPERT = "linux-expert"
    REVIEWER = "reviewer"


# Tool allowlists per role. The host filters the global tool registry down
# to these names so each subagent only sees the tools it should call.
DEFAULT_TOOL_ALLOWLIST: dict[SubagentRole, list[str]] = {
    SubagentRole.ARCHITECT: ["list_files", "read_file", "search_text", "search_files", "update_plan", "update_todos"],
    SubagentRole.CODER: ["list_files", "read_file", "write_file", "edit_file", "apply_patch", "search_text", "exec", "update_plan", "update_todos"],
    SubagentRole.SECURITY: ["list_files", "read_file", "search_text", "search_files", "git_status", "git_diff", "git_log", "exec", "update_plan"],
    SubagentRole.TESTER: ["list_files", "read_file", "write_file", "edit_file", "exec", "search_text", "update_plan"],
    SubagentRole.RESEARCHER: ["list_files", "read_file", "search_text", "search_files", "search_symbol", "update_plan"],
    SubagentRole.LINUX_EXPERT: ["list_files", "read_file", "write_file", "edit_file", "exec", "search_text", "update_plan"],
    SubagentRole.REVIEWER: ["list_files", "read_file", "search_text", "git_diff", "git_status", "git_log", "update_plan"],
}


DEFAULT_READ_ONLY: dict[SubagentRole, bool] = {
    SubagentRole.ARCHITECT: True,
    SubagentRole.CODER: False,
    SubagentRole.SECURITY: True,
    SubagentRole.TESTER: False,
    SubagentRole.RESEARCHER: True,
    SubagentRole.LINUX_EXPERT: False,
    SubagentRole.REVIEWER: True,
}


DEFAULT_INSTRUCTIONS: dict[SubagentRole, str] = {
    SubagentRole.ARCHITECT: (
        "You are the ARCHITECT subagent. Read the codebase and propose a concrete plan. "
        "Do not write code; produce a step-by-step plan with file targets and risks. "
        "Return a concise plan summary."
    ),
    SubagentRole.CODER: (
        "You are the CODER subagent. Implement the plan with minimal, targeted edits. "
        "Prefer edit_file / apply_patch over rewriting files. Run tests after changes."
    ),
    SubagentRole.SECURITY: (
        "You are the SECURITY subagent. Audit the staged changes for vulnerabilities, "
        "dangerous patterns, and unsafe commands. Report findings; do not fix them."
    ),
    SubagentRole.TESTER: (
        "You are the TESTER subagent. Run the test suite and report failures. "
        "If a test fails, you may write a minimal fix only after re-running the test."
    ),
    SubagentRole.RESEARCHER: (
        "You are the RESEARCHER subagent. Investigate the question using read-only tools. "
        "Return a concise summary with file:line citations."
    ),
    SubagentRole.LINUX_EXPERT: (
        "You are the LINUX-EXPERT subagent. Handle packaging, systemd, AppImage, and "
        "shell-tooling tasks. Prefer existing scripts over inventing new ones."
    ),
    SubagentRole.REVIEWER: (
        "You are the REVIEWER subagent. Review the diff for correctness, style, and "
        "regressions. Return a verdict (approve / request-changes) with specific notes."
    ),
}


@dataclass
class AgentSpec:
    role: SubagentRole
    instructions: str = ""
    tools: list[str] = field(default_factory=list)
    read_only: bool = False
    source: str = "default"  # default | project

    def to_dict(self) -> dict[str, Any]:
        return {
            "role": self.role.value,
            "instructions": self.instructions[:200],
            "tools": self.tools,
            "read_only": self.read_only,
            "source": self.source,
        }


_FRONT_MATTER = re.compile(r"^---\s*\n(.*?)\n---\s*\n?(.*)$", re.DOTALL)


def load_agent_spec(workspace: Path, role: SubagentRole) -> AgentSpec:
    """Load a subagent spec from ``.shadowcode/agents/<role>.md`` or use defaults."""
    for base in [workspace / ".shadowcode" / "agents", workspace / ".shadow" / "agents"]:
        path = base / f"{role.value}.md"
        if path.is_file():
            text = path.read_text(encoding="utf-8")
            meta, body = _parse_front_matter(text)
            tools = list(meta.get("tools") or DEFAULT_TOOL_ALLOWLIST[role])
            read_only = bool(meta.get("read_only", DEFAULT_READ_ONLY[role]))
            instructions = (body or text).strip() or DEFAULT_INSTRUCTIONS[role]
            return AgentSpec(role=role, instructions=instructions, tools=tools, read_only=read_only, source="project")
    return AgentSpec(
        role=role,
        instructions=DEFAULT_INSTRUCTIONS[role],
        tools=list(DEFAULT_TOOL_ALLOWLIST[role]),
        read_only=DEFAULT_READ_ONLY[role],
        source="default",
    )


def list_agent_specs(workspace: Path) -> list[AgentSpec]:
    return [load_agent_spec(workspace, role) for role in SubagentRole]


def _parse_front_matter(text: str) -> tuple[dict[str, Any], str]:
    match = _FRONT_MATTER.match(text)
    if not match:
        return {}, text
    try:
        meta = yaml.safe_load(match.group(1)) or {}
    except yaml.YAMLError:
        return {}, text
    if not isinstance(meta, dict):
        return {}, text
    return meta, match.group(2).strip()
