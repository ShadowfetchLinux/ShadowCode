"""ShadowCode as an MCP (Model Context Protocol) server.

Exposes ShadowCode's flagship harness capabilities as MCP tools, resources,
and prompts so other agents (Claude Code, Cursor, Codex, …) can delegate work
to ShadowCode over stdio or HTTP/SSE.

This module is an *addition* to the harness: it reuses the existing
0.4.0–0.8.0 harness functions (AgentRunner, GoalStore, understand,
checkpoints, health, ModelRegistry, MemoryStore, ToolRegistry) and never
duplicates their logic. The model is replaceable — the MCP server calls the
same Agent API the CLI/UI use.
"""

from __future__ import annotations

from shadow_agent.mcp_server.server import (
    ShadowMCPServer,
    serve_http,
    serve_stdio,
)
from shadow_agent.mcp_server.register import print_register_blocks

__all__ = [
    "ShadowMCPServer",
    "serve_http",
    "serve_stdio",
    "print_register_blocks",
]
