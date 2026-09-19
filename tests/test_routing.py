from __future__ import annotations

from shadow_agent.config import AppConfig, RoutingConfig
from shadow_agent.models.adapters.mock import MockProvider
from shadow_agent.models.routing import ModelRouter


def test_router_defaults_and_purposes():
    cfg = AppConfig()
    router = ModelRouter(cfg)
    assert router.resolve_id("coder") == "mock"
    cfg.routing = RoutingConfig(enabled=True, planner="grok", coder="ollama", tester="mock")
    router = ModelRouter(cfg)
    assert router.resolve_id("planner") == "grok"
    assert router.resolve_id("coder") == "ollama"
    assert isinstance(router.provider_for("tester"), MockProvider)
