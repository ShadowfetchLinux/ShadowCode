"""MCP tool catalog: name, description, JSON-schema, dispatcher binding."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Callable

from shadow_agent.mcp_server import tools as T

ToolDispatcher = Callable[[dict[str, Any], "T.ToolContext"], dict[str, Any]]


@dataclass
class ToolDef:
    name: str
    description: str
    input_schema: dict[str, Any]
    dispatcher: ToolDispatcher
    destructive: bool = False


def _schema(props: dict[str, Any], required: list[str] | None = None) -> dict[str, Any]:
    return {"type": "object", "properties": props, "required": required or []}


def _ws_prop() -> dict[str, Any]:
    return {"type": "string", "description": "Absolute project path. Defaults to the server's workspace."}


TOOL_DEFS: list[ToolDef] = [
    ToolDef("shadow_understand",
            "Analyze a project and save a project map (stack, modules, tech debt) to ShadowCode memory. Returns the rendered map. Reuses /understand.",
            _schema({"workspace": _ws_prop(), "task_id": {"type": "string"}}),
            T.shadow_understand),
    ToolDef("shadow_goal",
            "Create, list, advance, or abandon a multi-milestone Goal. action=create needs instruction; advance needs goal_id (+ optional milestone_id, status, detail). Reuses GoalStore.",
            _schema({"action": {"type": "string", "enum": ["create", "list", "get", "advance", "abandon"]},
                     "workspace": _ws_prop(),
                     "instruction": {"type": "string"},
                     "goal_id": {"type": "string"},
                     "milestone_id": {"type": "string"},
                     "status": {"type": "string"},
                     "detail": {"type": "string"}},
            ["action"]),
            T.shadow_goal),
    ToolDef("shadow_status",
            "Report ShadowCode version, provider health, model, permission level, and active jobs.",
            _schema({"workspace": _ws_prop()}),
            T.shadow_status),
    ToolDef("shadow_run",
            "Run one task through the agent loop (UNDERSTAND→PLAN→INSPECT→ACT→OBSERVE→VERIFY) and return the result summary. Reuses AgentRunner. Destructive sub-actions surface as approvals that must be resolved via shadow_approve.",
            _schema({"task": {"type": "string"},
                    "workspace": _ws_prop(),
                    "model": {"type": "string"},
                    "purpose": {"type": "string"},
                    "permission_level": {"type": "string", "enum": ["read_only", "workspace", "elevated"]}},
            ["task"]),
            T.shadow_run),
    ToolDef("shadow_doctor",
            "Run install/config health checks with auto-fix suggestions. Reuses health.doctor_report.",
            _schema({"workspace": _ws_prop()}),
            T.shadow_doctor),
    ToolDef("shadow_why",
            "Explain a change: git log + diff for a path. Read-only.",
            _schema({"workspace": _ws_prop(), "path": {"type": "string"}, "count": {"type": "integer"}}),
            T.shadow_why),
    ToolDef("shadow_checkpoint",
            "Show the last checkpoint (task_id + number of recorded file changes). Read-only.",
            _schema({"workspace": _ws_prop()}),
            T.shadow_checkpoint),
    ToolDef("shadow_rollback",
            "Restore the last checkpoint (destructive). Requires confirm=true.",
            _schema({"workspace": _ws_prop(), "confirm": {"type": "boolean"}}, []),
            T.shadow_rollback,
            destructive=True),
    ToolDef("shadow_review",
            "Review the pending working-tree diff (status + diff + staged). Read-only.",
            _schema({"workspace": _ws_prop()}),
            T.shadow_review),
    ToolDef("shadow_test",
            "Run a test command in the workspace and return stdout/stderr/exit_code. Auto-detects pytest/npm/cargo/go when no command is given.",
            _schema({"workspace": _ws_prop(),
                    "command": {"type": "string"},
                    "timeout": {"type": "integer"}},
            []),
            T.shadow_test),
    ToolDef("shadow_models",
            "List available/configured/detected models and the active default + provider. Reuses ModelRegistry.",
            _schema({"workspace": _ws_prop(), "detect": {"type": "boolean"}},
            []),
            T.shadow_models),
    ToolDef("shadow_memory",
            "Read or append to ShadowCode project/task memory (.shadow/memory/). action=read|append; scope=project|task.",
            _schema({"workspace": _ws_prop(),
                    "action": {"type": "string", "enum": ["read", "append"]},
                    "scope": {"type": "string", "enum": ["project", "task"]},
                    "note": {"type": "string"},
                    "task_id": {"type": "string"}},
            ["action"]),
            T.shadow_memory),
    ToolDef("shadow_tools",
            "List the harness tools installed for the workspace (name, read_only, destructive).",
            _schema({"workspace": _ws_prop()}),
            T.shadow_tools),
    ToolDef("shadow_approve",
            "Approve or deny a pending destructive action surfaced by another tool (approval_id + decision=approve|deny). Required to actually execute destructive sub-actions from shadow_run.",
            _schema({"approval_id": {"type": "string"},
                    "decision": {"type": "string", "enum": ["approve", "deny"]}},
            ["approval_id", "decision"]),
            T.shadow_approve),
    ToolDef("shadow_sessions",
            "List recent ShadowCode sessions (id, workspace, status, title, model_id).",
            _schema({"limit": {"type": "integer"}},
            []),
            T.shadow_sessions),
]


def tool_by_name(name: str) -> ToolDef | None:
    for td in TOOL_DEFS:
        if td.name == name:
            return td
    return None


__all__ = ["ToolDef", "TOOL_DEFS", "tool_by_name"]
