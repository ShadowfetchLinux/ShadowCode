from __future__ import annotations

from abc import ABC, abstractmethod
from collections.abc import Iterator

from shadow_agent.models.types import Capabilities, ChatRequest, ChatResponse, ToolCall, ToolResult


class ModelProvider(ABC):
    """Replaceable LLM. The harness owns everything else."""

    name: str = "base"

    @abstractmethod
    def generate(self, prompt: str, **kwargs: object) -> str:
        raise NotImplementedError

    @abstractmethod
    def stream(self, request: ChatRequest) -> Iterator[str]:
        raise NotImplementedError

    @abstractmethod
    def chat(self, request: ChatRequest) -> ChatResponse:
        raise NotImplementedError

    def tool_call(self, request: ChatRequest) -> list[ToolCall]:
        return self.chat(request).tool_calls

    @abstractmethod
    def get_capabilities(self) -> Capabilities:
        raise NotImplementedError

    @abstractmethod
    def get_context_limit(self) -> int:
        raise NotImplementedError

    def observe_tools(self, results: list[ToolResult]) -> None:
        """Optional hook after the harness executes tools."""
        return None
