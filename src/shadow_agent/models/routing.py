"""Light multi-model routing: purpose → registered model id."""

from __future__ import annotations

from shadow_agent.config import AppConfig
from shadow_agent.models.interface import ModelProvider
from shadow_agent.models.registry import ModelRegistry


class ModelRouter:
    def __init__(self, config: AppConfig, registry: ModelRegistry | None = None) -> None:
        self.config = config
        self.registry = registry or ModelRegistry()

    def resolve_id(self, purpose: str = "coder") -> str:
        if not self.config.routing.enabled:
            return self.config.model.default
        table = {
            "planner": self.config.routing.planner,
            "coder": self.config.routing.coder,
            "reviewer": self.config.routing.reviewer,
            "tester": self.config.routing.tester,
            "researcher": self.config.routing.planner,
            "debugger": self.config.routing.coder,
        }
        return table.get(purpose, self.config.model.default)

    def provider_for(self, purpose: str = "coder") -> ModelProvider:
        return self.registry.create(self.config, self.resolve_id(purpose))
