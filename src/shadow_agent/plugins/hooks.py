"""Simple plugin hooks for tools, providers, events. MCP is reserved."""

from __future__ import annotations

import importlib.util
from collections.abc import Callable
from pathlib import Path
from typing import Any

from shadow_agent import paths
from shadow_agent.events import EventBus


class PluginRegistry:
    def __init__(self) -> None:
        self.tools: list[Callable] = []
        self.providers: list[Callable] = []
        self.event_hooks: list[Callable[[str, dict[str, Any]], None]] = []

    def on_event(self, fn: Callable[[str, dict[str, Any]], None]) -> None:
        self.event_hooks.append(fn)

    def load_from(self, directory: Path | None = None) -> int:
        root = directory or paths.plugins_dir()
        if not root.is_dir():
            return 0
        loaded = 0
        for path in sorted(root.glob("*.py")):
            spec = importlib.util.spec_from_file_location(f"shadow_plugin_{path.stem}", path)
            if spec is None or spec.loader is None:
                continue
            module = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(module)
            register = getattr(module, "register", None)
            if callable(register):
                register(self)
                loaded += 1
        return loaded

    def bind_events(self, bus: EventBus) -> None:
        for hook in self.event_hooks:
            bus.subscribe(None, hook)
