from __future__ import annotations

from shadow_agent.config import AppConfig
from shadow_agent.models.adapters.llamacpp import LlamaCppProvider
from shadow_agent.models.adapters.local import LocalOpenAIProvider
from shadow_agent.models.adapters.mock import MockProvider
from shadow_agent.models.adapters.ollama import OllamaProvider
from shadow_agent.models.adapters.openai_compatible import OpenAICompatibleProvider
from shadow_agent.models.adapters.vllm import VLLMProvider
from shadow_agent.models.interface import ModelProvider
from shadow_agent.models.types import ModelInfo

BUILTIN_MODELS = [
    ModelInfo(id="mock", name="Mock Coder", provider="mock", context_limit=32000, metadata={"offline": True}),
    ModelInfo(
        id="grok",
        name="Grok (xAI)",
        provider="openai_compatible",
        endpoint="https://api.x.ai/v1",
        metadata={"api_key_env": "XAI_API_KEY", "model": "grok-4"},
    ),
    ModelInfo(
        id="openai",
        name="OpenAI-compatible",
        provider="openai_compatible",
        endpoint="https://api.openai.com/v1",
        metadata={"api_key_env": "OPENAI_API_KEY"},
    ),
    ModelInfo(
        id="local",
        name="Local OpenAI-compatible",
        provider="local",
        endpoint="http://127.0.0.1:1234/v1",
        metadata={"hint": "LM Studio / any local /v1"},
    ),
    ModelInfo(
        id="ollama",
        name="Ollama",
        provider="ollama",
        endpoint="http://127.0.0.1:11434/v1",
        metadata={"api_key_env": "OLLAMA_API_KEY"},
    ),
    ModelInfo(
        id="llamacpp",
        name="llama.cpp server",
        provider="llamacpp",
        endpoint="http://127.0.0.1:8080/v1",
    ),
    ModelInfo(
        id="vllm",
        name="vLLM",
        provider="vllm",
        endpoint="http://127.0.0.1:8000/v1",
    ),
]


class ModelRegistry:
    def __init__(self) -> None:
        self._info = {item.id: item for item in BUILTIN_MODELS}
        self._custom: dict[str, ModelInfo] = {}

    def list_models(self) -> list[ModelInfo]:
        merged = {**self._info, **self._custom}
        return list(merged.values())

    def get(self, model_id: str) -> ModelInfo | None:
        return self._custom.get(model_id) or self._info.get(model_id)

    def register(self, info: ModelInfo) -> None:
        self._custom[info.id] = info

    def create(self, config: AppConfig, model_id: str | None = None) -> ModelProvider:
        mid = model_id or config.model.default
        info = self.get(mid)
        provider = (info.provider if info else config.model.provider).lower()
        endpoint = config.model.endpoint or (info.endpoint if info else "")
        name = config.model.name or (info.metadata.get("model") if info else mid) or mid
        api_key_env = config.model.api_key_env
        if info and info.metadata.get("api_key_env") and not config.model.endpoint:
            api_key_env = str(info.metadata["api_key_env"])
        context_limit = config.model.context_limit or (info.context_limit if info else 128000)
        return build_provider(provider, name=name, endpoint=endpoint, api_key_env=api_key_env, context_limit=context_limit)


def build_provider(
    provider: str,
    *,
    name: str,
    endpoint: str,
    api_key_env: str,
    context_limit: int,
) -> ModelProvider:
    provider = provider.lower().replace("-", "_")
    if provider in {"mock", "test"}:
        return MockProvider(context_limit=context_limit)
    if provider in {"openai", "openai_compatible", "grok", "xai"}:
        return OpenAICompatibleProvider(
            model=name,
            endpoint=endpoint or "https://api.openai.com/v1",
            api_key_env=api_key_env,
            context_limit=context_limit,
        )
    if provider == "local":
        return LocalOpenAIProvider(model=name, endpoint=endpoint, api_key_env=api_key_env, context_limit=context_limit)
    if provider == "ollama":
        return OllamaProvider(model=name, endpoint=endpoint, api_key_env=api_key_env, context_limit=context_limit)
    if provider in {"llamacpp", "llama_cpp", "llama.cpp"}:
        return LlamaCppProvider(model=name, endpoint=endpoint, api_key_env=api_key_env, context_limit=context_limit)
    if provider == "vllm":
        return VLLMProvider(model=name, endpoint=endpoint, api_key_env=api_key_env, context_limit=context_limit)
    raise ValueError(f"unknown provider: {provider}")
