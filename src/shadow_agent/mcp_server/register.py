"""`shadow mcp register` — print JSON config blocks for MCP clients.

Prints ready-to-paste snippets for Claude Code, Cursor, and Codex (and a
generic stdio block). HTTP/SSE clients get the URL + token; stdio clients get
the command + env. Never prints the token inline for stdio (the wrapper
sources secrets.env already); HTTP/SSE clients need it in headers.
"""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path
from typing import Any

from shadow_agent import __version__
from shadow_agent.mcp_server.auth import ensure_token, load_token, token_file


def _shadow_bin() -> str:
    """Return the absolute path to the shadow wrapper."""
    candidate = Path.home() / ".local" / "bin" / "shadow"
    if candidate.is_file():
        return str(candidate)
    return "shadow"


def _stdio_block(client: str) -> dict[str, Any]:
    return {
        "mcpServers": {
            "shadowcode": {
                "command": _shadow_bin(),
                "args": ["mcp", "serve"],
            }
        }
    }


def _http_block(host: str, port: int, token: str | None) -> dict[str, Any]:
    url = f"http://{host}:{port}/sse"
    block: dict[str, Any] = {
        "mcpServers": {
            "shadowcode": {
                "url": url,
                "type": "sse",
            }
        }
    }
    if token:
        block["mcpServers"]["shadowcode"]["headers"] = {"Authorization": f"Bearer {token}"}
    return block


def print_register_blocks(host: str = "127.0.0.1", port: int = 7431, ensure: bool = True) -> None:
    """Print JSON config blocks for Claude Code / Cursor / Codex / generic.

    With ``ensure=True`` (default), a fresh random token is created when none
    exists, so HTTP/SSE clients are secured by default.
    """
    token = None
    if ensure:
        token = ensure_token()
    else:
        token = load_token()

    bin_path = _shadow_bin()
    print(f"# ShadowCode MCP registration — shadow-agent v{__version__}", file=sys.stderr)
    print(f"# token file: {token_file()}", file=sys.stderr)
    if token:
        print(f"# HTTP/SSE token: {token}", file=sys.stderr)
    print(file=sys.stderr)

    print("# Claude Code (~/.config/claude-code/mcp.json) — stdio")
    print(json.dumps(_stdio_block("claude-code"), indent=2))
    print()

    print("# Cursor (Settings → MCP) — stdio")
    print(json.dumps(_stdio_block("cursor"), indent=2))
    print()

    print("# Codex (~/.codex/config.toml) — stdio (TOML equivalent):")
    print(f'# [mcp_servers.shadowcode]')
    print(f'# command = "{bin_path}"')
    print(f'# args = ["mcp", "serve"]')
    print()

    print(f"# HTTP/SSE (any client that supports remote MCP) — http://{host}:{port}/sse")
    print(json.dumps(_http_block(host, port, token), indent=2))


__all__ = ["print_register_blocks", "_stdio_block", "_http_block"]
