"""Remote OpenAI-compatible Chat Completions adapter (Grok, OpenAI, proxies)."""

from __future__ import annotations

import json
import os
import uuid
from collections.abc import Iterator
from typing import Any

import httpx

from shadow_agent.models.interface import ModelProvider
from shadow_agent.models.types import Capabilities, ChatRequest, ChatResponse, Message, ToolCall, ToolSpec


class OpenAICompatibleProvider(ModelProvider):
    name = "openai_compatible"

    def __init__(
        self,
        model: str,
        endpoint: str,
        api_key_env: str = "OPENAI_API_KEY",
        context_limit: int = 128000,
        timeout: float = 120.0,
    ) -> None:
        self.model = model
        self.endpoint = endpoint.rstrip("/")
        self.api_key_env = api_key_env
        self._context_limit = context_limit
        self.timeout = timeout

    def _headers(self) -> dict[str, str]:
        headers = {"Content-Type": "application/json"}
        key = os.environ.get(self.api_key_env, "")
        if key:
            headers["Authorization"] = f"Bearer {key}"
        return headers

    def _url(self, suffix: str) -> str:
        base = self.endpoint
        if base.endswith("/v1"):
            return base + suffix
        return base + "/v1" + suffix

    def get_capabilities(self) -> Capabilities:
        return Capabilities(chat=True, stream=True, tools=True, vision=False, provider=self.name)

    def get_context_limit(self) -> int:
        return self._context_limit

    def generate(self, prompt: str, **kwargs: object) -> str:
        return self.chat(ChatRequest(messages=[Message(role="user", content=prompt)])).text

    def chat(self, request: ChatRequest) -> ChatResponse:
        payload = self._payload(request, stream=False)
        response = httpx.post(self._url("/chat/completions"), headers=self._headers(), json=payload, timeout=self.timeout)
        response.raise_for_status()
        data = response.json()
        return self._parse(data)

    def stream(self, request: ChatRequest) -> Iterator[str]:
        payload = self._payload(request, stream=True)
        with httpx.stream(
            "POST",
            self._url("/chat/completions"),
            headers=self._headers(),
            json=payload,
            timeout=self.timeout,
        ) as response:
            response.raise_for_status()
            for line in response.iter_lines():
                if not line:
                    continue
                if line.startswith("data: "):
                    line = line[6:]
                if line.strip() == "[DONE]":
                    break
                try:
                    chunk = json.loads(line)
                except json.JSONDecodeError:
                    continue
                delta = (chunk.get("choices") or [{}])[0].get("delta") or {}
                text = delta.get("content") or ""
                if text:
                    yield text

    def _payload(self, request: ChatRequest, stream: bool) -> dict[str, Any]:
        body: dict[str, Any] = {
            "model": request.model or self.model,
            "messages": [_to_openai_message(msg) for msg in request.messages],
            "stream": stream,
        }
        if request.tools:
            body["tools"] = [_to_openai_tool(spec) for spec in request.tools]
            body["tool_choice"] = "auto"
        if request.max_tokens is not None:
            body["max_tokens"] = request.max_tokens
        if request.temperature is not None:
            body["temperature"] = request.temperature
        return body

    def _parse(self, data: dict[str, Any]) -> ChatResponse:
        choice = (data.get("choices") or [{}])[0]
        message = choice.get("message") or {}
        tool_calls: list[ToolCall] = []
        for raw in message.get("tool_calls") or []:
            fn = raw.get("function") or {}
            args = fn.get("arguments") or "{}"
            if isinstance(args, str):
                try:
                    parsed = json.loads(args) if args else {}
                except json.JSONDecodeError:
                    parsed = {"_raw": args}
            else:
                parsed = args
            tool_calls.append(
                ToolCall(
                    id=raw.get("id") or "call_" + uuid.uuid4().hex[:8],
                    tool_name=fn.get("name") or "",
                    arguments=parsed if isinstance(parsed, dict) else {"value": parsed},
                )
            )
        finish_reason = choice.get("finish_reason")
        text = message.get("content") or ""
        finish = finish_reason in {"stop", "end_turn"} and not tool_calls
        usage = data.get("usage") or {}
        return ChatResponse(
            text=text,
            tool_calls=tool_calls,
            finish=finish,
            usage={k: int(v) for k, v in usage.items() if isinstance(v, (int, float))},
            raw=data,
        )


def _to_openai_message(message: Message) -> dict[str, Any]:
    if message.role == "tool":
        return {
            "role": "tool",
            "tool_call_id": message.tool_call_id or "",
            "content": message.content,
        }
    payload: dict[str, Any] = {"role": message.role, "content": message.content}
    if message.tool_calls:
        payload["tool_calls"] = [
            {
                "id": call.id,
                "type": "function",
                "function": {"name": call.tool_name, "arguments": json.dumps(call.arguments)},
            }
            for call in message.tool_calls
        ]
    return payload


def _to_openai_tool(spec: ToolSpec) -> dict[str, Any]:
    return {
        "type": "function",
        "function": {
            "name": spec.name,
            "description": spec.description,
            "parameters": spec.parameters or {"type": "object", "properties": {}},
        },
    }
