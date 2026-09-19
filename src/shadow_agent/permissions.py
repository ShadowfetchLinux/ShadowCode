"""Permission levels, command policy, and audit-friendly decisions."""

from __future__ import annotations

import re
from dataclasses import dataclass

from shadow_agent.config import PermissionLevel

READ_TOOLS = {
    "list_files",
    "read_file",
    "search_files",
    "search_text",
    "search_symbol",
    "git_status",
    "git_diff",
    "git_log",
    "git_branch",
    "update_plan",
    "update_todos",
}

WRITE_TOOLS = {
    "write_file",
    "edit_file",
    "create_directory",
    "move_file",
    "delete_file",
    "apply_patch",
    "exec",
    "kill",
    "git_add",
    "git_commit",
    "git_checkout",
}

ELEVATED_TOOLS = {
    "git_reset",
    "git_clean",
}

DANGEROUS_COMMAND = [
    re.compile(r"\bsudo\b"),
    re.compile(r"\brm\s+(-[a-zA-Z]*r[a-zA-Z]*f|-[a-zA-Z]*f[a-zA-Z]*r)\b"),
    re.compile(r"\bmkfs\b"),
    re.compile(r"\bdd\s+"),
    re.compile(r":\(\)\s*\{"),
    re.compile(r"\bchmod\s+(-R\s+)?777\b"),
    re.compile(r"(curl|wget).+\|\s*(ba)?sh"),
    re.compile(r"\b(shutdown|reboot|halt|poweroff)\b"),
    re.compile(r">\s*/dev/sd"),
    re.compile(r"\bgit\s+push\s+[^\n]*--force"),
    re.compile(r"\bgit\s+reset\s+--hard"),
    re.compile(r"\bgit\s+clean\s+-[a-zA-Z]*f"),
    re.compile(r"\bchown\s+-R\s+root\b"),
]

NETWORK_COMMAND = re.compile(r"\b(curl|wget|ssh|scp|rsync|nc|ncat|nmap|pip3?|uv|npm|pnpm|yarn)\b")
ROOT_COMMAND = re.compile(r"\bsudo\b|\b--privileged\b")


@dataclass
class PermissionDecision:
    allowed: bool
    reason: str
    needs_approval: bool = False
    elevated: bool = False


class PermissionGate:
    def __init__(
        self,
        level: PermissionLevel = PermissionLevel.WORKSPACE,
        *,
        require_approval_for_dangerous: bool = True,
        network: bool = False,
        allow_root: bool = False,
        auto_approve: bool = False,
    ) -> None:
        self.level = level
        self.require_approval_for_dangerous = require_approval_for_dangerous
        self.network = network
        self.allow_root = allow_root
        self.auto_approve = auto_approve

    def check_tool(self, tool_name: str) -> PermissionDecision:
        if tool_name in ELEVATED_TOOLS and self.level != PermissionLevel.ELEVATED:
            return PermissionDecision(False, f"{tool_name} requires elevated permissions")
        if tool_name in WRITE_TOOLS and self.level == PermissionLevel.READ_ONLY:
            return PermissionDecision(False, f"{tool_name} blocked in read-only mode")
        if tool_name in READ_TOOLS or tool_name in WRITE_TOOLS or tool_name in ELEVATED_TOOLS:
            return PermissionDecision(True, "ok")
        if self.level == PermissionLevel.READ_ONLY:
            return PermissionDecision(False, f"unknown tool {tool_name} blocked in read-only mode")
        return PermissionDecision(True, "ok")

    def check_command(self, command: str) -> PermissionDecision:
        if ROOT_COMMAND.search(command) and not self.allow_root:
            return PermissionDecision(False, "root/sudo is disabled")
        if any(pat.search(command) for pat in DANGEROUS_COMMAND):
            if self.level != PermissionLevel.ELEVATED:
                return PermissionDecision(
                    False,
                    "dangerous command requires elevated permissions",
                    needs_approval=True,
                    elevated=True,
                )
            if self.require_approval_for_dangerous and not self.auto_approve:
                return PermissionDecision(
                    False,
                    "dangerous command waiting for approval",
                    needs_approval=True,
                    elevated=True,
                )
        if not self.network and NETWORK_COMMAND.search(command):
            return PermissionDecision(False, "network commands are disabled")
        return PermissionDecision(True, "ok")
