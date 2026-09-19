"""MCP client — connect ShadowCode to external MCP servers.

Loads server configs from ``.shadowcode/mcp/*.yaml`` (or ``.json``) and exposes
their tools to the agent. Each config file describes one server:

    name: filesystem
    command: ["npx", "-y", "@modelcontextprotocol/server-filesystem", "/abs/path"]
    # OR
    url: http://127.0.0.1:8765/sse

For the in-tree smoke we ship two **in-process** MCP servers (no external
binary required): ``filesystem`` and ``sqlite``. They implement the same
``list_tools`` / ``call_tool`` contract as external servers would, so the
agent sees a uniform surface.

External servers are launched as subprocesses speaking JSON-RPC over
stdio (the standard MCP transport). The client is intentionally minimal:
it lists tools, calls tools, and surfaces errors. It does not implement
streaming or resource subscriptions — those are reserved for the in-flight
MCP-server agent (a9fd19d4) to avoid duplication.
"""

from __future__ import annotations

import json
import os
import re
import sqlite3
import subprocess
import threading
import time
from pathlib import Path
from typing import Any

import yaml

from shadow_agent import __version__
from shadow_agent.models.types import ToolResult, ToolSpec


class MCPServerConfig:
    def __init__(
        self,
        name: str,
        *,
        command: list[str] | None = None,
        url: str | None = None,
        env: dict[str, str] | None = None,
        description: str = "",
        builtin: str | None = None,
    ) -> None:
        self.name = name
        self.command = command
        self.url = url
        self.env = env or {}
        self.description = description
        self.builtin = builtin  # "filesystem" | "sqlite" | None

    def to_dict(self) -> dict[str, Any]:
        return {
            "name": self.name,
            "command": self.command,
            "url": self.url,
            "description": self.description,
            "builtin": self.builtin,
        }


class MCPClient:
    """Loads configured MCP servers and exposes their tools to the agent."""

    def __init__(self, workspace: Path | None = None) -> None:
        self.workspace = Path(workspace) if workspace else None
        self._servers: dict[str, MCPServerConfig] = {}
        self._tools: dict[str, list[ToolSpec]] = {}
        self._sessions: dict[str, _SubprocessSession] = {}
        self._builtin_runtimes: dict[str, _BuiltinRuntime] = {}
        self._last_errors: dict[str, str] = {}
        self._lock = threading.Lock()
        self._register_builtins()

    def load(self, directory: Path | None = None) -> list[MCPServerConfig]:
        """Load ``.shadowcode/mcp/*.yaml`` (and ``.json``) from the workspace."""
        loaded: list[MCPServerConfig] = []
        if directory is None:
            if self.workspace is None:
                return loaded
            directory = self.workspace / ".shadowcode" / "mcp"
        if not directory.is_dir():
            return loaded
        for path in sorted(list(directory.glob("*.yaml")) + list(directory.glob("*.yml")) + list(directory.glob("*.json"))):
            try:
                raw = path.read_text(encoding="utf-8")
                data = yaml.safe_load(raw) if path.suffix in {".yaml", ".yml"} else json.loads(raw)
            except (OSError, yaml.YAMLError, json.JSONDecodeError):
                continue
            if not isinstance(data, dict):
                continue
            name = str(data.get("name") or path.stem)
            cfg = MCPServerConfig(
                name=name,
                command=data.get("command"),
                url=data.get("url"),
                env=data.get("env") or {},
                description=str(data.get("description") or ""),
                builtin=data.get("builtin"),
            )
            self._servers[name] = cfg
            loaded.append(cfg)
        return loaded

    def list_servers(self) -> list[MCPServerConfig]:
        return list(self._servers.values())

    def get_server(self, name: str) -> MCPServerConfig | None:
        return self._servers.get(name)

    def connect(self, name: str) -> list[ToolSpec]:
        """Connect to a server (launch subprocess or init builtin) and list tools."""
        cfg = self._servers.get(name)
        if cfg is None:
            raise KeyError(name)
        if cfg.builtin:
            runtime = self._builtin_runtimes.get(cfg.builtin)
            if runtime is None:
                raise KeyError(f"unknown builtin {cfg.builtin}")
            tools = runtime.list_tools()
            self._tools[name] = tools
            return tools
        if cfg.command:
            session = _SubprocessSession(cfg)
            session.start()
            tools = session.list_tools()
            self._sessions[name] = session
            self._tools[name] = tools
            return tools
        if cfg.url:
            raise RuntimeError(f"MCP server {name} uses url transport; not yet supported by the in-tree client")
        raise RuntimeError(f"MCP server {name} has no command or url")

    def connect_all(self) -> dict[str, list[ToolSpec]]:
        out: dict[str, list[ToolSpec]] = {}
        for name in list(self._servers):
            try:
                out[name] = self.connect(name)
            except Exception as exc:  # noqa: BLE001
                out[name] = []
                self._last_errors[name] = str(exc)
        return out

    def list_tools(self, server: str | None = None) -> list[ToolSpec]:
        if server:
            return list(self._tools.get(server, []))
        out: list[ToolSpec] = []
        for tools in self._tools.values():
            out.extend(tools)
        return out

    def all_tool_specs(self) -> list[ToolSpec]:
        return self.list_tools()

    def call(self, server: str, tool_name: str, arguments: dict[str, Any]) -> ToolResult:
        cfg = self._servers.get(server)
        if cfg is None:
            return ToolResult(id=f"mcp-{server}", success=False, error=f"unknown MCP server: {server}")
        if cfg.builtin:
            runtime = self._builtin_runtimes.get(cfg.builtin)
            if runtime is None:
                return ToolResult(id=f"mcp-{server}", success=False, error=f"builtin {cfg.builtin} not registered")
            return runtime.call_tool(tool_name, arguments)
        session = self._sessions.get(server)
        if session is None:
            return ToolResult(id=f"mcp-{server}", success=False, error=f"server {server} not connected")
        return session.call_tool(tool_name, arguments)

    def call_by_tool_name(self, tool_name: str, arguments: dict[str, Any]) -> ToolResult:
        for server, tools in self._tools.items():
            if any(t.name == tool_name for t in tools):
                return self.call(server, tool_name, arguments)
        return ToolResult(id="mcp-dispatch", success=False, error=f"no MCP server owns tool {tool_name!r}")

    def close(self) -> None:
        for session in list(self._sessions.values()):
            try:
                session.stop()
            except Exception:  # noqa: BLE001
                pass
        self._sessions.clear()

    def _register_builtins(self) -> None:
        self._builtin_runtimes["filesystem"] = _FilesystemRuntime()
        self._builtin_runtimes["sqlite"] = _SQLiteRuntime()
        for name, runtime in self._builtin_runtimes.items():
            cfg = MCPServerConfig(name=name, builtin=name, description=runtime.description)
            self._servers[name] = cfg
            self._tools[name] = runtime.list_tools()


class _BuiltinRuntime:
    description = "in-process MCP server"

    def list_tools(self) -> list[ToolSpec]:
        raise NotImplementedError

    def call_tool(self, name: str, arguments: dict[str, Any]) -> ToolResult:
        raise NotImplementedError


class _FilesystemRuntime(_BuiltinRuntime):
    description = "in-process filesystem MCP server (read/list/search)"

    def list_tools(self) -> list[ToolSpec]:
        return [
            ToolSpec(
                name="mcp_fs_list",
                description="List files in a directory (MCP filesystem).",
                parameters={"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]},
            ),
            ToolSpec(
                name="mcp_fs_read",
                description="Read a file as text (MCP filesystem).",
                parameters={"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]},
            ),
            ToolSpec(
                name="mcp_fs_search",
                description="Search file contents for a regex (MCP filesystem).",
                parameters={
                    "type": "object",
                    "properties": {"path": {"type": "string"}, "pattern": {"type": "string"}},
                    "required": ["path", "pattern"],
                },
            ),
        ]

    def call_tool(self, name: str, arguments: dict[str, Any]) -> ToolResult:
        path = str(arguments.get("path") or ".")
        if name == "mcp_fs_list":
            p = Path(path)
            if not p.is_dir():
                return ToolResult(id="mcp_fs_list", success=False, error=f"not a directory: {path}")
            entries = sorted([str(child.name) for child in p.iterdir()])
            return ToolResult(id="mcp_fs_list", success=True, output=json.dumps({"entries": entries}))
        if name == "mcp_fs_read":
            p = Path(path)
            if not p.is_file():
                return ToolResult(id="mcp_fs_read", success=False, error=f"not a file: {path}")
            try:
                return ToolResult(id="mcp_fs_read", success=True, output=p.read_text(encoding="utf-8"))
            except UnicodeDecodeError as exc:
                return ToolResult(id="mcp_fs_read", success=False, error=str(exc))
        if name == "mcp_fs_search":
            pattern = str(arguments.get("pattern") or "")
            p = Path(path)
            matches: list[dict[str, Any]] = []
            try:
                regex = re.compile(pattern)
            except re.error as exc:
                return ToolResult(id="mcp_fs_search", success=False, error=f"bad regex: {exc}")
            for f in p.rglob("*"):
                if not f.is_file() or ".git" in f.parts:
                    continue
                try:
                    text = f.read_text(encoding="utf-8")
                except (OSError, UnicodeDecodeError):
                    continue
                for i, line in enumerate(text.splitlines(), 1):
                    if regex.search(line):
                        matches.append({"path": str(f), "line": i, "text": line[:200]})
                        if len(matches) >= 50:
                            break
                if len(matches) >= 50:
                    break
            return ToolResult(id="mcp_fs_search", success=True, output=json.dumps({"matches": matches}))
        return ToolResult(id=name, success=False, error=f"unknown filesystem tool: {name}")


class _SQLiteRuntime(_BuiltinRuntime):
    description = "in-process sqlite MCP server (query/tables)"

    def list_tools(self) -> list[ToolSpec]:
        return [
            ToolSpec(
                name="mcp_sqlite_tables",
                description="List tables in a SQLite database file (MCP sqlite).",
                parameters={"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]},
            ),
            ToolSpec(
                name="mcp_sqlite_query",
                description="Run a read-only SQL query against a SQLite database file (MCP sqlite).",
                parameters={
                    "type": "object",
                    "properties": {"path": {"type": "string"}, "sql": {"type": "string"}},
                    "required": ["path", "sql"],
                },
            ),
        ]

    def call_tool(self, name: str, arguments: dict[str, Any]) -> ToolResult:
        path = str(arguments.get("path") or "")
        if not path:
            return ToolResult(id=name, success=False, error="path is required")
        if name == "mcp_sqlite_tables":
            try:
                conn = sqlite3.connect(path)
                rows = conn.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name").fetchall()
                conn.close()
            except sqlite3.Error as exc:
                return ToolResult(id=name, success=False, error=str(exc))
            return ToolResult(id=name, success=True, output=json.dumps({"tables": [r[0] for r in rows]}))
        if name == "mcp_sqlite_query":
            sql = str(arguments.get("sql") or "")
            if not sql.strip().lower().startswith("select"):
                return ToolResult(id=name, success=False, error="only SELECT queries are allowed")
            try:
                conn = sqlite3.connect(path)
                conn.row_factory = sqlite3.Row
                cur = conn.execute(sql)
                rows = [dict(r) for r in cur.fetchall()]
                conn.close()
            except sqlite3.Error as exc:
                return ToolResult(id=name, success=False, error=str(exc))
            return ToolResult(id=name, success=True, output=json.dumps({"rows": rows}, default=str))
        return ToolResult(id=name, success=False, error=f"unknown sqlite tool: {name}")


class _SubprocessSession:
    """Minimal stdio JSON-RPC session for an external MCP server."""

    def __init__(self, cfg: MCPServerConfig) -> None:
        self.cfg = cfg
        self.proc: subprocess.Popen | None = None
        self._next_id = 1
        self._lock = threading.Lock()

    def start(self) -> None:
        if not self.cfg.command:
            raise RuntimeError("no command for MCP server")
        env = os.environ.copy()
        env.update(self.cfg.env)
        self.proc = subprocess.Popen(  # noqa: S603
            self.cfg.command,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=env,
            text=True,
            bufsize=1,
        )
        self._rpc("initialize", {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "shadow-agent", "version": __version__}})

    def list_tools(self) -> list[ToolSpec]:
        result = self._rpc("tools/list", {})
        tools: list[ToolSpec] = []
        if isinstance(result, dict):
            for raw in result.get("tools", []) or []:
                name = raw.get("name")
                if not name:
                    continue
                tools.append(
                    ToolSpec(
                        name=f"mcp_{self.cfg.name}_{name}",
                        description=str(raw.get("description") or ""),
                        parameters=raw.get("inputSchema") or raw.get("parameters") or {"type": "object", "properties": {}},
                    )
                )
        return tools

    def call_tool(self, tool_name: str, arguments: dict[str, Any]) -> ToolResult:
        bare = tool_name
        prefix = f"mcp_{self.cfg.name}_"
        if tool_name.startswith(prefix):
            bare = tool_name[len(prefix):]
        result = self._rpc("tools/call", {"name": bare, "arguments": arguments})
        if not isinstance(result, dict):
            return ToolResult(id=tool_name, success=False, error="malformed MCP response")
        content = result.get("content") or []
        text_parts: list[str] = []
        for item in content:
            if isinstance(item, dict) and item.get("type") == "text":
                text_parts.append(str(item.get("text") or ""))
        return ToolResult(id=tool_name, success=not result.get("isError", False), output="\n".join(text_parts), error=result.get("error") or "")

    def stop(self) -> None:
        if self.proc is not None and self.proc.poll() is None:
            try:
                self.proc.terminate()
                self.proc.wait(timeout=2.0)
            except Exception:  # noqa: BLE001
                try:
                    self.proc.kill()
                except Exception:  # noqa: BLE001
                    pass
        self.proc = None

    def _rpc(self, method: str, params: dict[str, Any]) -> dict[str, Any] | None:
        if self.proc is None or self.proc.stdin is None or self.proc.stdout is None:
            raise RuntimeError("MCP session not started")
        with self._lock:
            rid = self._next_id
            self._next_id += 1
            payload = {"jsonrpc": "2.0", "id": rid, "method": method, "params": params}
            self.proc.stdin.write(json.dumps(payload) + "\n")
            self.proc.stdin.flush()
            deadline = time.time() + 15.0
            while time.time() < deadline:
                line = self.proc.stdout.readline()
                if not line:
                    break
                try:
                    msg = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if msg.get("id") == rid:
                    return msg.get("result")
            return None


def write_server_config(directory: Path, name: str, *, command: list[str] | None = None, url: str | None = None) -> Path:
    """Write a ``.shadowcode/mcp/<name>.yaml`` config file."""
    directory.mkdir(parents=True, exist_ok=True)
    data: dict[str, Any] = {"name": name}
    if command:
        data["command"] = command
    if url:
        data["url"] = url
    path = directory / f"{name}.yaml"
    path.write_text(yaml.safe_dump(data, sort_keys=False), encoding="utf-8")
    return path
