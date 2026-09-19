from __future__ import annotations

from shadow_agent.config import PermissionLevel
from shadow_agent.models.types import ToolCall
from shadow_agent.permissions import PermissionGate
from shadow_agent.tools.registry import default_tools
from shadow_agent.tools.sandbox import WorkspaceSandbox


def test_read_only_blocks_writes(workspace):
    gate = PermissionGate(PermissionLevel.READ_ONLY)
    tools = default_tools(WorkspaceSandbox(workspace), gate)
    result = tools.execute(ToolCall(id="1", tool_name="write_file", arguments={"path": "x", "content": "y"}), gate)
    assert not result.success
    assert "read-only" in result.error


def test_dangerous_and_network_blocked(workspace):
    gate = PermissionGate(PermissionLevel.WORKSPACE, network=False)
    tools = default_tools(WorkspaceSandbox(workspace), gate)
    rm = tools.execute(ToolCall(id="1", tool_name="exec", arguments={"command": "rm -rf /"}), gate)
    assert not rm.success
    curl = tools.execute(ToolCall(id="2", tool_name="exec", arguments={"command": "curl https://example.com"}), gate)
    assert not curl.success
    sudo = tools.execute(ToolCall(id="3", tool_name="exec", arguments={"command": "sudo ls"}), gate)
    assert not sudo.success


def test_workspace_allows_echo(workspace):
    gate = PermissionGate(PermissionLevel.WORKSPACE)
    tools = default_tools(WorkspaceSandbox(workspace), gate)
    result = tools.execute(ToolCall(id="1", tool_name="exec", arguments={"command": "echo ok"}), gate)
    assert result.success
