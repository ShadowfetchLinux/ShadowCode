"""`shadow update` — self-updater against the public GitHub repository.

Flow:
  1. ``check_for_update``  — ask the GitHub API for the latest release (falls
     back to the newest tag) and compare it to the running version.
  2. ``apply_update``      — inside the source checkout the wrapper points at:
     ``git fetch --tags``, check out the release tag (or fast-forward ``main``),
     then re-run ``scripts/install-linux.sh`` so the wrapper, desktop entry,
     icons, and the built UI all match the new tree. Config, secrets, and the
     sessions DB are never touched.

Everything network-facing takes an injectable ``fetch`` so tests never leave
the machine.
"""

from __future__ import annotations

import json
import re
import subprocess
from pathlib import Path
from typing import Any, Callable

import httpx

from shadow_agent import __version__

REPO = "ShadowfetchLinux/ShadowCode"
API = "https://api.github.com/repos"

Fetcher = Callable[[str], dict[str, Any] | list[Any]]


def _default_fetch(url: str) -> dict[str, Any] | list[Any]:
    response = httpx.get(url, headers={"Accept": "application/vnd.github+json", "User-Agent": f"shadow-agent/{__version__}"}, timeout=8.0)
    response.raise_for_status()
    return response.json()


def version_tuple(text: str) -> tuple[int, ...]:
    """'v0.18.0' → (0, 18, 0). Non-numeric suffixes are dropped."""
    cleaned = text.strip().lstrip("vV")
    parts: list[int] = []
    for chunk in cleaned.split("."):
        match = re.match(r"\d+", chunk)
        if not match:
            break
        parts.append(int(match.group(0)))
    return tuple(parts) or (0,)


def check_for_update(current: str = __version__, repo: str = REPO, fetch: Fetcher | None = None) -> dict[str, Any]:
    """Return {current, latest, tag, update_available, url, source, error}."""
    fetch = fetch or _default_fetch
    out: dict[str, Any] = {
        "current": current,
        "latest": current,
        "tag": "",
        "update_available": False,
        "url": f"https://github.com/{repo}/releases",
        "source": "",
        "error": "",
    }
    try:
        release = fetch(f"{API}/{repo}/releases/latest")
        if isinstance(release, dict) and release.get("tag_name"):
            out["tag"] = str(release["tag_name"])
            out["latest"] = out["tag"].lstrip("vV")
            out["url"] = str(release.get("html_url") or out["url"])
            out["source"] = "release"
    except Exception as exc:  # noqa: BLE001 - fall through to tags
        out["error"] = f"{exc.__class__.__name__}: {exc}"
    if not out["tag"]:
        try:
            tags = fetch(f"{API}/{repo}/tags")
            if isinstance(tags, list) and tags:
                best = max((str(t.get("name", "")) for t in tags if isinstance(t, dict)), key=version_tuple, default="")
                if best:
                    out["tag"] = best
                    out["latest"] = best.lstrip("vV")
                    out["source"] = "tag"
                    out["error"] = ""
        except Exception as exc:  # noqa: BLE001
            out["error"] = out["error"] or f"{exc.__class__.__name__}: {exc}"
    out["update_available"] = version_tuple(out["latest"]) > version_tuple(current)
    return out


def source_root() -> Path:
    """The git checkout that the running package was imported from."""
    return Path(__file__).resolve().parents[2]


def _git(root: Path, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(["git", *args], cwd=root, capture_output=True, text=True, check=False)


def apply_update(
    tag: str = "",
    root: Path | None = None,
    reinstall: bool = True,
    runner: Callable[..., subprocess.CompletedProcess[str]] | None = None,
) -> dict[str, Any]:
    """Fetch and check out ``tag`` (or fast-forward main), then reinstall.

    Returns {ok, root, before, after, tag, steps, error}. Never raises for
    ordinary git/install failures — the CLI renders the dict.
    """
    root = Path(root or source_root())
    run = runner or (lambda *args, **kw: subprocess.run(*args, **kw))
    result: dict[str, Any] = {"ok": False, "root": str(root), "before": "", "after": "", "tag": tag, "steps": [], "error": ""}
    if not (root / ".git").exists():
        result["error"] = f"This is a packaged installation. Download the new release from https://github.com/{REPO}/releases/latest and run scripts/install-appimage.sh (or reinstall the wheel)."
        return result
    head = _git(root, "rev-parse", "--short", "HEAD")
    result["before"] = head.stdout.strip()
    dirty = _git(root, "status", "--porcelain")
    if dirty.stdout.strip():
        result["error"] = "working tree has uncommitted changes; commit or stash them before `shadow update`"
        return result
    fetch = _git(root, "fetch", "--tags", "--prune", "origin")
    result["steps"].append("git fetch --tags origin")
    if fetch.returncode != 0:
        result["error"] = (fetch.stderr or "git fetch failed").strip()
        return result
    if tag:
        checkout = _git(root, "checkout", "--quiet", tag)
        result["steps"].append(f"git checkout {tag}")
        if checkout.returncode != 0:
            result["error"] = (checkout.stderr or f"could not check out {tag}").strip()
            return result
    else:
        pull = _git(root, "pull", "--ff-only", "origin", "main")
        result["steps"].append("git pull --ff-only origin main")
        if pull.returncode != 0:
            result["error"] = (pull.stderr or "git pull failed").strip()
            return result
    result["after"] = _git(root, "rev-parse", "--short", "HEAD").stdout.strip()
    if reinstall:
        script = root / "scripts" / "install-linux.sh"
        if script.is_file():
            proc = run(["bash", str(script)], cwd=root, capture_output=True, text=True, check=False)
            result["steps"].append("scripts/install-linux.sh")
            if proc.returncode != 0:
                result["error"] = (proc.stderr or proc.stdout or "install script failed").strip()[-400:]
                return result
    result["ok"] = True
    return result


def render_check(info: dict[str, Any]) -> str:
    if info.get("update_available"):
        return f"ShadowCode {info['current']} → {info['latest']} available ({info.get('source') or 'github'}).\nRun `shadow update` to install it."
    suffix = f"  ({info['error']})" if info.get("error") else ""
    return f"ShadowCode {info['current']} is up to date.{suffix}"


def to_json(info: dict[str, Any]) -> str:
    return json.dumps(info, indent=2, default=str)
