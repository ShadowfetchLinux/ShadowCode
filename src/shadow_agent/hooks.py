"""Deterministic lifecycle hooks.

Hooks fire at fixed points in the agent loop — the model does not have to
remember to invoke them. The built-in hooks wire common workflows
(ruff/format on after_edit, pytest on after_test, block-dangerous-commands
on before_command, notify on on_complete) so the agent gets them for free.

Hook sources, in precedence order:
  1. Built-in hooks (registered in code).
  2. Project hooks: ``.shadowcode/hooks/*.py`` (or legacy ``.shadow/hooks``).
     Each file defines ``register(registry: HookRegistry)`` and calls
     ``registry.on(event, handler)``.

Hook events:
  before_command  — before any exec/terminal tool runs (can block)
  after_edit      — after write_file / edit_file / apply_patch
  before_commit   — before git_commit
  after_test      — after a test command finishes
  on_error        — when a tool or model call errors
  on_complete     — when the agent loop completes a task
  on_compaction   — when the context engine compacts turns
"""

from __future__ import annotations

import importlib.util
import subprocess
from collections.abc import Callable
from dataclasses import dataclass, field
from enum import Enum
from pathlib import Path
from typing import Any

from shadow_agent import paths
from shadow_agent.models.types import ToolCall, ToolResult


class HookEvent(str, Enum):
    BEFORE_COMMAND = "before_command"
    AFTER_EDIT = "after_edit"
    BEFORE_COMMIT = "before_commit"
    AFTER_TEST = "after_test"
    ON_ERROR = "on_error"
    ON_COMPLETE = "on_complete"
    ON_COMPACTION = "on_compaction"


ALL_EVENTS = tuple(event.value for event in HookEvent)


@dataclass
class HookContext:
    workspace: Path
    event: str = ""
    tool: str = ""
    arguments: dict[str, Any] = field(default_factory=dict)
    result: ToolResult | None = None
    command: str = ""
    error: str = ""
    task_id: str = ""
    session_id: str = ""
    extra: dict[str, Any] = field(default_factory=dict)


@dataclass
class HookOutcome:
    name: str
    event: str
    block: bool = False
    message: str = ""
    output: str = ""
    success: bool = True

    def to_dict(self) -> dict[str, Any]:
        return {
            "name": self.name,
            "event": self.event,
            "block": self.block,
            "message": self.message,
            "output": self.output[:2000],
            "success": self.success,
        }


HookHandler = Callable[[HookContext], HookOutcome | None]


class Hook:
    def __init__(self, name: str, events: list[str], handler: HookHandler) -> None:
        self.name = name
        self.events = [HookEvent(e) if isinstance(e, str) else e for e in events]
        self.handler = handler

    def matches(self, event: str | HookEvent) -> bool:
        target = HookEvent(event) if isinstance(event, str) else event
        return target in self.events


class HookRegistry:
    """Registers and fires deterministic lifecycle hooks.

    Hooks fire in registration order. A hook may return ``block=True`` to
    stop the action (for before_* events) — the caller is responsible for
    honoring the block.
    """

    def __init__(self) -> None:
        self._hooks: list[Hook] = []

    def add(self, hook: Hook) -> None:
        self._hooks.append(hook)

    def on(self, events: list[str] | str, name: str | None = None) -> Callable[[HookHandler], HookHandler]:
        """Decorator: ``@registry.on("after_edit", "ruff-format")``."""
        if isinstance(events, str):
            events = [events]

        def decorator(handler: HookHandler) -> HookHandler:
            self.add(Hook(name or handler.__name__, list(events), handler))
            return handler

        return decorator

    def register(self, name: str, events: list[str] | str, handler: HookHandler) -> None:
        if isinstance(events, str):
            events = [events]
        self.add(Hook(name, list(events), handler))

    def fire(self, event: str | HookEvent, context: HookContext) -> list[HookOutcome]:
        target = HookEvent(event) if isinstance(event, str) else event
        context.event = target.value
        outcomes: list[HookOutcome] = []
        for hook in self._hooks:
            if not hook.matches(target):
                continue
            try:
                outcome = hook.handler(context)
            except Exception as exc:  # noqa: BLE001 — hooks must not crash the loop
                outcomes.append(HookOutcome(name=hook.name, event=target.value, block=False, success=False, message=f"hook error: {exc}"))
                continue
            if outcome is not None:
                outcomes.append(outcome)
        return outcomes

    def load_from(self, directory: Path | None = None) -> int:
        """Load ``.shadowcode/hooks/*.py`` (and legacy ``.shadow/hooks``)."""
        loaded = 0
        for root in _hook_dirs(directory):
            if not root.is_dir():
                continue
            for path in sorted(root.glob("*.py")):
                if path.name.startswith("_"):
                    continue
                spec = importlib.util.spec_from_file_location(f"shadow_hook_{path.stem}_{root.parent.name}", path)
                if spec is None or spec.loader is None:
                    continue
                module = importlib.util.module_from_spec(spec)
                try:
                    spec.loader.exec_module(module)
                except Exception:  # noqa: BLE001
                    continue
                register = getattr(module, "register", None)
                if callable(register):
                    register(self)
                    loaded += 1
        return loaded

    def names(self) -> list[str]:
        return [hook.name for hook in self._hooks]


def _hook_dirs(explicit: Path | None) -> list[Path]:
    if explicit is not None:
        return [explicit]
    # Project-local hooks; checked in priority order.
    candidates: list[Path] = []
    # We do not know the workspace here; the loop passes one via context.
    # For global hooks, fall back to the user config dir.
    candidates.append(paths.config_dir() / "hooks")
    return candidates


def project_hook_dirs(workspace: Path) -> list[Path]:
    return [
        Path(workspace) / ".shadowcode" / "hooks",
        Path(workspace) / ".shadow" / "hooks",
    ]


# --- Built-in hooks --------------------------------------------------------


def _run(workspace: Path, command: str, timeout: float = 30.0) -> tuple[bool, str]:
    try:
        proc = subprocess.run(
            command,
            shell=True,
            cwd=workspace,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
    except FileNotFoundError as exc:
        return False, str(exc)
    except subprocess.TimeoutExpired as exc:
        return False, f"timeout after {exc.timeout}s"
    out = (proc.stdout or "") + (proc.stderr or "")
    return proc.returncode == 0, out


def builtin_hooks() -> list[Hook]:
    """The four built-in hooks the spec calls out.

    Each is best-effort: if the underlying tool (ruff, pytest, notify-send)
    is not installed, the hook returns success and a "skipped" note instead
    of failing the agent action.
    """

    def ruff_format(ctx: HookContext) -> HookOutcome | None:
        path = str(ctx.arguments.get("path") or "")
        if not path:
            return None
        ok, out = _run(ctx.workspace, f"ruff format {path!r}", timeout=15.0)
        if "ruff: not found" in out or "command not found" in out or "not installed" in out:
            return HookOutcome(name="ruff-format", event=ctx.event, success=True, message="ruff not installed; skipped")
        return HookOutcome(name="ruff-format", event=ctx.event, success=ok, output=out, message="ruff format" if ok else "ruff format failed")

    def block_dangerous(ctx: HookContext) -> HookOutcome | None:
        from shadow_agent.permissions import DANGEROUS_COMMAND

        command = ctx.command or str(ctx.arguments.get("command") or "")
        if not command:
            return None
        for pat in DANGEROUS_COMMAND:
            if pat.search(command):
                return HookOutcome(
                    name="block-dangerous-commands",
                    event=ctx.event,
                    block=True,
                    success=False,
                    message=f"blocked dangerous command (matched {pat.pattern!r})",
                )
        return None

    def pytest_after_test(ctx: HookContext) -> HookOutcome | None:
        # Only re-run pytest if the agent itself didn't just run it.
        command = str(ctx.arguments.get("command") or ctx.command or "")
        if "pytest" in command:
            return None  # the agent already ran pytest; no need to double-run
        ok, out = _run(ctx.workspace, "python3 -m pytest -q", timeout=60.0)
        if "No tests" in out or "no tests ran" in out.lower():
            return HookOutcome(name="pytest-after-test", event=ctx.event, success=True, message="no tests; skipped")
        return HookOutcome(name="pytest-after-test", event=ctx.event, success=ok, output=out[-2000:])

    def notify_complete(ctx: HookContext) -> HookOutcome | None:
        import shutil
        import subprocess

        binary = shutil.which("notify-send")
        if not binary:
            return None
        title = "ShadowCode task complete"
        body = str(ctx.extra.get("summary") or "Task finished.")
        try:
            subprocess.Popen(  # noqa: S603
                [binary, "-a", "Shadow Agent", "-i", "shadow-agent", title, body[:160]],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
        except OSError:
            return None
        return HookOutcome(name="notify-on-complete", event=ctx.event, success=True, message="notified")

    return [
        Hook("ruff-format", [HookEvent.AFTER_EDIT], ruff_format),
        Hook("block-dangerous-commands", [HookEvent.BEFORE_COMMAND], block_dangerous),
        Hook("pytest-after-test", [HookEvent.AFTER_TEST], pytest_after_test),
        Hook("notify-on-complete", [HookEvent.ON_COMPLETE], notify_complete),
    ]


def default_registry(workspace: Path | None = None) -> HookRegistry:
    """Build a registry with built-ins + project hooks loaded."""
    registry = HookRegistry()
    for hook in builtin_hooks():
        registry.add(hook)
    if workspace is not None:
        for root in project_hook_dirs(workspace):
            if root.is_dir():
                registry.load_from(root)
    return registry


def outcomes_blocked(outcomes: list[HookOutcome]) -> bool:
    return any(o.block for o in outcomes)


def outcomes_summary(outcomes: list[HookOutcome]) -> str:
    if not outcomes:
        return ""
    bits = []
    for o in outcomes:
        tag = "✓" if o.success else "✗"
        bits.append(f"{tag} {o.name}" + (f": {o.message}" if o.message else ""))
    return "  ".join(bits)
