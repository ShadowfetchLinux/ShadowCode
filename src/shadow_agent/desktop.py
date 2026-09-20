"""Launch the loopback Agent API and open a Brave/Chromium app window."""

from __future__ import annotations

import os
import sys
import json
import shutil
import subprocess
import time
import urllib.request
from pathlib import Path

from shadow_agent import paths, __version__
from shadow_agent.resources import resource_root
from shadow_agent.secrets import load_secrets


def launch_desktop(workspace: Path, host: str = "127.0.0.1", port: int = 7430) -> None:
    load_secrets()
    url = f"http://{host}:{port}"
    state = paths.state_dir()
    profile = state / "chrome-profile"
    log = state / "ui.log"
    profile.mkdir(parents=True, exist_ok=True)
    _ensure_ui_built()
    if _up(url):
        _open_browser(url, profile)
        return
    if getattr(sys, "frozen", False):
        # Keep the AppImage mount/extraction alive for the lifetime of the API.
        # A detached child would lose its bundled assets when the launcher exits.
        import threading
        from shadow_agent.api.server import serve

        def open_when_ready():
            for _ in range(120):
                if _up(url):
                    _open_browser(url, profile)
                    return
                time.sleep(0.15)

        threading.Thread(target=open_when_ready, daemon=True).start()
        serve(host, port, workspace)
        return
    env = os.environ.copy()
    env["SHADOW_AGENT_WORKSPACE"] = str(workspace)
    with log.open("a", encoding="utf-8") as handle:
        subprocess.Popen(
            ([sys.executable] if getattr(sys, "frozen", False) else [sys.executable, "-m", "shadow_agent"]) +
            ["ui", "--no-browser", "--host", host, "--port", str(port), "--project", str(workspace)],
            stdout=handle,
            stderr=handle,
            env=env,
            start_new_session=True,
            close_fds=True,
        )
    for _ in range(80):
        if _up(url):
            break
        time.sleep(0.15)
    else:
        raise RuntimeError(f"ShadowCode UI failed to start. See {log}")
    _open_browser(url, profile)


def _open_browser(url: str, profile: Path) -> None:
    browser = _browser()
    if not browser:
        subprocess.Popen(["xdg-open", url])
        return
    subprocess.Popen(
        [
            browser,
            f"--app={url}",
            f"--user-data-dir={profile}",
            "--class=shadow-agent",
            "--window-size=1560,980",
            "--window-name=ShadowCode",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-sync",
        ]
    )


def _up(url: str) -> bool:
    try:
        with urllib.request.urlopen(url + "/api/health", timeout=1) as resp:
            body = json.load(resp)
            return resp.status == 200 and body.get("app") == "ShadowCode" and body.get("version") == __version__
    except (OSError, ValueError):
        return False


def _browser() -> str | None:
    for name in (
        "brave-browser",
        "brave-browser-stable",
        "brave",
        "chromium-browser",
        "chromium",
        "google-chrome-stable",
        "google-chrome",
    ):
        found = shutil.which(name)
        if found:
            return found
    return None


def _ensure_ui_built() -> None:
    root = resource_root()
    dist = root / "ui" / "dist" / "index.html"
    if dist.is_file():
        return
    ui = root / "ui"
    if not (ui / "package.json").is_file():
        raise RuntimeError("Desktop assets are missing. Reinstall the ShadowCode release.")
    subprocess.run(["npm", "ci", "--no-fund", "--no-audit"], cwd=ui, check=True)
    subprocess.run(["npm", "run", "build"], cwd=ui, check=True)
