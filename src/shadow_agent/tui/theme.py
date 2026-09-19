"""Codex-inspired color palette for light and dark themes.

Codex keeps the palette subtle: a near-black background, a single cyan/teal
accent, muted grays for chrome, and green/red only for diffs and status. No
neon. The dark theme is the default; the light theme mirrors Codex's paper
mode.
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class Theme:
    name: str
    bg: str
    panel: str
    border: str
    text: str
    muted: str
    dim: str
    accent: str
    accent_text: str
    ok: str
    warn: str
    danger: str
    add: str
    del_: str
    user_bg: str
    agent_bg: str

    def rich_style(self, role: str) -> str:
        return {
            "bg": self.bg,
            "panel": self.panel,
            "border": self.border,
            "text": self.text,
            "muted": self.muted,
            "dim": self.dim,
            "accent": self.accent,
            "accent_text": self.accent_text,
            "ok": self.ok,
            "warn": self.warn,
            "danger": self.danger,
            "add": self.add,
            "del": self.del_,
        }.get(role, self.text)


DARK = Theme(
    name="dark",
    bg="#0b0f14",
    panel="#111821",
    border="#1d2a34",
    text="#e6f2f7",
    muted="#8aa0ab",
    dim="#5d7380",
    accent="#6bd6f0",
    accent_text="#c8f6ff",
    ok="#3ddc84",
    warn="#f5c86b",
    danger="#ff6b7a",
    add="#3ddc84",
    del_="#ff6b7a",
    user_bg="#0e1a22",
    agent_bg="#0c141a",
)

LIGHT = Theme(
    name="light",
    bg="#f6f7f9",
    panel="#ffffff",
    border="#d6dde3",
    text="#1a232b",
    muted="#5a6b76",
    dim="#8a9aa3",
    accent="#0a7d8c",
    accent_text="#0a5a66",
    ok="#1a9f4e",
    warn="#a8730a",
    danger="#c0334a",
    add="#1a9f4e",
    del_="#c0334a",
    user_bg="#eaf2f6",
    agent_bg="#f1f4f7",
)


def load_theme(name: str) -> Theme:
    if name == "light":
        return LIGHT
    return DARK


def system_theme() -> str:
    """Best-effort detection of the OS color scheme. Returns 'dark' or 'light'."""
    import os

    try:
        import subprocess

        proc = subprocess.run(
            ["gsettings", "get", "org.gnome.desktop.interface", "color-scheme"],
            capture_output=True,
            text=True,
            timeout=1,
            check=False,
        )
        if proc.returncode == 0:
            value = proc.stdout.strip().strip("'\"")
            if "light" in value:
                return "light"
            if "dark" in value:
                return "dark"
    except (OSError, subprocess.SubprocessError):
        pass
    if os.environ.get("SHADOW_AGENT_THEME", "").lower() == "light":
        return "light"
    return "dark"
