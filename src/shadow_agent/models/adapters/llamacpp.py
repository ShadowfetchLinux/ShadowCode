from __future__ import annotations

from shadow_agent.models.adapters.openai_compatible import OpenAICompatibleProvider


class LlamaCppProvider(OpenAICompatibleProvider):
    name = "llamacpp"

    def __init__(
        self,
        model: str = "local",
        endpoint: str = "http://127.0.0.1:8080/v1",
        api_key_env: str = "OPENAI_API_KEY",
        context_limit: int = 32000,
    ) -> None:
        super().__init__(
            model=model,
            endpoint=endpoint or "http://127.0.0.1:8080/v1",
            api_key_env=api_key_env,
            context_limit=context_limit,
        )
