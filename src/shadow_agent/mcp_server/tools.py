"""MCP tool dispatchers for ShadowCode's flagship capabilities.

Each dispatcher maps to an *existing* harness function — no logic is
duplicated. Destructive calls route through the in-process ApprovalHub so
the calling agent must explicitly approve them via ``shadow_approve``.
"""

from __future__ import annotations

import json
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable

from shadow_agent.approvals import ApprovalHub
from shadow_agent.config import AppConfig, PermissionLevel, load_config
from shadow_agent.context.memory import MemoryStore
from shadow_agent.health import collect_health, doctor_report
from shadow_agent.models.registry import ModelRegistry
from shadow_agent.models.types import ToolCall
from shadow_agent.permissions import PermissionGate, WRITE_TOOLS
from shadow_agent.runtime import JobManager
from shadow_agent.store import Store
from shadow_agent.tools.registry import default_tools
from shadow_agent.tools.sandbox import WorkspaceSandbox

DESTRUCTIVE_TOOLS = {
    "write_file", "edit_file", "apply_patch", "delete_file", "move_file",
    "exec", "git_add", "git_commit", "git_checkout", "git_reset", "git_clean", "kill",
}


@dataclass
class ToolContext:
    store: Store
    approvals: ApprovalHub
    jobs: JobManager
    config: AppConfig
    workspace: Path

    def reload_config(self, workspace: Path) -> AppConfig:
        self.workspace = Path(workspace).resolve()
        self.config = load_config(self.workspace)
        return self.config


def _ok(text: str, **extra: Any) -> dict[str, Any]:
    payload: dict[str, Any] = {"ok": True, "text": text}
    payload.update(extra)
    return payload


def _err(text: str, **extra: Any) -> dict[str, Any]:
    payload: dict[str, Any] = {"ok": False, "error": text}
    payload.update(extra)
    return payload


def _workspace_arg(args: dict[str, Any], ctx: ToolContext) -> Path:
    raw = args.get("workspace")
    if raw:
        return Path(str(raw)).expanduser().resolve()
    return ctx.workspace


def shadow_understand(args: dict[str, Any], ctx: ToolContext) -> dict[str, Any]:
    from shadow_agent.understand import render_map, save_project_map

    workspace = _workspace_arg(args, ctx)
    if not workspace.is_dir():
        return _err(f"workspace not found: {workspace}")
    mp = save_project_map(workspace, task_id=str(args.get("task_id") or "mcp-understand"))
    return _ok(render_map(mp), project_map=mp,
               memory_file=str(workspace / ".shadow" / "memory" / "project.md"))


def shadow_goal(args: dict[str, Any], ctx: ToolContext) -> dict[str, Any]:
    from shadow_agent.goal import GoalStore

    workspace = _workspace_arg(args, ctx)
    action = str(args.get("action") or "create").lower()
    store = GoalStore()
    try:
        if action == "create":
            instruction = str(args.get("instruction") or "").strip()
            if not instruction:
                return _err("instruction is required for action=create")
            goal = store.create_goal(workspace, instruction)
            return _ok(f"Goal {goal['id'][:8]} created with {len(goal['milestones'])} milestones.", goal=goal)
        if action == "list":
            goals = store.list_goals(workspace)
            return _ok(f"{len(goals)} goal(s).", goals=goals)
        if action == "get":
            gid = str(args.get("goal_id") or "").strip()
            if not gid:
                return _err("goal_id is required for action=get")
            goal = store.get_goal(gid)
            if not goal:
                return _err("goal not found")
            return _ok(f"Goal {gid[:8]} — {int(goal['progress'] * 100)}% complete.", goal=goal)
        if action == "advance":
            gid = str(args.get("goal_id") or "").strip()
            if not gid:
                return _err("goal_id is required for action=advance")
            status = str(args.get("status") or "done")
            detail = str(args.get("detail") or "")
            goal = store.update_milestone(gid, args.get("milestone_id"), status, detail=detail)
            if not goal:
                return _err("goal or milestone not found")
            return _ok(f"Goal {gid[:8]} now {int(goal['progress'] * 100)}% complete.", goal=goal)
        if action == "abandon":
            gid = str(args.get("goal_id") or "").strip()
            if not gid:
                return _err("goal_id is required for action=abandon")
            store.abandon(gid)
            return _ok(f"Goal {gid[:8]} abandoned.")
        return _err(f"unknown action: {action}")
    finally:
        store.close()


def shadow_status(args: dict[str, Any], ctx: ToolContext) -> dict[str, Any]:
    workspace = _workspace_arg(args, ctx)
    cfg = load_config(workspace)
    health = collect_health(cfg, workspace)
    active = ctx.jobs.list_active()
    payload = {
        "version": health.get("version"),
        "workspace": str(workspace),
        "provider": health.get("provider"),
        "model": health.get("model"),
        "permissions": cfg.permissions.model_dump(mode="json"),
        "active_jobs": [job.to_dict() for job in active],
    }
    return _ok(f"ShadowCode {health.get('version')} — {len(active)} active job(s).", status=payload)


def shadow_run(args: dict[str, Any], ctx: ToolContext) -> dict[str, Any]:
    from shadow_agent.agent.loop import AgentRunner
    from shadow_agent.events import EventBus

    task = str(args.get("task") or "").strip()
    if not task:
        return _err("task is required")
    workspace = _workspace_arg(args, ctx)
    if not workspace.is_dir():
        return _err(f"workspace not found: {workspace}")
    cfg = load_config(workspace)
    level = str(args.get("permission_level") or cfg.permissions.level.value)
    try:
        cfg.permissions.level = PermissionLevel(level)
    except ValueError as exc:
        return _err(f"bad permission_level: {exc}")
    cfg.permissions.require_approval_for_dangerous = True
    runner = AgentRunner(
        workspace, config=cfg, store=ctx.store, events=EventBus(),
        approval_hub=ctx.approvals,
        model_override=args.get("model") or None,
        purpose=str(args.get("purpose") or "coder"),
    )
    result = runner.run(task)
    return _ok(result.summary, result=result.model_dump(mode="json"))


def shadow_doctor(args: dict[str, Any], ctx: ToolContext) -> dict[str, Any]:
    workspace = _workspace_arg(args, ctx)
    cfg = load_config(workspace)
    report = doctor_report(cfg, workspace)
    summary = f"{sum(1 for c in report['checks'] if c['ok'])}/{len(report['checks'])} checks ok."
    return _ok(summary, report=report)


def shadow_why(args: dict[str, Any], ctx: ToolContext) -> dict[str, Any]:
    workspace = _workspace_arg(args, ctx)
    path = str(args.get("path") or "")
    count = int(args.get("count") or 8)
    if not (workspace / ".git").is_dir():
        return _err("not a git repo")
    cmd = ["git", "log", f"-{count}", "--oneline", "--decorate"]
    if path:
        cmd += ["--", path]
    log = subprocess.run(cmd, cwd=workspace, capture_output=True, text=True, check=False).stdout
    diff_cmd = ["git", "diff", "--", path] if path else ["git", "diff"]
    diff = subprocess.run(diff_cmd, cwd=workspace, capture_output=True, text=True, check=False).stdout
    return _ok(f"Last {count} commits touching {path or '(workspace)'}:\n{log}", log=log, diff=diff)


def shadow_checkpoint(args: dict[str, Any], ctx: ToolContext) -> dict[str, Any]:
    from shadow_agent.checkpoints import last_checkpoint

    workspace = _workspace_arg(args, ctx)
    pointer = last_checkpoint(workspace)
    if not pointer:
        return _ok("No checkpoint exists for this project yet.", pointer=None)
    return _ok(f"Last checkpoint: task {pointer.get('task_id', '')[:8]} with {pointer.get('changes', 0)} change(s).", pointer=pointer)


def shadow_rollback(args: dict[str, Any], ctx: ToolContext) -> dict[str, Any]:
    from shadow_agent.checkpoints import restore_last

    workspace = _workspace_arg(args, ctx)
    if not args.get("confirm"):
        return _err("rollback is destructive — pass confirm=true to restore the last checkpoint.", needs_confirmation=True)
    result = restore_last(workspace)
    return _ok(f"Restored {len(result.get('restored', []))} file(s).", result=result)


def shadow_review(args: dict[str, Any], ctx: ToolContext) -> dict[str, Any]:
    workspace = _workspace_arg(args, ctx)
    if not (workspace / ".git").is_dir():
        return _err("not a git repo")
    porcelain = subprocess.run(["git", "status", "--porcelain=v1", "-b"], cwd=workspace, capture_output=True, text=True, check=False).stdout
    diff = subprocess.run(["git", "diff"], cwd=workspace, capture_output=True, text=True, check=False).stdout
    staged = subprocess.run(["git", "diff", "--cached"], cwd=workspace, capture_output=True, text=True, check=False).stdout
    files = [line[3:] for line in porcelain.splitlines() if not line.startswith("##") and len(line) >= 3]
    return _ok(f"{len(files)} pending file(s): {', '.join(files[:8])}", files=files, diff=diff, staged=staged)


def shadow_test(args: dict[str, Any], ctx: ToolContext) -> dict[str, Any]:
    workspace = _workspace_arg(args, ctx)
    cfg = load_config(workspace)
    command = str(args.get("command") or "").strip()
    if not command:
        if (workspace / "pyproject.toml").is_file() or (workspace / "pytest.ini").is_file():
            command = "python3 -m pytest -q"
        elif (workspace / "package.json").is_file():
            command = "npm test"
        elif (workspace / "Cargo.toml").is_file():
            command = "cargo test"
        elif (workspace / "go.mod").is_file():
            command = "go test ./..."
        else:
            return _err("no test command given and none auto-detected")
    sandbox = WorkspaceSandbox(workspace)
    gate = PermissionGate(cfg.permissions.level, require_approval_for_dangerous=True,
                          network=cfg.permissions.network, allow_root=False, auto_approve=False)
    from shadow_agent.tools.terminal import exec_command
    call = ToolCall(id="mcp-test", tool_name="exec",
                    arguments={"command": command, "timeout": int(args.get("timeout") or 300)})
    result = exec_command(sandbox, gate, call, default_timeout=300)
    try:
        parsed = json.loads(result.output) if result.output else {}
    except json.JSONDecodeError:
        parsed = {"stdout": result.output}
    return _ok(f"test command: {command}  exit={parsed.get('exit_code', -1)}",
               command=command, success=result.success,
               stdout=parsed.get("stdout", ""), stderr=parsed.get("stderr", ""),
               exit_code=parsed.get("exit_code", -1))


def shadow_models(args: dict[str, Any], ctx: ToolContext) -> dict[str, Any]:
    detect = bool(args.get("detect", True))
    registry = ModelRegistry(detect=detect)
    cfg = load_config(ctx.workspace)
    models = [
        {"id": info.id, "name": info.name, "provider": info.provider,
         "endpoint": info.endpoint, "context_limit": info.context_limit,
         "default": info.id == cfg.model.default}
        for info in registry.list_models()
    ]
    return _ok(f"{len(models)} model(s). default={cfg.model.default} provider={cfg.model.provider}",
               models=models, default=cfg.model.default, provider=cfg.model.provider,
               routing_enabled=cfg.routing.enabled)


def shadow_memory(args: dict[str, Any], ctx: ToolContext) -> dict[str, Any]:
    workspace = _workspace_arg(args, ctx)
    action = str(args.get("action") or "read").lower()
    memory = MemoryStore(workspace, task_id=str(args.get("task_id") or "mcp"))
    if action == "read":
        return _ok(memory.combined() or "(no project memory yet)",
                   project=memory.load_project(), task=memory.load_task())
    if action == "append":
        note = str(args.get("note") or "").strip()
        if not note:
            return _err("note is required for action=append")
        target = str(args.get("scope") or "project").lower()
        if target == "task":
            memory.append_task(note)
        else:
            memory.append_project(note)
        return _ok(f"appended {target} memory note.")
    return _err(f"unknown action: {action}")


def shadow_tools(args: dict[str, Any], ctx: ToolContext) -> dict[str, Any]:
    workspace = _workspace_arg(args, ctx)
    cfg = load_config(workspace)
    sandbox = WorkspaceSandbox(workspace)
    gate = PermissionGate(cfg.permissions.level)
    registry = default_tools(sandbox, gate, cfg.agent.tool_timeout_sec)
    tools = [
        {"name": name, "read_only": name not in WRITE_TOOLS, "destructive": name in DESTRUCTIVE_TOOLS}
        for name in registry.names()
    ]
    return _ok(f"{len(tools)} tool(s) installed.", tools=tools)


def shadow_approve(args: dict[str, Any], ctx: ToolContext) -> dict[str, Any]:
    approval_id = str(args.get("approval_id") or "").strip()
    decision = str(args.get("decision") or "deny").lower()
    if decision not in {"approve", "deny"}:
        return _err("decision must be approve or deny")
    if not approval_id:
        return _err("approval_id is required")
    try:
        result = ctx.approvals.decide(approval_id, decision)
    except KeyError as exc:
        return _err(f"unknown approval: {exc}")
    return _ok(f"approval {approval_id[:8]} → {decision}", approval=result)


def shadow_sessions(args: dict[str, Any], ctx: ToolContext) -> dict[str, Any]:
    rows = ctx.store.list_sessions(limit=int(args.get("limit") or 20))
    out = [
        {"id": row["id"], "workspace": row["workspace"], "status": row["status"],
         "title": row.get("title") or "", "model_id": row.get("model_id") or ""}
        for row in rows
    ]
    return _ok(f"{len(out)} session(s).", sessions=out)


__all__ = [
    "DESTRUCTIVE_TOOLS", "ToolContext",
    "shadow_understand", "shadow_goal", "shadow_status", "shadow_run",
    "shadow_doctor", "shadow_why", "shadow_checkpoint", "shadow_rollback",
    "shadow_review", "shadow_test", "shadow_models", "shadow_memory",
    "shadow_tools", "shadow_approve", "shadow_sessions",
]
