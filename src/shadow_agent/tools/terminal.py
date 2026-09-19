from __future__ import annotations

import json
import os
import signal
import subprocess
import threading

from shadow_agent.models.types import ToolCall, ToolResult
from shadow_agent.permissions import PermissionGate
from shadow_agent.tools.sandbox import WorkspaceSandbox

_running: dict[int, subprocess.Popen[str]] = {}
_lock = threading.Lock()


def register_terminal_tools(
    registry: object,
    sandbox: WorkspaceSandbox,
    gate: PermissionGate,
    timeout_sec: int = 60,
) -> None:
    from shadow_agent.tools.registry import ToolRegistry

    assert isinstance(registry, ToolRegistry)
    registry.add(
        "exec",
        "Run a shell command in the workspace. Returns stdout, stderr, and exit code.",
        {
            "type": "object",
            "properties": {
                "command": {"type": "string"},
                "timeout": {"type": "integer"},
            },
            "required": ["command"],
        },
        lambda call: exec_command(sandbox, gate, call, default_timeout=timeout_sec),
    )
    registry.add(
        "kill",
        "Kill a process previously started by exec, by pid.",
        {"type": "object", "properties": {"pid": {"type": "integer"}}, "required": ["pid"]},
        lambda call: kill_process(call),
    )


def exec_command(
    sandbox: WorkspaceSandbox,
    gate: PermissionGate,
    call: ToolCall,
    default_timeout: int = 60,
) -> ToolResult:
    command = str(call.arguments.get("command") or "").strip()
    if not command:
        return ToolResult(id=call.id, success=False, error="command is required")
    decision = gate.check_command(command)
    if not decision.allowed:
        return ToolResult(
            id=call.id,
            success=False,
            error=decision.reason,
            metadata={"needs_approval": decision.needs_approval, "elevated": decision.elevated},
        )
    timeout = int(call.arguments.get("timeout") or default_timeout)
    env = _safe_env()
    try:
        proc = subprocess.Popen(
            command,
            shell=True,
            cwd=str(sandbox.root),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            env=env,
            start_new_session=True,
        )
    except OSError as exc:
        return ToolResult(id=call.id, success=False, error=str(exc))
    with _lock:
        _running[proc.pid] = proc
    try:
        stdout, stderr = proc.communicate(timeout=timeout)
        code = proc.returncode if proc.returncode is not None else -1
        payload = {
            "command": command,
            "stdout": stdout,
            "stderr": stderr,
            "exit_code": code,
            "cwd": str(sandbox.root),
        }
        return ToolResult(
            id=call.id,
            success=code == 0,
            output=json.dumps(payload),
            error="" if code == 0 else (stderr or f"exit {code}"),
            metadata={"exit_code": code, "pid": proc.pid, "command": command},
        )
    except subprocess.TimeoutExpired:
        _kill_pgid(proc)
        stdout, stderr = proc.communicate(timeout=2)
        return ToolResult(
            id=call.id,
            success=False,
            output=json.dumps({"command": command, "stdout": stdout, "stderr": stderr, "exit_code": -1, "timeout": True}),
            error=f"timed out after {timeout}s",
            metadata={"timeout": True, "pid": proc.pid, "command": command},
        )
    finally:
        with _lock:
            _running.pop(proc.pid, None)


def kill_process(call: ToolCall) -> ToolResult:
    pid = int(call.arguments.get("pid") or 0)
    with _lock:
        proc = _running.get(pid)
    if proc is None:
        return ToolResult(id=call.id, success=False, error=f"no tracked process {pid}")
    _kill_pgid(proc)
    return ToolResult(id=call.id, success=True, output=json.dumps({"killed": pid}))


def _kill_pgid(proc: subprocess.Popen[str]) -> None:
    try:
        os.killpg(proc.pid, signal.SIGKILL)
    except ProcessLookupError:
        proc.kill()


def _safe_env() -> dict[str, str]:
    keep = {
        "PATH",
        "HOME",
        "USER",
        "LANG",
        "LC_ALL",
        "TERM",
        "TMPDIR",
        "VIRTUAL_ENV",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
    }
    env = {key: value for key, value in os.environ.items() if key in keep or key.startswith("SHADOW_")}
    env.setdefault("PATH", "/usr/bin:/bin")
    env["PYTHONDONTWRITEBYTECODE"] = "1"
    return env
