"""Slash command registry and parsing.

Built-in commands live in code. Users add their own by dropping a
`.shadow/commands/<name>.md` (or `.yaml`) file in any workspace. A custom
command file is rendered into the system prompt as an extra instruction when
the user types `/<name>` so the agent behaves as if the user had typed that
text. This keeps the command surface extensible without a plugin runtime.

Built-in commands return a structured `CommandResult` (see handlers.py) so
any UI (TUI or desktop) can render them as Codex-style cards. Custom
commands expand to a prompt and run as a task (`passthrough=True`).
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
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
    # Argument spec for the composer autocomplete / help overlay.
    # "" = no arg, "<text>" = free-form arg hint.
    arg_spec: str = ""

    def render(self, args: str = "") -> str:
        """Return the prompt text this command expands to."""
        text = self.body.strip()
        if args:
            text = f"{text}\n\nAdditional context from user: {args}" if text else args
        return text


@dataclass
class CommandResult:
    """Structured result a builtin handler returns.

    `kind` selects the renderer:
      - text    : plain text block (default)
      - card    : Codex-style headline card (icon + headline + body)
      - list    : titled list of {label, value} rows
      - diff    : unified diff card (path + diff text)
      - approval: dangerous action awaiting a yes/no decision
      - error   : red error card
      - overlay : switch the UI overlay (help/models/sessions/settings)
      - quit    : exit the TUI / close the session
    """
    handled: bool = True
    text: str = ""
    kind: str = "text"
    icon: str = ""
    headline: str = ""
    body: str = ""
    items: list[dict[str, str]] = field(default_factory=list)
    diff: str = ""
    path: str = ""
    approval_action: str = ""
    approval_reason: str = ""
    overlay: str = ""
    quit: bool = False
    passthrough: bool = False
    metadata: dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> dict[str, Any]:
        return {
            "handled": self.handled,
            "text": self.text,
            "kind": self.kind,
            "icon": self.icon,
            "headline": self.headline,
            "body": self.body,
            "items": self.items,
            "diff": self.diff,
            "path": self.path,
            "approval_action": self.approval_action,
            "approval_reason": self.approval_reason,
            "overlay": self.overlay,
            "quit": self.quit,
            "passthrough": self.passthrough,
            "metadata": self.metadata,
        }


# Codex-style builtin command catalog. Handlers live in `commands.handlers`.
_BUILTIN_SPECS: list[dict[str, Any]] = [
    # --- brand / orientation ---
    {"name": "shadowcode", "description": "Brand intro: version, workspace, model, shortcuts", "arg_spec": ""},
    {"name": "help", "description": "List all slash commands and keybindings", "arg_spec": ""},
    {"name": "status", "description": "Agent + provider health: model, context, tokens, jobs", "arg_spec": ""},
    # --- model / context ---
    {"name": "model", "description": "Show the active model; with <id> switch to it", "arg_spec": "<id>"},
    {"name": "models", "description": "List all available/configured/detected models", "arg_spec": ""},
    {"name": "plan", "description": "Show the current plan + todos; with <text> replace it", "arg_spec": "<text>"},
    {"name": "compact", "description": "Manually compact the transcript now", "arg_spec": ""},
    {"name": "expand", "description": "Toggle the last tool card expanded/collapsed", "arg_spec": ""},
    {"name": "context", "description": "Context meter: tokens, files, recent tool results", "arg_spec": ""},
    # --- workspace / git ---
    {"name": "diff", "description": "Show the working-tree diff for the workspace", "arg_spec": "[path]"},
    {"name": "review", "description": "Self-review pending changes (lightweight)", "arg_spec": ""},
    {"name": "test", "description": "Run a test command; auto-detect if none given", "arg_spec": "<cmd>"},
    {"name": "run", "description": "Run a shell command in the workspace and stream output", "arg_spec": "<cmd>"},
    {"name": "git", "description": "Git status summary", "arg_spec": ""},
    {"name": "commit", "description": "Stage all + commit (message prompt or arg)", "arg_spec": "<message>"},
    {"name": "undo", "description": "Undo the last agent file change", "arg_spec": ""},
    # --- subagents / mcp / memory / settings ---
    {"name": "agents", "description": "List subagent hooks; with <role> dispatch to one", "arg_spec": "<role>"},
    {"name": "mcp", "description": "Show configured MCP servers/tools", "arg_spec": ""},
    {"name": "memory", "description": "Show project + task memory; with <text> append a note", "arg_spec": "<text>"},
    {"name": "settings", "description": "Open settings (desktop) / print config (TUI); with <key> <value> set", "arg_spec": "<key> <value>"},
    # --- diagnostics ---
    {"name": "hyperpod-nccl", "description": "Read-only HyperPod NCCL diagnostic flow (safe; prompts for cluster)", "arg_spec": "[cluster] [region]"},
    # --- legacy / session UX (kept for back-compat with the prior TUI) ---
    {"name": "clear", "description": "Clear the transcript scrollback", "arg_spec": ""},
    {"name": "new", "description": "Start a fresh session in this workspace", "arg_spec": ""},
    {"name": "branch", "description": "Fork the current session to try a path", "arg_spec": ""},
    {"name": "sessions", "description": "List recent sessions", "arg_spec": ""},
    {"name": "resume", "description": "Resume a session by id (prefix match)", "arg_spec": "<id>"},
    {"name": "pin", "description": "Pin/bookmark the last agent message", "arg_spec": ""},
    {"name": "cost", "description": "Show tokens and cost for this session", "arg_spec": ""},
    {"name": "doctor", "description": "Run install/config checks with auto-fix", "arg_spec": ""},
    {"name": "health", "description": "Ping the provider and detect tools", "arg_spec": ""},
    {"name": "ui", "description": "Open the desktop 4-panel UI", "arg_spec": ""},
    {"name": "quit", "description": "Exit the TUI", "arg_spec": ""},
    {"name": "exit", "description": "Alias of /quit", "arg_spec": ""},
    # --- ShadowCode 0.8.0 flagship 15-pillar commands ---
    {"name": "understand", "description": "Analyze the repo and save a project map to memory", "arg_spec": ""},
    {"name": "goal", "description": "Turn a one-line instruction into a goal with milestones", "arg_spec": "<task>"},
    {"name": "goals", "description": "List goals for this workspace", "arg_spec": ""},
    {"name": "team", "description": "Run a multi-agent team (LEAD + ARCHITECT/CODER/TESTER/SECURITY/REVIEWER)", "arg_spec": "<task>"},
    {"name": "router", "description": "Show the model routing table (purpose → model)", "arg_spec": ""},
    {"name": "why", "description": "Explain a change: files changed, reason, related commits", "arg_spec": "[path|task-id]"},
    {"name": "vision", "description": "Send a screenshot to a vision model for debugging", "arg_spec": "<image-path>"},
    {"name": "docs", "description": "Research docs and plan a migration", "arg_spec": "<task> [url]"},
    {"name": "tools", "description": "List installed + available tools (marketplace)", "arg_spec": ""},
    {"name": "profile", "description": "Show or switch the permission profile (safe/developer/autonomous/locked)", "arg_spec": "[name]"},
    {"name": "rollback", "description": "Restore files from a named checkpoint", "arg_spec": "<name>"},
    {"name": "checkpoints", "description": "List named checkpoints in this workspace", "arg_spec": ""},
]


def builtin_commands() -> list[SlashCommand]:
    return [
        SlashCommand(
            spec["name"],
            spec.get("description", ""),
            "",
            "",
            "builtin",
            spec.get("arg_spec", ""),
        )
        for spec in _BUILTIN_SPECS
    ]


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
        return CommandResult(handled=False, text=f"Unknown command: /{name}", kind="error")
    return handler(args)
