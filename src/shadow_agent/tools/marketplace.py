"""Tool marketplace — pluggable tool registry with installed vs available.

`shadowcode tools` / `/tools` lists installed tools (registered in the
ToolRegistry) and available tools (the marketplace catalog). Tools are
pluggable: a tool plugin is a small register function that adds one or more
tools to a ToolRegistry.

The marketplace catalog is a static list of known tool packs (filesystem,
terminal, git, github, docker, browser, postgres, sqlite, ssh, kubernetes,
aws, mcp). Each pack has an `install` function that registers its tools;
whether it is "installed" depends on whether its runtime dependencies are
available (e.g. the docker CLI, the kubectl CLI, a postgres client lib).

This module is the catalog + installer; the actual tool implementations
live in `shadow_agent/tools/` and new packs can be added without touching
the agent loop.
"""

from __future__ import annotations

import importlib
import shutil
from dataclasses import dataclass, field
from typing import Any, Callable

from shadow_agent.tools.registry import ToolRegistry


@dataclass
class ToolPack:
    name: str
    description: str
    category: str
    install: Callable[[ToolRegistry], int]  # returns number of tools added
    is_available: Callable[[], bool] = field(default=lambda: True)
    installed_tools: list[str] = field(default_factory=list)


def _fs_pack(reg: ToolRegistry) -> int:
    """Filesystem tools are always installed (registered by default_tools)."""
    return 0


def _terminal_pack(reg: ToolRegistry) -> int:
    return 0


def _git_pack(reg: ToolRegistry) -> int:
    return 0


def _github_pack(reg: ToolRegistry) -> int:
    # Register a minimal github tool: `github_search` (uses gh CLI if present).
    from shadow_agent.models.types import ToolCall, ToolResult
    import subprocess

    def _github_search(call: ToolCall) -> ToolResult:
        query = str(call.arguments.get("query") or "")
        if not shutil.which("gh"):
            return ToolResult(id=call.id, success=False, error="gh CLI not installed")
        proc = subprocess.run(["gh", "search", "repos", query], capture_output=True, text=True, check=False)
        return ToolResult(id=call.id, success=proc.returncode == 0, output=proc.stdout, error=proc.stderr)

    reg.add(
        "github_search",
        "Search GitHub repos via the `gh` CLI.",
        {"type": "object", "properties": {"query": {"type": "string"}}, "required": ["query"]},
        _github_search,
        read_only=True,
    )
    return 1


def _docker_pack(reg: ToolRegistry) -> int:
    from shadow_agent.models.types import ToolCall, ToolResult
    import subprocess

    def _docker_ps(call: ToolCall) -> ToolResult:
        proc = subprocess.run(["docker", "ps", "--format", "{{.Names}}\t{{.Status}}"], capture_output=True, text=True, check=False)
        return ToolResult(id=call.id, success=proc.returncode == 0, output=proc.stdout, error=proc.stderr)

    reg.add("docker_ps", "List running docker containers.", {"type": "object", "properties": {}}, _docker_ps, read_only=True)
    return 1


def _browser_pack(reg: ToolRegistry) -> int:
    from shadow_agent.models.types import ToolCall, ToolResult

    def _browser_fetch(call: ToolCall) -> ToolResult:
        url = str(call.arguments.get("url") or "")
        if not url:
            return ToolResult(id=call.id, success=False, error="url required")
        try:
            import httpx
            resp = httpx.get(url, timeout=15.0, follow_redirects=True)
            return ToolResult(id=call.id, success=True, output=resp.text[:8000], metadata={"status": resp.status_code})
        except Exception as exc:  # noqa: BLE001
            return ToolResult(id=call.id, success=False, error=str(exc))

    reg.add(
        "browser_fetch",
        "Fetch a URL and return up to 8KB of HTML/text.",
        {"type": "object", "properties": {"url": {"type": "string"}}, "required": ["url"]},
        _browser_fetch,
        read_only=True,
    )
    return 1


def _postgres_pack(reg: ToolRegistry) -> int:
    from shadow_agent.models.types import ToolCall, ToolResult

    def _postgres_query(call: ToolCall) -> ToolResult:
        # Best-effort: use psql if present.
        import subprocess
        dsn = str(call.arguments.get("dsn") or "")
        sql = str(call.arguments.get("sql") or "")
        if not shutil.which("psql"):
            return ToolResult(id=call.id, success=False, error="psql not installed")
        proc = subprocess.run(["psql", dsn, "-c", sql], capture_output=True, text=True, check=False)
        return ToolResult(id=call.id, success=proc.returncode == 0, output=proc.stdout, error=proc.stderr)

    reg.add(
        "postgres_query",
        "Run a SQL query against a Postgres DSN via psql.",
        {"type": "object", "properties": {"dsn": {"type": "string"}, "sql": {"type": "string"}}, "required": ["dsn", "sql"]},
        _postgres_query,
        read_only=True,
    )
    return 1


def _sqlite_pack(reg: ToolRegistry) -> int:
    from shadow_agent.models.types import ToolCall, ToolResult
    import sqlite3

    def _sqlite_query(call: ToolCall) -> ToolResult:
        db = str(call.arguments.get("db") or "")
        sql = str(call.arguments.get("sql") or "")
        try:
            conn = sqlite3.connect(db)
            cur = conn.execute(sql)
            rows = cur.fetchall()[:200]
            conn.close()
            return ToolResult(id=call.id, success=True, output=str(rows))
        except Exception as exc:  # noqa: BLE001
            return ToolResult(id=call.id, success=False, error=str(exc))

    reg.add(
        "sqlite_query",
        "Run a SQL query against a SQLite database file.",
        {"type": "object", "properties": {"db": {"type": "string"}, "sql": {"type": "string"}}, "required": ["db", "sql"]},
        _sqlite_query,
        read_only=True,
    )
    return 1


def _ssh_pack(reg: ToolRegistry) -> int:
    from shadow_agent.models.types import ToolCall, ToolResult
    import subprocess

    def _ssh_run(call: ToolCall) -> ToolResult:
        host = str(call.arguments.get("host") or "")
        cmd = str(call.arguments.get("command") or "")
        if not shutil.which("ssh"):
            return ToolResult(id=call.id, success=False, error="ssh not installed")
        proc = subprocess.run(["ssh", host, cmd], capture_output=True, text=True, check=False, timeout=60)
        return ToolResult(id=call.id, success=proc.returncode == 0, output=proc.stdout, error=proc.stderr)

    reg.add(
        "ssh_run",
        "Run a command over SSH.",
        {"type": "object", "properties": {"host": {"type": "string"}, "command": {"type": "string"}}, "required": ["host", "command"]},
        _ssh_run,
    )
    return 1


def _kubernetes_pack(reg: ToolRegistry) -> int:
    from shadow_agent.models.types import ToolCall, ToolResult
    import subprocess

    def _kubectl(call: ToolCall) -> ToolResult:
        args = call.arguments.get("args") or []
        if not isinstance(args, list):
            args = [str(args)]
        if not shutil.which("kubectl"):
            return ToolResult(id=call.id, success=False, error="kubectl not installed")
        proc = subprocess.run(["kubectl", *args], capture_output=True, text=True, check=False, timeout=60)
        return ToolResult(id=call.id, success=proc.returncode == 0, output=proc.stdout, error=proc.stderr)

    reg.add(
        "kubectl",
        "Run a kubectl command.",
        {"type": "object", "properties": {"args": {"type": "array", "items": {"type": "string"}}}},
        _kubectl,
    )
    return 1


def _aws_pack(reg: ToolRegistry) -> int:
    from shadow_agent.models.types import ToolCall, ToolResult
    import subprocess

    def _aws_cli(call: ToolCall) -> ToolResult:
        args = call.arguments.get("args") or []
        if not isinstance(args, list):
            args = [str(args)]
        if not shutil.which("aws"):
            return ToolResult(id=call.id, success=False, error="aws CLI not installed")
        proc = subprocess.run(["aws", *args], capture_output=True, text=True, check=False, timeout=120)
        return ToolResult(id=call.id, success=proc.returncode == 0, output=proc.stdout, error=proc.stderr)

    reg.add(
        "aws_cli",
        "Run an AWS CLI command.",
        {"type": "object", "properties": {"args": {"type": "array", "items": {"type": "string"}}}},
        _aws_cli,
    )
    return 1


def _mcp_pack(reg: ToolRegistry) -> int:
    from shadow_agent.models.types import ToolCall, ToolResult

    def _mcp_call(call: ToolCall) -> ToolResult:
        # Stub: MCP integration is configured via config.mcp.servers; this tool
        # surfaces that a server exists but does not implement the MCP runtime.
        return ToolResult(id=call.id, success=False, error="MCP runtime not wired; configure servers in config.mcp and call them directly.")

    reg.add(
        "mcp_call",
        "Call an MCP server tool (configured in config.mcp.servers).",
        {"type": "object", "properties": {"server": {"type": "string"}, "tool": {"type": "string"}, "arguments": {"type": "object"}}},
        _mcp_call,
        read_only=True,
    )
    return 1


CATALOG: list[ToolPack] = [
    ToolPack("filesystem", "Read/write/list files in the workspace", "core", _fs_pack, lambda: True),
    ToolPack("terminal", "Run shell commands in the workspace", "core", _terminal_pack, lambda: True),
    ToolPack("git", "git status/diff/log/branch/commit", "core", _git_pack, lambda: True),
    ToolPack("github", "Search GitHub via the gh CLI", "devops", _github_pack, lambda: shutil.which("gh") is not None),
    ToolPack("docker", "List running containers", "devops", _docker_pack, lambda: shutil.which("docker") is not None),
    ToolPack("browser", "Fetch a URL and return HTML/text", "web", _browser_pack, lambda: True),
    ToolPack("postgres", "Run SQL against a Postgres DSN via psql", "data", _postgres_pack, lambda: shutil.which("psql") is not None),
    ToolPack("sqlite", "Run SQL against a SQLite database file", "data", _sqlite_pack, lambda: True),
    ToolPack("ssh", "Run a command over SSH", "remote", _ssh_pack, lambda: shutil.which("ssh") is not None),
    ToolPack("kubernetes", "Run kubectl commands", "devops", _kubernetes_pack, lambda: shutil.which("kubectl") is not None),
    ToolPack("aws", "Run AWS CLI commands", "cloud", _aws_pack, lambda: shutil.which("aws") is not None),
    ToolPack("mcp", "Call MCP server tools (configured in config.mcp)", "ext", _mcp_pack, lambda: True),
]


def install_pack(reg: ToolRegistry, name: str) -> int:
    """Install a pack by name. Returns the number of tools added (0 if already
    installed or unavailable)."""
    pack = next((p for p in CATALOG if p.name == name), None)
    if pack is None:
        return 0
    if not pack.is_available():
        return 0
    if any(t in reg.names() for t in pack.installed_tools):
        return 0
    added = pack.install(reg)
    pack.installed_tools.extend(reg.names()[-added:] if added else [])
    return added


def install_default_packs(reg: ToolRegistry) -> int:
    """Install every available pack whose tools aren't already registered."""
    total = 0
    for pack in CATALOG:
        total += install_pack(reg, pack.name)
    return total


def marketplace_view(reg: ToolRegistry) -> dict[str, Any]:
    """Return {installed: [...], available: [...], counts: {installed, available}}."""
    installed_names = set(reg.names())
    installed: list[dict[str, Any]] = []
    available: list[dict[str, Any]] = []
    for pack in CATALOG:
        is_installed = bool(pack.installed_tools and any(t in installed_names for t in pack.installed_tools)) or pack.name in {"filesystem", "terminal", "git"} and pack.name in {"filesystem", "terminal", "git"}
        entry = {
            "name": pack.name,
            "category": pack.category,
            "description": pack.description,
            "available": pack.is_available(),
            "installed": is_installed,
        }
        (installed if is_installed else available).append(entry)
    return {
        "installed": installed,
        "available": available,
        "counts": {"installed": len(installed), "available": len(available), "total": len(CATALOG)},
    }


def render_marketplace(view: dict[str, Any]) -> str:
    lines = [f"Tool marketplace  ·  installed {view['counts']['installed']} / available {view['counts']['available']} (total {view['counts']['total']})", ""]
    lines.append("Installed:")
    for entry in view["installed"]:
        lines.append(f"  ✓ {entry['name']:12}  {entry['category']:8}  {entry['description']}")
    lines.append("")
    lines.append("Available:")
    for entry in view["available"]:
        mark = "○" if entry["available"] else "✗"
        lines.append(f"  {mark} {entry['name']:12}  {entry['category']:8}  {entry['description']}")
    return "\n".join(lines)
