"""Native Ollama adapter (http://127.0.0.1:11434/api/chat).

Uses the native endpoint instead of the OpenAI-compatible shim because it is
dramatically faster for thinking models: `think: false` is honored server-side
(19 eval tokens vs 300+ through /v1), tool calls stream correctly, and usage
counts come back as prompt_eval_count/eval_count.
"""

from __future__ import annotations

import json
import uuid
from collections.abc import Iterator
from typing import Any

import httpx

from shadow_agent.models.interface import ModelProvider
from shadow_agent.models.types import Capabilities, ChatRequest, ChatResponse, Message, ToolCall, ToolSpec


class OllamaProvider(ModelProvider):
    name = "ollama"

    def __init__(
        self,
        model: str = "llama3.2",
        endpoint: str = "http://127.0.0.1:11434/v1",
        api_key_env: str = "OLLAMA_API_KEY",
        context_limit: int = 128000,
        timeout: float = 600.0,
        think: bool = False,
        num_ctx: int | None = None,
    ) -> None:
        self.model = model or "llama3.2"
        self.endpoint = self._native_base(endpoint)
        self.api_key_env = api_key_env
        self._context_limit = context_limit
        self.timeout = timeout
        self.think = think
        self.num_ctx = num_ctx

    @staticmethod
    def _native_base(endpoint: str) -> str:
        base = (endpoint or "http://127.0.0.1:11434").rstrip("/")
        if base.endswith("/v1"):
            base = base[: -len("/v1")]
        return base

    def get_capabilities(self) -> Capabilities:
        return Capabilities(chat=True, stream=True, tools=True, vision=False, provider=self.name)

    def get_context_limit(self) -> int:
        return self._context_limit

    def generate(self, prompt: str, **kwargs: object) -> str:
        return self.chat(ChatRequest(messages=[Message(role="user", content=prompt)])).text

    def chat(self, request: ChatRequest) -> ChatResponse:
        payload = self._payload(request, stream=False)
        response = httpx.post(f"{self.endpoint}/api/chat", json=payload, timeout=self.timeout)
        response.raise_for_status()
        return self._parse(response.json())

    def stream(self, request: ChatRequest) -> Iterator[str]:
        payload = self._payload(request, stream=True)
        with httpx.stream("POST", f"{self.endpoint}/api/chat", json=payload, timeout=self.timeout) as response:
            response.raise_for_status()
            for line in response.iter_lines():
                if not line:
                    continue
                try:
                    chunk = json.loads(line)
                except json.JSONDecodeError:
                    continue
                message = chunk.get("message") or {}
                # Skip reasoning chunks; only surface user-visible content.
                text = message.get("content") or ""
                if text:
                    yield text
                if chunk.get("done"):
                    break

    def _payload(self, request: ChatRequest, stream: bool) -> dict[str, Any]:
        body: dict[str, Any] = {
            "model": request.model or self.model,
            "messages": [_to_ollama_message(msg) for msg in request.messages],
            "stream": stream,
            "think": self.think,
        }
        if request.tools:
            body["tools"] = [_to_ollama_tool(spec) for spec in request.tools]
        options: dict[str, Any] = {}
        if request.temperature is not None:
            options["temperature"] = request.temperature
        if request.max_tokens is not None:
            options["num_predict"] = request.max_tokens
        if self.num_ctx:
            options["num_ctx"] = self.num_ctx
        if options:
            body["options"] = options
        return body

    def _parse(self, data: dict[str, Any]) -> ChatResponse:
        message = data.get("message") or {}
        tool_calls: list[ToolCall] = []
        for raw in message.get("tool_calls") or []:
            fn = raw.get("function") or {}
            args = fn.get("arguments") or {}
            if isinstance(args, str):
                try:
                    args = json.loads(args) if args else {}
                except json.JSONDecodeError:
                    args = {"_raw": args}
            tool_calls.append(
                ToolCall(
                    id=raw.get("id") or "call_" + uuid.uuid4().hex[:8],
                    tool_name=fn.get("name") or "",
                    arguments=args if isinstance(args, dict) else {"value": args},
                )
            )
        text = message.get("content") or ""
        done_reason = data.get("done_reason") or ""
        finish = bool(data.get("done")) and done_reason in {"stop", ""} and not tool_calls
        usage = {
            "prompt_tokens": int(data.get("prompt_eval_count") or 0),
            "completion_tokens": int(data.get("eval_count") or 0),
        }
        usage["total_tokens"] = usage["prompt_tokens"] + usage["completion_tokens"]
        return ChatResponse(text=text, tool_calls=tool_calls, finish=finish, usage=usage, raw=data)


def _to_ollama_message(message: Message) -> dict[str, Any]:
    if message.role == "tool":
        payload: dict[str, Any] = {"role": "tool", "content": message.content}
        if message.name:
            payload["name"] = message.name
        return payload
    payload = {"role": message.role, "content": message.content}
    if message.tool_calls:
        payload["tool_calls"] = [
            {"type": "function", "function": {"name": call.tool_name, "arguments": call.arguments}}
            for call in message.tool_calls
        ]
    return payload


def _to_ollama_tool(spec: ToolSpec) -> dict[str, Any]:
    return {
        "type": "function",
        "function": {
            "name": spec.name,
            "description": spec.description,
            "parameters": spec.parameters or {"type": "object", "properties": {}},
        },
    }
