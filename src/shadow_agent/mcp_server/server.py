"""ShadowCode MCP server — stdio + HTTP/SSE transports.

Uses the official `mcp` Python SDK. Tools/resources/prompts are wired to the
existing harness functions (see `tools.py`, `resources.py`, `catalog.py`).
The model is replaceable — `shadow_run` constructs the same `AgentRunner` the
CLI/UI use, so any registered provider works.

Safety:
  - Loopback only by default (127.0.0.1).
  - Optional bearer token in ~/.config/shadow-agent/mcp-token.
  - Destructive tool calls route through ApprovalHub; the calling agent must
    resolve them via `shadow_approve`. We never auto-run destructive commands.
"""

from __future__ import annotations

import asyncio
import json
import os
import sys
import threading
from pathlib import Path
from typing import Any

from mcp import types
from mcp.server import Server
from mcp.server.stdio import stdio_server

from shadow_agent import __version__
from shadow_agent.approvals import ApprovalHub
from shadow_agent.config import ensure_user_config, load_config
from shadow_agent.events import EventBus
from shadow_agent.mcp_server.auth import load_token, redact, request_allowed
from shadow_agent.mcp_server.catalog import TOOL_DEFS, tool_by_name
from shadow_agent.mcp_server.resources import list_resources, read_resource
from shadow_agent.mcp_server.tools import ToolContext
from shadow_agent.runtime import JobManager
from shadow_agent.secrets import load_secrets
from shadow_agent.store import Store


class ShadowMCPServer:
    """The ShadowCode MCP server: tool/resource/prompt surface + dispatch."""

    def __init__(self, workspace: Path | None = None) -> None:
        load_secrets()
        self.workspace = (workspace or Path.cwd()).resolve()
        # Build the shared harness context once.
        cfg = load_config(self.workspace)
        self.store = Store()
        self.approvals = ApprovalHub(timeout_sec=300.0)
        self.jobs = JobManager(self.store, EventBus(), self.approvals)
        self.ctx = ToolContext(
            store=self.store, approvals=self.approvals, jobs=self.jobs,
            config=cfg, workspace=self.workspace,
        )
        self.server = Server("shadowcode")
        self._register_handlers()

    # -- handlers -----------------------------------------------------------

    def _register_handlers(self) -> None:
        server = self.server

        @server.list_tools()
        async def _list_tools() -> list[types.Tool]:
            return [
                types.Tool(
                    name=td.name,
                    description=td.description,
                    inputSchema=td.input_schema,
                    annotations=types.ToolAnnotations(
                        destructiveHint=td.destructive,
                        idempotentHint=False,
                        openWorldHint=False,
                        readOnlyHint=not td.destructive,
                    ),
                )
                for td in TOOL_DEFS
            ]

        @server.call_tool()
        async def _call_tool(name: str, arguments: dict[str, Any] | None) -> list[types.TextContent]:
            args = arguments or {}
            td = tool_by_name(name)
            if td is None:
                return [types.TextContent(type="text", text=json.dumps({"ok": False, "error": f"unknown tool: {name}"}))]
            # Run the sync dispatcher in a worker thread so we don't block the
            # async event loop. The harness is fully sync.
            loop = asyncio.get_running_loop()
            try:
                result = await loop.run_in_executor(None, td.dispatcher, args, self.ctx)
            except Exception as exc:  # noqa: BLE001 - never crash the server over one tool
                result = {"ok": False, "error": f"{exc.__class__.__name__}: {exc}"}
            # Redact secret-looking keys before sending anything back.
            result = redact(result)
            return [types.TextContent(type="text", text=json.dumps(result, default=str, indent=2))]

        @server.list_resources()
        async def _list_resources() -> list[types.Resource]:
            return [
                types.Resource(**res) for res in list_resources(self.workspace)
            ]

        @server.read_resource()
        async def _read_resource(uri: Any) -> str:
            uri_str = str(uri)
            try:
                mime, text = read_resource(uri_str, self.workspace, self.store)
            except Exception as exc:  # noqa: BLE001
                return f"error: {exc}"
            return text

        @server.list_prompts()
        async def _list_prompts() -> list[types.Prompt]:
            return [
                types.Prompt(
                    name="understand",
                    description="Inspect a project and return a project map. Pass {workspace}.",
                    arguments=[types.PromptArgument(name="workspace", description="Absolute project path", required=False)],
                ),
                types.Prompt(
                    name="delegate",
                    description="Delegate a task to ShadowCode. Pass {task} and optional {workspace}.",
                    arguments=[
                        types.PromptArgument(name="task", description="Task for the agent loop", required=True),
                        types.PromptArgument(name="workspace", description="Absolute project path", required=False),
                    ],
                ),
            ]

        @server.get_prompt()
        async def _get_prompt(name: str, arguments: dict[str, Any] | None) -> types.GetPromptResult:
            args = arguments or {}
            ws = args.get("workspace") or str(self.workspace)
            if name == "understand":
                user_msg = f"Ask ShadowCode to inspect the project at {ws} and return a project map."
                return types.GetPromptResult(
                    description="Delegate /understand to ShadowCode",
                    messages=[
                        types.PromptMessage(
                            role="user",
                            content=types.TextContent(type="text", text=user_msg),
                        )
                    ],
                )
            if name == "delegate":
                task = args.get("task", "")
                user_msg = f"Ask ShadowCode to run the following task in the project at {ws}:\n{task}"
                return types.GetPromptResult(
                    description="Delegate a task to ShadowCode",
                    messages=[
                        types.PromptMessage(
                            role="user",
                            content=types.TextContent(type="text", text=user_msg),
                        )
                    ],
                )
            return types.GetPromptResult(
                description=f"unknown prompt: {name}",
                messages=[types.PromptMessage(role="user", content=types.TextContent(type="text", text=f"unknown prompt: {name}"))],
            )

    # -- transports ---------------------------------------------------------

    async def run_stdio(self) -> None:
        """Run the server over stdio (for `shadow mcp serve`)."""
        async with stdio_server() as (read, write):
            await self.server.run(
                read, write,
                self.server.create_initialization_options(
                    notification_options=None,
                ),
            )

    def serve_stdio_blocking(self) -> None:
        asyncio.run(self.run_stdio())

    def serve_http_blocking(self, host: str = "127.0.0.1", port: int = 7431) -> None:
        """Run the server over HTTP/SSE on (host, port). Loopback-only by default."""
        # We construct the ASGI app lazily so the SSE import stays optional.
        from shadow_agent.mcp_server.http_transport import build_http_app

        token = load_token()
        app = build_http_app(self.server, token, host, port)
        # uvicorn is a hard dependency of the harness (FastAPI), so this works.
        import uvicorn

        print(f"ShadowCode MCP server (SSE) → http://{host}:{port}/sse", file=sys.stderr)
        if token:
            print(f"  auth: bearer token required (file: ~/.config/shadow-agent/mcp-token)", file=sys.stderr)
        else:
            print("  auth: no token configured — anyone on loopback can call", file=sys.stderr)
        uvicorn.run(app, host=host, port=port, log_level="warning")


def serve_stdio(workspace: Path | None = None) -> None:
    server = ShadowMCPServer(workspace=workspace)
    server.serve_stdio_blocking()


def serve_http(host: str = "127.0.0.1", port: int = 7431, workspace: Path | None = None) -> None:
    server = ShadowMCPServer(workspace=workspace)
    server.serve_http_blocking(host=host, port=port)


__all__ = ["ShadowMCPServer", "serve_stdio", "serve_http"]
