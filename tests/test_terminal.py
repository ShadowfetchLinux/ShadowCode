from __future__ import annotations

from shadow_agent.config import PermissionLevel
from shadow_agent.models.types import ToolCall
from shadow_agent.permissions import PermissionGate
from shadow_agent.tools.sandbox import WorkspaceSandbox
from shadow_agent.tools.terminal import exec_command


def test_exec_captures_output(workspace):
    gate = PermissionGate(PermissionLevel.WORKSPACE)
    result = exec_command(
        WorkspaceSandbox(workspace),
        gate,
        ToolCall(id="1", tool_name="exec", arguments={"command": "echo hello-agent"}),
    )
    assert result.success
    assert "hello-agent" in result.output
    assert result.metadata["exit_code"] == 0


def test_timeout_kills(workspace):
    gate = PermissionGate(PermissionLevel.WORKSPACE)
    result = exec_command(
        WorkspaceSandbox(workspace),
        gate,
        ToolCall(id="1", tool_name="exec", arguments={"command": "sleep 5", "timeout": 1}),
        default_timeout=1,
    )
    assert not result.success
    assert "timed out" in result.error
