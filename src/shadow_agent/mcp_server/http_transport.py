"""HTTP/SSE transport for the ShadowCode MCP server.

Uses Starlette + the official `mcp` SSE transport. Loopback only by default;
optional bearer token enforced on every request.
"""

from __future__ import annotations

import secrets as _secrets
from typing import Any

from mcp.server.sse import SseServerTransport


def build_http_app(server: Any, token: str | None, host: str, port: int) -> Any:
    """Return an ASGI app exposing /sse + /messages for the MCP server."""
    from starlette.applications import Starlette
    from starlette.requests import Request
    from starlette.responses import JSONResponse
    from starlette.routing import Mount, Route

    sse = SseServerTransport("/messages")

    def _authorized_scope(scope: dict) -> bool:
        if token is None:
            return True
        headers = {k.decode("latin-1").lower(): v.decode("latin-1") for k, v in scope.get("headers", [])}
        provided = headers.get("authorization", "")
        if not provided.lower().startswith("bearer "):
            return False
        return _secrets.compare_digest(provided.split(" ", 1)[1].strip(), token)

    async def handle_sse(request: Request) -> None:
        if not _authorized_scope(request.scope):
            return JSONResponse({"error": "unauthorized"}, status_code=401)
        async with sse.connect_sse(request.scope, request.receive, request._send) as (read, write):
            await server.run(read, write, server.create_initialization_options())

    async def health(request: Request) -> JSONResponse:
        return JSONResponse({"ok": True, "name": "shadowcode", "version": _version()})

    async def list_tools(request: Request) -> JSONResponse:
        if not _authorized_scope(request.scope):
            return JSONResponse({"error": "unauthorized"}, status_code=401)
        from shadow_agent.mcp_server.catalog import TOOL_DEFS
        return JSONResponse({"tools": [{"name": td.name, "description": td.description} for td in TOOL_DEFS]})

    # Wrap the ASGI post handler with auth.
    async def authed_post_handler(scope: dict, receive: Any, send: Any) -> None:
        if not _authorized_scope(scope):
            response = JSONResponse({"error": "unauthorized"}, status_code=401)
            await response(scope, receive, send)
            return
        await sse.handle_post_message(scope, receive, send)

    app = Starlette(
        debug=False,
        routes=[
            Route("/sse", endpoint=handle_sse),
            Mount("/messages", app=authed_post_handler),
            Route("/health", endpoint=health),
            Route("/tools", endpoint=list_tools),
        ],
    )
    return app


def _version() -> str:
    from shadow_agent import __version__
    return __version__


__all__ = ["build_http_app"]
