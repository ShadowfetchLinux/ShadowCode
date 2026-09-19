"""MCP is designed-for, not required in v1."""

from __future__ import annotations

from pydantic import BaseModel

from shadow_agent.config import MCPConfig


class MCPServer(BaseModel):
    name: str
    command: list[str] | None = None
    url: str | None = None
    connected: bool = False


class MCPBridge:
    def __init__(self, config: MCPConfig | None = None) -> None:
        self.servers = [MCPServer(**item.model_dump()) for item in (config.servers if config else [])]

    def list_servers(self) -> list[MCPServer]:
        return list(self.servers)

    def connect(self, name: str) -> MCPServer:
        for server in self.servers:
            if server.name == name:
                # Handshake reserved for a later milestone.
                server.connected = False
                return server
        raise KeyError(name)
