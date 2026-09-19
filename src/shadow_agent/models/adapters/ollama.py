from __future__ import annotations

from shadow_agent.models.adapters.openai_compatible import OpenAICompatibleProvider


class OllamaProvider(OpenAICompatibleProvider):
    """Ollama's OpenAI-compatible endpoint (http://127.0.0.1:11434/v1)."""

    name = "ollama"

    def __init__(
        self,
        model: str = "llama3.2",
        endpoint: str = "http://127.0.0.1:11434/v1",
        api_key_env: str = "OLLAMA_API_KEY",
        context_limit: int = 128000,
    ) -> None:
        super().__init__(
            model=model or "llama3.2",
            endpoint=endpoint or "http://127.0.0.1:11434/v1",
            api_key_env=api_key_env,
            context_limit=context_limit,
        )
