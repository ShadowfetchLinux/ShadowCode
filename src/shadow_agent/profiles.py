"""Permission profiles — SAFE / DEVELOPER / AUTONOMOUS / LOCKED.

Each profile maps to a PermissionLevel + flags (require_approval_for_dangerous,
network, allow_root). Destructive commands (`rm -rf build/`) always prompt with
`Allow? [y/N]` unless the profile is AUTONOMOUS (and even then, only if the
user has explicitly enabled autonomous-destructive).

Profiles are sticky (persisted in config) and switchable via
`/profile safe|developer|autonomous|locked` or `shadow profile <name>`.
"""

from __future__ import annotations

from dataclasses import dataclass

from shadow_agent.config import AppConfig, PermissionLevel


@dataclass(frozen=True)
class PermissionProfile:
    name: str
    level: PermissionLevel
    require_approval_for_dangerous: bool
    network: bool
    allow_root: bool
    description: str


PROFILES: dict[str, PermissionProfile] = {
    "safe": PermissionProfile(
        name="safe",
        level=PermissionLevel.READ_ONLY,
        require_approval_for_dangerous=True,
        network=False,
        allow_root=False,
        description="Read-only. No writes, no exec, no network. Safe to point at any repo.",
    ),
    "developer": PermissionProfile(
        name="developer",
        level=PermissionLevel.WORKSPACE,
        require_approval_for_dangerous=True,
        network=True,
        allow_root=False,
        description="Workspace writes + exec. Network on. Destructive commands prompt with Allow? [y/N].",
    ),
    "autonomous": PermissionProfile(
        name="autonomous",
        level=PermissionLevel.ELEVATED,
        require_approval_for_dangerous=False,
        network=True,
        allow_root=False,
        description="Elevated. Destructive commands run without prompting. Use only in sandboxes.",
    ),
    "locked": PermissionProfile(
        name="locked",
        level=PermissionLevel.READ_ONLY,
        require_approval_for_dangerous=True,
        network=False,
        allow_root=False,
        description="Hard lock: read-only, no network, no approvals. Like SAFE but cannot be escaped by a tool.",
    ),
}


def apply_profile(config: AppConfig, name: str) -> tuple[AppConfig, PermissionProfile]:
    """Apply a named profile to the config. Returns (new_config, profile)."""
    profile = PROFILES.get(name.lower())
    if profile is None:
        raise ValueError(f"unknown profile: {name}. Choose one of: {', '.join(PROFILES)}")
    config.permissions.level = profile.level
    config.permissions.require_approval_for_dangerous = profile.require_approval_for_dangerous
    config.permissions.network = profile.network
    config.permissions.allow_root = profile.allow_root
    config.permissions.profile = profile.name
    return config, profile


def current_profile(config: AppConfig) -> str:
    """Return the named profile if set explicitly, else best-effort reverse map."""
    explicit = getattr(config.permissions, "profile", "")
    if explicit:
        return explicit
    for name, profile in PROFILES.items():
        if (
            config.permissions.level == profile.level
            and config.permissions.require_approval_for_dangerous == profile.require_approval_for_dangerous
            and config.permissions.network == profile.network
            and config.permissions.allow_root == profile.allow_root
        ):
            return name
    return "custom"


def render_profiles(current: str) -> str:
    lines = ["Permission profiles", ""]
    for name, profile in PROFILES.items():
        mark = "●" if name == current else "○"
        lines.append(f"{mark} {name:12}  {profile.description}")
    lines.append("")
    lines.append(f"Current: {current}")
    lines.append("Switch with: /profile safe|developer|autonomous|locked")
    return "\n".join(lines)
