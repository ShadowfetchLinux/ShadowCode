"""Slash command system.

Built-in commands live in code. Users add their own by dropping a
`.shadow/commands/<name>.md` (or `.yaml`) file in any workspace. A custom
command file is rendered into the system prompt as an extra instruction when
the user types `/<name>` so the agent behaves as if the user had typed that
text. This keeps the command surface extensible without a plugin runtime.

A command file may begin with YAML front matter:

    ---
    description: Re-run the test suite and summarize failures
    alias: rt
    ---
    Run `python3 -m pytest -q` and summarize the failures.

If no front matter is present the whole file is the prompt body and the name
is derived from the file stem.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable

import yaml


@dataclass
class SlashCommand:
    name: str
    description: str = ""
    body: str = ""
    alias: str = ""
    source: str = "builtin"  # builtin | project | user

    def render(self, args: str = "") -> str:
        """Return the prompt text this command expands to."""
        text = self.body.strip()
        if args:
            text = f"{text}\n\nAdditional context from user: {args}" if text else args
        return text


@dataclass
class CommandResult:
    handled: bool
    text: str = ""
    quit: bool = False
    passthrough: bool = False  # True when the expanded text should be run as a task


_BUILTIN_SPECS: list[dict[str, Any]] = [
    {"name": "help", "description": "Show available slash commands and keybindings", "builtin": True},
    {"name": "clear", "description": "Clear the transcript scrollback", "builtin": True},
    {"name": "new", "description": "Start a fresh session in this workspace", "builtin": True},
    {"name": "model", "description": "Show or switch the active model", "builtin": True},
    {"name": "models", "description": "Alias of /model", "builtin": True},
    {"name": "config", "description": "Show or set config values", "builtin": True},
    {"name": "compact", "description": "Summarize and compact the transcript now", "builtin": True},
    {"name": "expand", "description": "Expand/collapse the last tool card", "builtin": True},
    {"name": "undo", "description": "Undo the last agent file changes", "builtin": True},
    {"name": "diff", "description": "Show the working-tree diff", "builtin": True},
    {"name": "git", "description": "Show git status and recent log", "builtin": True},
    {"name": "branch", "description": "Fork the current session to try a path", "builtin": True},
    {"name": "sessions", "description": "List recent sessions", "builtin": True},
    {"name": "resume", "description": "Resume a session by id (prefix match)", "builtin": True},
    {"name": "pin", "description": "Pin/bookmark the last agent message", "builtin": True},
    {"name": "cost", "description": "Show tokens and cost for this session", "builtin": True},
    {"name": "doctor", "description": "Run install/config checks with auto-fix", "builtin": True},
    {"name": "health", "description": "Ping the provider and detect tools", "builtin": True},
    {"name": "ui", "description": "Open the desktop 4-panel UI", "builtin": True},
    {"name": "quit", "description": "Exit the TUI", "builtin": True},
    {"name": "exit", "description": "Alias of /quit", "builtin": True},
]


def builtin_commands() -> list[SlashCommand]:
    return [SlashCommand(spec["name"], spec.get("description", ""), "", "", "builtin") for spec in _BUILTIN_SPECS]


def _parse_front_matter(text: str) -> tuple[dict[str, Any], str]:
    match = re.match(r"^---\s*\n(.*?)\n---\s*\n?(.*)$", text, re.DOTALL)
    if not match:
        return {}, text
    try:
        meta = yaml.safe_load(match.group(1)) or {}
    except yaml.YAMLError:
        return {}, text
    if not isinstance(meta, dict):
        return {}, text
    return meta, match.group(2).strip()


def load_project_commands(workspace: Path) -> list[SlashCommand]:
    """Load `.shadow/commands/*.md` and `*.yaml` files from the workspace."""
    out: list[SlashCommand] = []
    root = Path(workspace) / ".shadow" / "commands"
    if not root.is_dir():
        return out
    for path in sorted(root.glob("*.md")) + sorted(root.glob("*.yaml")) + sorted(root.glob("*.yml")):
        try:
            text = path.read_text(encoding="utf-8")
        except OSError:
            continue
        meta, body = _parse_front_matter(text) if path.suffix == ".md" else ({}, text)
        name = str(meta.get("name") or path.stem).strip()
        if not name or "/" in name:
            continue
        out.append(
            SlashCommand(
                name=name,
                description=str(meta.get("description") or ""),
                body=body or text,
                alias=str(meta.get("alias") or ""),
                source="project",
            )
        )
    return out


class CommandRegistry:
    """Resolves `/name args` → SlashCommand, including user-defined ones."""

    def __init__(self, workspace: Path | None = None) -> None:
        self.workspace = Path(workspace) if workspace else None
        self._builtins = {cmd.name: cmd for cmd in builtin_commands()}
        self._project: dict[str, SlashCommand] = {}
        self._aliases: dict[str, str] = {}
        self.reload()

    def reload(self) -> None:
        self._project.clear()
        self._aliases.clear()
        if self.workspace is not None:
            for cmd in load_project_commands(self.workspace):
                self._project[cmd.name] = cmd
                if cmd.alias:
                    self._aliases[cmd.alias] = cmd.name

    def list(self) -> list[SlashCommand]:
        merged: dict[str, SlashCommand] = {}
        for cmd in self._builtins.values():
            merged[cmd.name] = cmd
        for name, cmd in self._project.items():
            merged[name] = cmd
        return sorted(merged.values(), key=lambda c: c.name)

    def get(self, name: str) -> SlashCommand | None:
        if name in self._project:
            return self._project[name]
        target = self._aliases.get(name, name)
        return self._builtins.get(target) or self._project.get(target)

    def is_builtin(self, name: str) -> bool:
        target = self._aliases.get(name, name)
        return target in self._builtins

    def parse(self, line: str) -> tuple[SlashCommand | None, str, str]:
        """Split a `/name args` line. Returns (command_or_None, name, args)."""
        line = line.strip()
        if not line.startswith("/"):
            return None, "", line
        parts = line[1:].split(None, 1)
        name = parts[0] if parts else ""
        args = parts[1] if len(parts) > 1 else ""
        return self.get(name), name, args


def dispatch_builtin(
    name: str,
    args: str,
    *,
    handlers: dict[str, Callable[[str], CommandResult]],
) -> CommandResult:
    """Run a builtin command through a handler table. Custom commands are
    handled by the caller (they expand to a prompt and run as a task)."""
    handler = handlers.get(name)
    if handler is None:
        return CommandResult(handled=False, text=f"Unknown command: /{name}")
    return handler(args)
