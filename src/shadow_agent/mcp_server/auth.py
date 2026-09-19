"""Auth + loopback guard for the ShadowCode MCP server.

Defaults to loopback only (127.0.0.1). An optional bearer token can be placed
in ``~/.config/shadow-agent/mcp-token``; when present, every HTTP/SSE request
must carry ``Authorization: Bearer <token>``. Secrets are never echoed back.
"""

from __future__ import annotations

import os
import secrets as _secrets
from pathlib import Path
from typing import Any

from shadow_agent import paths


def token_file() -> Path:
    return paths.config_dir() / "mcp-token"


def load_token() -> str | None:
    """Return the configured bearer token, or None when no token is set.

    Never raises: a missing/unreadable token means "no auth required".
    """
    path = token_file()
    if not path.is_file():
        return None
    try:
        text = path.read_text(encoding="utf-8").strip()
    except OSError:
        return None
    if not text:
        return None
    return text


def ensure_token() -> str:
    """Create a random token if none exists, then return it.

    Used by ``shadow mcp register`` so a fresh install has a token by default.
    """
    existing = load_token()
    if existing:
        return existing
    path = token_file()
    new = _secrets.token_urlsafe(32)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(new + "\n", encoding="utf-8")
    try:
        path.chmod(0o600)
    except OSError:
        pass
    return new


def request_allowed(scope: dict[str, Any], *, token: str | None) -> bool:
    """Authorize an ASGI/SSE request scope against the configured token.

    Loopback is implicit (we bind 127.0.0.1 by default). When a token is
    configured, the request must carry a matching ``Authorization`` header.
    """
    if token is None:
        return True
    headers: dict[bytes, bytes] = {
        (k or b"").lower(): (v or b"")
        for k, v in (scope.get("headers") or [])
    }
    raw = headers.get(b"authorization", b"")
    try:
        provided = raw.decode("latin-1").strip()
    except UnicodeDecodeError:
        return False
    if not provided.lower().startswith("bearer "):
        return False
    candidate = provided.split(" ", 1)[1].strip()
    # Constant-time compare to avoid timing leaks.
    return _secrets.compare_digest(candidate, token)


def redact(obj: Any) -> Any:
    """Strip obvious secret-looking keys from a dict before returning it to a client."""
    if isinstance(obj, dict):
        out: dict[str, Any] = {}
        for key, value in obj.items():
            if isinstance(key, str) and any(
                token in key.lower()
                for token in ("key", "token", "secret", "password", "auth")
            ):
                out[key] = "***"
            else:
                out[key] = redact(value)
        return out
    if isinstance(obj, list):
        return [redact(item) for item in obj]
    return obj


__all__ = [
    "token_file",
    "load_token",
    "ensure_token",
    "request_allowed",
    "redact",
]
