"""Plugin install / list / remove.

A plugin packages commands + agents + MCP servers + hooks + skills into one
installable unit. Plugins live under ``~/.shadowcode/plugins/<name>/`` and
ship a ``plugin.yaml`` manifest:

    name: python-expert
    version: 1.0.0
    description: Python tooling: ruff format on edit, pytest on test
    commands:
      - ruff-format
      - pytest-watch
    agents:
      - python-expert
    mcp_servers:
      - name: filesystem
        command: ["python3", "-m", "shadow_agent.mcp_fs"]
    hooks:
      - file: after_edit.py
        events: [after_edit]
    skills:
      - python-build.md

The in-tree registry ships a couple of real plugins (python-expert,
linux-expert) so ``shadow plugin install python-expert`` works out of the box.
"""

from __future__ import annotations

import json
import shutil
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import yaml

from shadow_agent import paths


PLUGINS_DIR = paths.config_dir() / "shadowcode" / "plugins"


@dataclass
class PluginManifest:
    name: str
    version: str = "0.1.0"
    description: str = ""
    commands: list[str] = field(default_factory=list)
    agents: list[str] = field(default_factory=list)
    mcp_servers: list[dict[str, Any]] = field(default_factory=list)
    hooks: list[dict[str, Any]] = field(default_factory=list)
    skills: list[str] = field(default_factory=list)
    files: dict[str, str] = field(default_factory=dict)  # path -> content, written on install

    def to_dict(self) -> dict[str, Any]:
        return {
            "name": self.name,
            "version": self.version,
            "description": self.description,
            "commands": self.commands,
            "agents": self.agents,
            "mcp_servers": self.mcp_servers,
            "hooks": self.hooks,
            "skills": self.skills,
        }


# The in-tree registry. Each entry is a manifest with bundled files.
def _python_expert() -> PluginManifest:
    return PluginManifest(
        name="python-expert",
        version="1.0.0",
        description="Python tooling: ruff format on edit, pytest on test, lint on commit",
        commands=["ruff-format", "pytest-watch"],
        agents=["python-expert"],
        hooks=[
            {"file": "after_edit.py", "events": ["after_edit"]},
            {"file": "after_test.py", "events": ["after_test"]},
        ],
        skills=["python-build.md"],
        files={
            "after_edit.py": (
                "def register(registry):\n"
                "    @registry.on('after_edit', 'ruff-format')\n"
                "    def ruff_format(ctx):\n"
                "        import subprocess\n"
                "        path = ctx.arguments.get('path')\n"
                "        if not path or not str(path).endswith('.py'):\n"
                "            return None\n"
                "        proc = subprocess.run(['ruff', 'format', str(path)], cwd=ctx.workspace, capture_output=True, text=True)\n"
                "        from shadow_agent.hooks import HookOutcome\n"
                "        return HookOutcome(name='ruff-format', event=ctx.event, success=proc.returncode == 0, output=proc.stdout + proc.stderr)\n"
            ),
            "after_test.py": (
                "def register(registry):\n"
                "    @registry.on('after_test', 'pytest-watch')\n"
                "    def pytest_watch(ctx):\n"
                "        from shadow_agent.hooks import HookOutcome\n"
                "        return HookOutcome(name='pytest-watch', event=ctx.event, success=True, message='python-expert saw the test run')\n"
            ),
            "python-build.md": (
                "# Python Build Skill\n\n"
                "Steps:\n"
                "1. `python3 -m venv .venv`\n"
                "2. `pip install -e .`\n"
                "3. `ruff check .`\n"
                "4. `pytest -q`\n"
                "5. `python3 -m build` (if installed)\n"
            ),
        },
    )


def _linux_expert() -> PluginManifest:
    return PluginManifest(
        name="linux-expert",
        version="1.0.0",
        description="Linux packaging skills: apt, dpkg, AppImage, systemd units",
        commands=["apt-info", "appimage-build"],
        agents=["linux-expert"],
        skills=["appimage.md", "apt-package.md"],
        files={
            "appimage.md": (
                "# Build a Linux AppImage\n\n"
                "Steps:\n"
                "1. Build the binary (cargo build --release / pip install)\n"
                "2. Stage in AppDir/usr/bin/\n"
                "3. Drop a .desktop + icon in AppDir/\n"
                "4. Run appimagetool AppDir/ AppName-x86_64.AppImage\n"
                "5. Verify with ./AppImage --appimage-extract-and-run --version\n"
                "6. sha256sum the AppImage\n"
                "7. Upload to releases\n"
            ),
            "apt-package.md": (
                "# Build a .deb\n\n"
                "Steps:\n"
                "1. Layout: debian/DEBIAN/control, debian/usr/bin/...\n"
                "2. dpkg-deb --build debian\n"
                "3. lintian the .deb\n"
                "4. Install with apt install ./pkg.deb\n"
            ),
        },
    )


REGISTRY: dict[str, callable] = {  # type: ignore[type-arg]
    "python-expert": _python_expert,
    "linux-expert": _linux_expert,
    "posthog": lambda: PluginManifest(
        name="posthog",
        version="0.1.0",
        description="PostHog MCP server stub (configure .shadowcode/mcp/posthog.yaml)",
        mcp_servers=[{"name": "posthog", "url": "https://mcp.posthog.com/sse"}],
    ),
    "docker": lambda: PluginManifest(
        name="docker",
        version="0.1.0",
        description="Docker MCP server stub",
        mcp_servers=[{"name": "docker", "command": ["docker", "mcp", "server"]}],
    ),
    "ios": lambda: PluginManifest(
        name="ios",
        version="0.1.0",
        description="iOS tooling stub (xcodebuild wrappers)",
        skills=["ios-build.md"],
        files={"ios-build.md": "# iOS Build\n\n1. xcodebuild -scheme App -archivePath App.xcarchive archive\n2. xcodebuild -exportArchive ...\n"},
    ),
}


class PluginRegistry:
    """Installs / lists / removes plugins under ``~/.shadowcode/plugins/``."""

    def __init__(self, root: Path | None = None) -> None:
        self.root = root or PLUGINS_DIR
        self.root.mkdir(parents=True, exist_ok=True)

    def list_installed(self) -> list[PluginManifest]:
        out: list[PluginManifest] = []
        for child in sorted(self.root.iterdir()) if self.root.is_dir() else []:
            mpath = child / "plugin.yaml"
            if not mpath.is_file():
                continue
            try:
                data = yaml.safe_load(mpath.read_text(encoding="utf-8"))
            except (OSError, yaml.YAMLError):
                continue
            if not isinstance(data, dict):
                continue
            out.append(PluginManifest(
                name=str(data.get("name") or child.name),
                version=str(data.get("version") or "0.1.0"),
                description=str(data.get("description") or ""),
                commands=list(data.get("commands") or []),
                agents=list(data.get("agents") or []),
                mcp_servers=list(data.get("mcp_servers") or []),
                hooks=list(data.get("hooks") or []),
                skills=list(data.get("skills") or []),
            ))
        return out

    def list_registry(self) -> list[str]:
        return sorted(REGISTRY.keys())

    def install(self, name: str) -> PluginManifest:
        builder = REGISTRY.get(name)
        if builder is None:
            raise KeyError(f"unknown plugin: {name}")
        manifest = builder()
        target = self.root / manifest.name
        target.mkdir(parents=True, exist_ok=True)
        # Write manifest
        (target / "plugin.yaml").write_text(yaml.safe_dump(manifest.to_dict(), sort_keys=False), encoding="utf-8")
        # Write bundled files
        hooks_dir = target / "hooks"
        hooks_dir.mkdir(exist_ok=True)
        for hook in manifest.hooks:
            fname = hook.get("file")
            if fname and fname in manifest.files:
                (hooks_dir / fname).write_text(manifest.files[fname], encoding="utf-8")
        skills_dir = target / "skills"
        skills_dir.mkdir(exist_ok=True)
        for skill in manifest.skills:
            if skill in manifest.files:
                (skills_dir / skill).write_text(manifest.files[skill], encoding="utf-8")
        # Write any leftover files at the plugin root
        for fname, content in manifest.files.items():
            if fname.endswith(".py") and fname not in {h.get("file") for h in manifest.hooks}:
                (target / fname).write_text(content, encoding="utf-8")
        return manifest

    def remove(self, name: str) -> bool:
        target = self.root / name
        if not target.is_dir():
            return False
        shutil.rmtree(target, ignore_errors=True)
        return True

    def is_installed(self, name: str) -> bool:
        return (self.root / name / "plugin.yaml").is_file()
