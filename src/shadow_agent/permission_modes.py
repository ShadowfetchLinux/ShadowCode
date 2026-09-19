"""Named permission modes + per-permission grant matrix + destructive-action card.

The legacy ``PermissionLevel`` (read_only / workspace / elevated) is kept for
backward compat. This module layers Claude-Code-style named modes on top:

  PLAN        — read-only + planning tools only (no terminal, no writes)
  READ        — filesystem.read + terminal.safe (no writes)
  EDIT        — READ + filesystem.write + git.commit (prompted)
  DEVELOPER   — EDIT + terminal.destructive (prompted) + git.push (prompted)
  AUTONOMOUS  — DEVELOPER + auto-approve destructive + git.push allowed
  LOCKED      — nothing; the agent cannot act, only plan

Each mode maps to a per-permission grant matrix. The matrix is the source of
truth; the gate consults it. A grant can be:
  True  — allowed silently
  False — denied silently
  "ask" — needs an approval card

The destructive-action card is a structured payload the UI/CLI renders:

    🔴 DESTRUCTIVE ACTION
    rm -rf build/
    Reason: Agent requested cleanup
    [y] Allow once   [a] Always allow   [n] Deny
"""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum
from typing import Any


class PermissionMode(str, Enum):
    PLAN = "plan"
    READ = "read"
    EDIT = "edit"
    DEVELOPER = "developer"
    AUTONOMOUS = "autonomous"
    LOCKED = "locked"


# Per-permission grant matrix. Values: True / False / "ask".
# These are the *base* grants; the user can override per-session.
GRANT_MATRIX: dict[PermissionMode, dict[str, bool | str]] = {
    PermissionMode.PLAN: {
        "filesystem.read": True,
        "filesystem.write": False,
        "terminal.safe": False,
        "terminal.destructive": False,
        "git.commit": False,
        "git.push": False,
        "network": False,
    },
    PermissionMode.READ: {
        "filesystem.read": True,
        "filesystem.write": False,
        "terminal.safe": True,
        "terminal.destructive": False,
        "git.commit": False,
        "git.push": False,
        "network": "ask",
    },
    PermissionMode.EDIT: {
        "filesystem.read": True,
        "filesystem.write": True,
        "terminal.safe": True,
        "terminal.destructive": False,
        "git.commit": "ask",
        "git.push": False,
        "network": "ask",
    },
    PermissionMode.DEVELOPER: {
        "filesystem.read": True,
        "filesystem.write": True,
        "terminal.safe": True,
        "terminal.destructive": "ask",
        "git.commit": "ask",
        "git.push": "ask",
        "network": "ask",
    },
    PermissionMode.AUTONOMOUS: {
        "filesystem.read": True,
        "filesystem.write": True,
        "terminal.safe": True,
        "terminal.destructive": True,
        "git.commit": True,
        "git.push": True,
        "network": True,
    },
    PermissionMode.LOCKED: {
        "filesystem.read": False,
        "filesystem.write": False,
        "terminal.safe": False,
        "terminal.destructive": False,
        "git.commit": False,
        "git.push": False,
        "network": False,
    },
}


MODE_ORDER: tuple[PermissionMode, ...] = (
    PermissionMode.LOCKED,
    PermissionMode.PLAN,
    PermissionMode.READ,
    PermissionMode.EDIT,
    PermissionMode.DEVELOPER,
    PermissionMode.AUTONOMOUS,
)


def mode_for_level(level: str) -> PermissionMode:
    """Map a legacy PermissionLevel to a named mode (back-compat)."""
    mapping = {
        "read_only": PermissionMode.READ,
        "workspace": PermissionMode.EDIT,
        "elevated": PermissionMode.DEVELOPER,
    }
    return mapping.get(level, PermissionMode.EDIT)


def grant_for(mode: PermissionMode, permission: str) -> bool | str:
    return GRANT_MATRIX.get(mode, {}).get(permission, False)


@dataclass
class DestructiveCard:
    """A structured destructive-action approval card.

    Rendered by the CLI/TUI/desktop as:

        🔴 DESTRUCTIVE ACTION
        <action>
        Reason: <reason>
        [y] Allow once   [a] Always allow   [n] Deny
    """

    action: str
    reason: str
    permission: str = ""
    options: list[str] = field(default_factory=lambda: ["allow_once", "always_allow", "deny"])

    def to_dict(self) -> dict[str, Any]:
        return {
            "kind": "destructive_card",
            "icon": "🔴",
            "action": self.action,
            "reason": self.reason,
            "permission": self.permission,
            "options": self.options,
        }


def is_destructive_permission(permission: str) -> bool:
    return permission in {"terminal.destructive", "git.push", "git.commit"}


def render_destructive_card(card: DestructiveCard) -> str:
    """Plain-text rendering for the CLI / non-interactive output."""
    return (
        f"🔴 DESTRUCTIVE ACTION\n"
        f"{card.action}\n"
        f"Reason: {card.reason}\n"
        f"[y] Allow once   [a] Always allow   [n] Deny"
    )


def parse_decision(raw: str) -> str:
    """Map a user keystroke to one of the three options."""
    raw = raw.strip().lower()
    if raw in {"y", "yes", "allow_once", "allow-once", "allow once"}:
        return "allow_once"
    if raw in {"a", "always", "always_allow", "always-allow"}:
        return "always_allow"
    return "deny"
