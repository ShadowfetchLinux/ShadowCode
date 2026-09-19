"""Provider-agnostic model types. Adapters translate native protocols."""

from __future__ import annotations

from typing import Any, Literal

from pydantic import BaseModel, Field


class ToolCall(BaseModel):
    id: str
    tool_name: str
    arguments: dict[str, Any] = Field(default_factory=dict)


class ToolResult(BaseModel):
    id: str
    success: bool
    output: str = ""
    error: str = ""
    metadata: dict[str, Any] = Field(default_factory=dict)


class Message(BaseModel):
    role: Literal["system", "user", "assistant", "tool"]
    content: str = ""
    tool_calls: list[ToolCall] = Field(default_factory=list)
    tool_call_id: str | None = None
    name: str | None = None


class ToolSpec(BaseModel):
    name: str
    description: str
    parameters: dict[str, Any] = Field(default_factory=dict)


class ChatRequest(BaseModel):
    messages: list[Message]
    tools: list[ToolSpec] = Field(default_factory=list)
    model: str | None = None
    max_tokens: int | None = None
    temperature: float | None = None


class ChatResponse(BaseModel):
    text: str = ""
    tool_calls: list[ToolCall] = Field(default_factory=list)
    finish: bool = False
    usage: dict[str, int] = Field(default_factory=dict)
    raw: dict[str, Any] = Field(default_factory=dict)


class Capabilities(BaseModel):
    chat: bool = True
    stream: bool = True
    tools: bool = True
    vision: bool = False
    provider: str = ""


class ModelInfo(BaseModel):
    id: str
    name: str
    provider: str
    endpoint: str = ""
    context_limit: int = 128000
    metadata: dict[str, Any] = Field(default_factory=dict)
