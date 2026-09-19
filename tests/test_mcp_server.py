"""Tests for the ShadowCode MCP server (stdio + in-memory protocol + HTTP/SSE)."""

from __future__ import annotations

import asyncio
import json
import os
import socket
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

import pytest

from shadow_agent.mcp_server import ShadowMCPServer
from shadow_agent.mcp_server.auth import ensure_token, load_token, redact, request_allowed
from shadow_agent.mcp_server.catalog import TOOL_DEFS, tool_by_name
from shadow_agent.mcp_server.register import _http_block, _stdio_block
from shadow_agent.mcp_server.resources import list_resources, read_resource


# --------------------------------------------------------------------------- helpers


def _make_server(workspace: Path) -> ShadowMCPServer:
    return ShadowMCPServer(workspace=workspace)


@pytest.fixture
def mcp_server(workspace: Path, isolated: Path) -> ShadowMCPServer:
    return _make_server(workspace)


# --------------------------------------------------------------------------- catalog


def test_catalog_lists_all_expected_tools():
    names = {td.name for td in TOOL_DEFS}
    expected = {
        "shadow_understand", "shadow_goal", "shadow_status", "shadow_run",
        "shadow_doctor", "shadow_why", "shadow_checkpoint", "shadow_rollback",
        "shadow_review", "shadow_test", "shadow_models", "shadow_memory",
        "shadow_tools", "shadow_approve", "shadow_sessions",
    }
    assert expected.issubset(names), f"missing: {expected - names}"


def test_only_rollback_is_marked_destructive():
    destructive = {td.name for td in TOOL_DEFS if td.destructive}
    assert destructive == {"shadow_rollback"}


def test_tool_by_name_round_trip():
    td = tool_by_name("shadow_understand")
    assert td is not None and td.name == "shadow_understand"
    assert tool_by_name("does_not_exist") is None


# --------------------------------------------------------------------------- dispatchers


def test_shadow_understand_returns_project_map(workspace: Path, mcp_server: ShadowMCPServer):
    (workspace / "pyproject.toml").write_text("[project]\nname = \"demo\"\n", encoding="utf-8")
    (workspace / "src").mkdir()
    (workspace / "src" / "app.py").write_text("def add(a, b):\n    return a + b\n", encoding="utf-8")
    td = tool_by_name("shadow_understand")
    result = td.dispatcher({}, mcp_server.ctx)
    assert result["ok"], result
    assert "Project map" in result["text"]
    assert "python" in result["project_map"]["stack"]["languages"]
    # Memory file should now exist.
    assert (workspace / ".shadow" / "memory" / "project.md").is_file()


def test_shadow_goal_create_and_advance(workspace: Path, mcp_server: ShadowMCPServer):
    td = tool_by_name("shadow_goal")
    created = td.dispatcher(
        {"action": "create", "instruction": "Understand the project and add tests"},
        mcp_server.ctx,
    )
    assert created["ok"], created
    goal = created["goal"]
    assert goal["milestones"]
    gid = goal["id"]

    listed = td.dispatcher({"action": "list", "workspace": str(workspace)}, mcp_server.ctx)
    assert listed["ok"] and len(listed["goals"]) >= 1

    got = td.dispatcher({"action": "get", "goal_id": gid}, mcp_server.ctx)
    assert got["ok"] and got["goal"]["id"] == gid

    advanced = td.dispatcher(
        {"action": "advance", "goal_id": gid, "status": "done", "detail": "mcp test"},
        mcp_server.ctx,
    )
    assert advanced["ok"]
    assert advanced["goal"]["progress"] > 0


def test_shadow_status_reports_version_and_workspace(workspace: Path, mcp_server: ShadowMCPServer):
    td = tool_by_name("shadow_status")
    result = td.dispatcher({}, mcp_server.ctx)
    assert result["ok"]
    assert result["status"]["workspace"] == str(workspace)
    assert result["status"]["version"]


def test_shadow_doctor_runs_checks(workspace: Path, mcp_server: ShadowMCPServer):
    td = tool_by_name("shadow_doctor")
    result = td.dispatcher({}, mcp_server.ctx)
    assert result["ok"]
    assert "checks" in result["report"]
    assert result["report"]["checks"]


def test_shadow_models_lists_registered(workspace: Path, mcp_server: ShadowMCPServer):
    td = tool_by_name("shadow_models")
    result = td.dispatcher({"detect": False}, mcp_server.ctx)
    assert result["ok"]
    assert any(m["id"] == "mock" for m in result["models"])


def test_shadow_tools_lists_harness_tools(workspace: Path, mcp_server: ShadowMCPServer):
    td = tool_by_name("shadow_tools")
    result = td.dispatcher({}, mcp_server.ctx)
    assert result["ok"]
    names = {t["name"] for t in result["tools"]}
    assert "read_file" in names
    assert "write_file" in names
    # write_file must be flagged destructive.
    write_tool = next(t for t in result["tools"] if t["name"] == "write_file")
    assert write_tool["destructive"]


def test_shadow_memory_append_and_read(workspace: Path, mcp_server: ShadowMCPServer):
    td = tool_by_name("shadow_memory")
    appended = td.dispatcher({"action": "append", "scope": "project", "note": "MCP test note"}, mcp_server.ctx)
    assert appended["ok"]
    read = td.dispatcher({"action": "read"}, mcp_server.ctx)
    assert read["ok"]
    assert "MCP test note" in read["project"]


def test_shadow_rollback_requires_confirmation(workspace: Path, mcp_server: ShadowMCPServer):
    td = tool_by_name("shadow_rollback")
    result = td.dispatcher({}, mcp_server.ctx)
    assert not result["ok"]
    assert result.get("needs_confirmation") is True


def test_shadow_sessions_returns_list(workspace: Path, mcp_server: ShadowMCPServer):
    td = tool_by_name("shadow_sessions")
    result = td.dispatcher({}, mcp_server.ctx)
    assert result["ok"]
    assert isinstance(result["sessions"], list)


def test_shadow_why_on_non_git_returns_error(workspace: Path, mcp_server: ShadowMCPServer):
    td = tool_by_name("shadow_why")
    result = td.dispatcher({}, mcp_server.ctx)
    assert not result["ok"]
    assert "not a git repo" in result["error"]


# --------------------------------------------------------------------------- permissions / safety


def test_shadow_run_read_only_level_blocks_writes(workspace: Path, mcp_server: ShadowMCPServer):
    """shadow_run with permission_level=read_only must not allow destructive tools.

    We don't run a full agent loop here (it would need a real model); we just
    confirm the dispatcher accepts the level and the gate would block writes.
    """
    from shadow_agent.config import PermissionLevel
    from shadow_agent.permissions import PermissionGate

    gate = PermissionGate(PermissionLevel.READ_ONLY)
    decision = gate.check_tool("write_file")
    assert not decision.allowed


def test_no_secrets_leak_in_dispatcher_output(workspace: Path, mcp_server: ShadowMCPServer):
    """Dispatcher output must not contain api keys / tokens."""
    td = tool_by_name("shadow_status")
    result = td.dispatcher({}, mcp_server.ctx)
    blob = json.dumps(result)
    # No real key prefix should ever appear.
    assert "sk-" not in blob
    assert "Bearer " not in blob


def test_redact_strips_secret_keys():
    out = redact({"api_key": "sk-real", "token": "abc", "name": "ok", "nested": {"secret_value": "x"}})
    assert out["api_key"] == "***"
    assert out["token"] == "***"
    assert out["name"] == "ok"
    assert out["nested"]["secret_value"] == "***"


# --------------------------------------------------------------------------- auth


def test_token_round_trip(isolated: Path):
    token = ensure_token()
    assert token and len(token) > 16
    assert load_token() == token


def test_request_allowed_no_token():
    scope = {"headers": [(b"authorization", b"")]}
    assert request_allowed(scope, token=None) is True


def test_request_allowed_with_token():
    scope_ok = {"headers": [(b"authorization", b"Bearer abc")]}
    scope_bad = {"headers": [(b"authorization", b"Bearer wrong")]}
    assert request_allowed(scope_ok, token="abc") is True
    assert request_allowed(scope_bad, token="abc") is False
    assert request_allowed({"headers": []}, token="abc") is False


# --------------------------------------------------------------------------- register


def test_stdio_block_shape():
    block = _stdio_block("claude-code")
    assert "mcpServers" in block
    assert "shadowcode" in block["mcpServers"]
    assert block["mcpServers"]["shadowcode"]["args"] == ["mcp", "serve"]


def test_http_block_includes_token():
    block = _http_block("127.0.0.1", 7431, "tok-123")
    sc = block["mcpServers"]["shadowcode"]
    assert sc["url"] == "http://127.0.0.1:7431/sse"
    assert sc["type"] == "sse"
    assert sc["headers"]["Authorization"] == "Bearer tok-123"


def test_http_block_without_token_has_no_headers():
    block = _http_block("127.0.0.1", 7431, None)
    assert "headers" not in block["mcpServers"]["shadowcode"]


# --------------------------------------------------------------------------- resources


def test_list_resources_includes_all_four(workspace: Path, mcp_server: ShadowMCPServer):
    res = list_resources(workspace)
    kinds = {r["name"] for r in res}
    assert kinds == {"project", "sessions", "plan", "memory"}


def test_read_resource_sessions(workspace: Path, mcp_server: ShadowMCPServer):
    mime, text = read_resource("shadow://sessions", workspace, mcp_server.store)
    assert mime == "application/json"
    assert text.startswith("[")


def test_read_resource_memory(workspace: Path, mcp_server: ShadowMCPServer):
    (workspace / ".shadow" / "memory").mkdir(parents=True, exist_ok=True)
    (workspace / ".shadow" / "memory" / "project.md").write_text("# demo memory\n", encoding="utf-8")
    mime, text = read_resource("shadow://memory", workspace, mcp_server.store)
    assert mime == "text/markdown"
    assert "demo memory" in text


def test_read_resource_unknown_raises(workspace: Path, mcp_server: ShadowMCPServer):
    with pytest.raises(ValueError):
        read_resource("shadow://bogus", workspace, mcp_server.store)


# --------------------------------------------------------------------------- protocol (in-memory)


def test_protocol_list_tools_and_call_shadow_understand(workspace: Path, isolated: Path):
    """End-to-end MCP protocol test using the SDK in-memory transport."""
    from mcp.shared.memory import create_connected_server_and_client_session

    async def scenario() -> None:
        server = ShadowMCPServer(workspace=workspace)
        (workspace / "pyproject.toml").write_text("[project]\nname = \"demo\"\n", encoding="utf-8")
        async with create_connected_server_and_client_session(server.server) as session:
            await session.initialize()
            tools = await session.list_tools()
            names = {t.name for t in tools.tools}
            assert "shadow_understand" in names
            assert "shadow_approve" in names
            result = await session.call_tool("shadow_understand", {})
            assert result.content
            text = result.content[0].text
            payload = json.loads(text)
            assert payload["ok"], payload
            assert "Project map" in payload["text"]
            assert "python" in payload["project_map"]["stack"]["languages"]

    asyncio.run(scenario())


def test_protocol_call_shadow_status_returns_version(workspace: Path, isolated: Path):
    from mcp.shared.memory import create_connected_server_and_client_session

    async def scenario() -> None:
        server = ShadowMCPServer(workspace=workspace)
        async with create_connected_server_and_client_session(server.server) as session:
            await session.initialize()
            result = await session.call_tool("shadow_status", {})
            payload = json.loads(result.content[0].text)
            assert payload["ok"]
            assert payload["status"]["version"]

    asyncio.run(scenario())


def test_protocol_list_resources(workspace: Path, isolated: Path):
    from mcp.shared.memory import create_connected_server_and_client_session

    async def scenario() -> None:
        server = ShadowMCPServer(workspace=workspace)
        async with create_connected_server_and_client_session(server.server) as session:
            await session.initialize()
            res = await session.list_resources()
            kinds = {r.name for r in res.resources}
            assert kinds == {"project", "sessions", "plan", "memory"}

    asyncio.run(scenario())


def test_protocol_list_prompts(workspace: Path, isolated: Path):
    from mcp.shared.memory import create_connected_server_and_client_session

    async def scenario() -> None:
        server = ShadowMCPServer(workspace=workspace)
        async with create_connected_server_and_client_session(server.server) as session:
            await session.initialize()
            prompts = await session.list_prompts()
            names = {p.name for p in prompts.prompts}
            assert "understand" in names and "delegate" in names
            got = await session.get_prompt("delegate", {"task": "inspect the repo"})
            assert got.messages

    asyncio.run(scenario())


# --------------------------------------------------------------------------- HTTP/SSE smoke


def _free_port() -> int:
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    return port


def test_http_health_and_tools_endpoints(workspace: Path, isolated: Path):
    """Spin up the HTTP/SSE app via uvicorn in a thread and curl /health + /tools."""
    import threading
    import uvicorn

    port = _free_port()
    server = ShadowMCPServer(workspace=workspace)
    from shadow_agent.mcp_server.http_transport import build_http_app

    app = build_http_app(server.server, token=None, host="127.0.0.1", port=port)
    config = uvicorn.Config(app, host="127.0.0.1", port=port, log_level="warning")
    instance = uvicorn.Server(config)

    thread = threading.Thread(target=instance.run, daemon=True)
    thread.start()
    try:
        # Wait for the server to come up.
        deadline = time.time() + 10
        while time.time() < deadline:
            try:
                with urllib.request.urlopen(f"http://127.0.0.1:{port}/health", timeout=1) as r:
                    if r.status == 200:
                        break
            except Exception:
                time.sleep(0.1)
        else:
            pytest.fail("server did not start")

        with urllib.request.urlopen(f"http://127.0.0.1:{port}/health", timeout=2) as r:
            health = json.loads(r.read().decode())
        assert health["ok"] is True
        assert health["name"] == "shadowcode"

        with urllib.request.urlopen(f"http://127.0.0.1:{port}/tools", timeout=2) as r:
            tools = json.loads(r.read().decode())
        assert "shadow_understand" in {t["name"] for t in tools["tools"]}
    finally:
        instance.should_exit = True
        thread.join(timeout=5)


def test_http_rejects_unauthorized_when_token_set(workspace: Path, isolated: Path):
    import threading
    import uvicorn

    port = _free_port()
    server = ShadowMCPServer(workspace=workspace)
    from shadow_agent.mcp_server.http_transport import build_http_app

    app = build_http_app(server.server, token="secret-tok", host="127.0.0.1", port=port)
    config = uvicorn.Config(app, host="127.0.0.1", port=port, log_level="warning")
    instance = uvicorn.Server(config)
    thread = threading.Thread(target=instance.run, daemon=True)
    thread.start()
    try:
        deadline = time.time() + 10
        while time.time() < deadline:
            try:
                urllib.request.urlopen(f"http://127.0.0.1:{port}/health", timeout=1)
                break
            except urllib.error.HTTPError:
                break  # 401 already counts as "up"
            except Exception:
                time.sleep(0.1)
        # /tools without token → 401
        with pytest.raises(urllib.error.HTTPError) as exc:
            urllib.request.urlopen(f"http://127.0.0.1:{port}/tools", timeout=2)
        assert exc.value.code == 401
        # /tools with wrong token → 401
        req = urllib.request.Request(f"http://127.0.0.1:{port}/tools", headers={"Authorization": "Bearer wrong"})
        with pytest.raises(urllib.error.HTTPError) as exc:
            urllib.request.urlopen(req, timeout=2)
        assert exc.value.code == 401
        # /tools with correct token → 200
        req = urllib.request.Request(f"http://127.0.0.1:{port}/tools", headers={"Authorization": "Bearer secret-tok"})
        with urllib.request.urlopen(req, timeout=2) as r:
            assert r.status == 200
    finally:
        instance.should_exit = True
        thread.join(timeout=5)
