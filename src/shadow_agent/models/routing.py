"""Visible model router: purpose → model id, with user overrides and fallback.

Purposes (the 7-pillar routing table):
    planning    → Kimi K3
    architecture→ GLM-5.2
    coding      → GLM-5.2
    small_edits → Qwen
    vision      → vision model
    review      → Grok 4.6
    local       → RTX local model

User overrides (sticky, persisted in config):
    /model kimi   → planning=architecture=coding=kimi-k3
    /model glm     → architecture=coding=glm-5.2
    /model local   → all purposes → local RTX model
    /model auto    → routing.enabled = True (use the table)
    /model <id>    → set default to <id>

If a named model isn't installed/registered, the router falls back to the
configured default and tells the user via the `routing.fallback` event.
"""

from __future__ import annotations

from typing import Any

from shadow_agent.config import AppConfig, RoutingConfig
from shadow_agent.models.interface import ModelProvider
from shadow_agent.models.registry import ModelRegistry


# Canonical default routing table (model-agnostic ids; resolved at runtime).
DEFAULT_PURPOSE_TABLE: dict[str, str] = {
    "planning": "kimi-k3",
    "architecture": "glm-5.2",
    "coding": "glm-5.2",
    "small_edits": "qwen3:14b",
    "vision": "vision",
    "review": "grok-4.6",
    "local": "local",
    "planner": "kimi-k3",  # legacy alias
    "coder": "glm-5.2",    # legacy alias
    "tester": "glm-5.2",   # legacy alias
    "reviewer": "grok-4.6",  # legacy alias
    "researcher": "kimi-k3",  # legacy alias
    "debugger": "glm-5.2",   # legacy alias
}

PURPOSES = ["planning", "architecture", "coding", "small_edits", "vision", "review", "local"]

# Named override presets the user can type: /model kimi, /model glm, /model local, /model auto.
# Keys are *purpose* names; they are mapped to RoutingConfig fields via _FIELD_FOR_PURPOSE.
OVERRIDE_PRESETS: dict[str, dict[str, str]] = {
    "kimi": {"planning": "kimi-k3", "architecture": "kimi-k3", "coding": "kimi-k3", "review": "kimi-k3"},
    "glm": {"architecture": "glm-5.2", "coding": "glm-5.2", "small_edits": "glm-5.2"},
    "qwen": {"small_edits": "qwen3:14b", "coding": "qwen3:14b"},
    "grok": {"review": "grok-4.6", "planning": "grok-4.6"},
    "local": {"planning": "local", "architecture": "local", "coding": "local", "small_edits": "local", "review": "local", "vision": "local", "local": "local"},
    "auto": {},  # enables routing.enabled and uses the default table
}

# Map purpose names (used in presets + the visible table) to RoutingConfig fields.
_FIELD_FOR_PURPOSE: dict[str, str] = {
    "planning": "planner",
    "architecture": "architecture",
    "coding": "coder",
    "small_edits": "small_edits",
    "vision": "vision",
    "review": "reviewer",
    "local": "local",
}


class ModelRouter:
    """Visible, override-aware router.

    Resolution order for a purpose:
      1. User override for that purpose (if set).
      2. Routing table entry (if routing.enabled).
      3. config.model.default.

    If the chosen id isn't registered, fall back to the default and emit a
    `routing.fallback` event with the reason.
    """

    def __init__(self, config: AppConfig, registry: ModelRegistry | None = None, emit=None) -> None:
        self.config = config
        self.registry = registry or ModelRegistry()
        self._emit = emit
        self.fallbacks: list[dict[str, str]] = []

    def _table(self) -> dict[str, str]:
        cfg = self.config.routing
        # User-set per-purpose values win; otherwise the canonical default.
        return {
            "planning": cfg.planner or DEFAULT_PURPOSE_TABLE["planning"],
            "architecture": cfg.architecture or DEFAULT_PURPOSE_TABLE["architecture"],
            "coding": cfg.coder or DEFAULT_PURPOSE_TABLE["coding"],
            "small_edits": cfg.small_edits or DEFAULT_PURPOSE_TABLE["small_edits"],
            "vision": cfg.vision or DEFAULT_PURPOSE_TABLE["vision"],
            "review": cfg.reviewer or DEFAULT_PURPOSE_TABLE["review"],
            "local": cfg.local or DEFAULT_PURPOSE_TABLE["local"],
            # legacy aliases
            "planner": cfg.planner or DEFAULT_PURPOSE_TABLE["planner"],
            "coder": cfg.coder or DEFAULT_PURPOSE_TABLE["coder"],
            "tester": cfg.tester or DEFAULT_PURPOSE_TABLE["tester"],
            "reviewer": cfg.reviewer or DEFAULT_PURPOSE_TABLE["reviewer"],
            "researcher": cfg.planner or DEFAULT_PURPOSE_TABLE["researcher"],
            "debugger": cfg.coder or DEFAULT_PURPOSE_TABLE["debugger"],
        }

    def resolve_id(self, purpose: str = "coder") -> str:
        if not self.config.routing.enabled:
            return self.config.model.default
        table = self._table()
        chosen = table.get(purpose, self.config.model.default)
        if chosen and self.registry.get(chosen) is None:
            # Try a fresh detection pass in case the model was just pulled.
            self.registry.refresh_detected()
        if chosen and self.registry.get(chosen) is None:
            # Fall back gracefully.
            fallback = self.config.model.default
            self.fallbacks.append({"purpose": purpose, "requested": chosen, "fallback": fallback})
            if self._emit:
                self._emit("routing.fallback", {"purpose": purpose, "requested": chosen, "fallback": fallback})
            return fallback
        return chosen

    def provider_for(self, purpose: str = "coder") -> ModelProvider:
        return self.registry.create(self.config, self.resolve_id(purpose))

    def table_view(self) -> dict[str, str]:
        """User-visible routing table (purpose → model id, or 'default')."""
        if not self.config.routing.enabled:
            return {p: self.config.model.default for p in PURPOSES}
        return {p: self._table().get(p, self.config.model.default) for p in PURPOSES}


def apply_override(config: AppConfig, name: str) -> tuple[AppConfig, str]:
    """Apply a `/model <name>` override preset. Returns (new_config, message).

    `name` is one of: kimi, glm, qwen, grok, local, auto, or a concrete model id.
    """
    name = (name or "").strip().lower()
    if name == "auto":
        config.routing.enabled = True
        return config, "Routing enabled (auto). Each purpose uses the default table."
    preset = OVERRIDE_PRESETS.get(name)
    if preset is not None:
        for purpose, mid in preset.items():
            field = _FIELD_FOR_PURPOSE.get(purpose, purpose)
            if hasattr(config.routing, field):
                setattr(config.routing, field, mid)
        config.routing.enabled = True
        return config, f"Override applied: {name}. Routing enabled."
    # Concrete model id: set as default and disable routing (single-model mode).
    config.model.default = name
    config.routing.enabled = False
    return config, f"Default model set to {name}. Routing disabled (single-model)."
