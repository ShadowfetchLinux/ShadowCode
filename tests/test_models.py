from __future__ import annotations

from shadow_agent.config import AppConfig
from shadow_agent.models.adapters.mock import MockProvider
from shadow_agent.models.adapters.openai_compatible import OpenAICompatibleProvider
from shadow_agent.models.registry import ModelRegistry
from shadow_agent.models.types import ChatRequest, Message, ToolCall, ToolSpec


def test_mock_capabilities_and_generate():
    model = MockProvider()
    caps = model.get_capabilities()
    assert caps.tools and caps.provider == "mock"
    assert model.get_context_limit() >= 1000
    text = model.generate("just say hi")
    assert "Inspected" in text or "hi" in text.lower() or text


def test_mock_hello_emits_tool_calls():
    model = MockProvider()
    tools = [ToolSpec(name="list_files", description="", parameters={})]
    resp = model.chat(ChatRequest(messages=[Message(role="user", content="Create a Python hello-world project")], tools=tools))
    assert resp.tool_calls
    assert resp.tool_calls[0].tool_name == "list_files"


def test_registry_builds_mock_and_lists_builtins():
    registry = ModelRegistry()
    ids = {item.id for item in registry.list_models()}
    assert {"mock", "grok", "openai", "ollama", "llamacpp", "vllm", "local"} <= ids
    provider = registry.create(AppConfig())
    assert isinstance(provider, MockProvider)


def test_openai_payload_shape():
    provider = OpenAICompatibleProvider(model="grok-4", endpoint="https://api.x.ai/v1", api_key_env="XAI_API_KEY")
    payload = provider._payload(
        ChatRequest(
            messages=[Message(role="user", content="hi")],
            tools=[ToolSpec(name="list_files", description="list", parameters={"type": "object"})],
        ),
        stream=False,
    )
    assert payload["model"] == "grok-4"
    assert payload["tools"][0]["type"] == "function"
    parsed = provider._parse(
        {
            "choices": [
                {
                    "message": {
                        "content": "",
                        "tool_calls": [
                            {"id": "1", "function": {"name": "list_files", "arguments": "{\"path\": \".\"}"}}
                        ],
                    },
                    "finish_reason": "tool_calls",
                }
            ],
            "usage": {"prompt_tokens": 3},
        }
    )
    assert parsed.tool_calls[0] == ToolCall(id="1", tool_name="list_files", arguments={"path": "."})
    assert parsed.usage["prompt_tokens"] == 3
