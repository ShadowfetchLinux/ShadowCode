"""`python -m shadow_agent.mcp_server` → stdio MCP server (for testing)."""

from __future__ import annotations

from pathlib import Path

from shadow_agent.mcp_server.server import serve_stdio


def main() -> None:
    # Default to the current working directory; tests override via env.
    workspace = Path.cwd()
    serve_stdio(workspace=workspace)


if __name__ == "__main__":
    main()
