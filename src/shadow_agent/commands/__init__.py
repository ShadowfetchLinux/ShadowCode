"""Slash command system."""

from shadow_agent.commands.registry import (
    CommandRegistry,
    CommandResult,
    SlashCommand,
    builtin_commands,
    dispatch_builtin,
    load_project_commands,
)
from shadow_agent.commands.handlers import CommandContext, dispatch, handler_table

__all__ = [
    "CommandRegistry",
    "CommandResult",
    "SlashCommand",
    "CommandContext",
    "builtin_commands",
    "dispatch",
    "dispatch_builtin",
    "handler_table",
    "load_project_commands",
]
