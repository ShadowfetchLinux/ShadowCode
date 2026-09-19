"""User-only API keys. Never stored in the repo or in config.yaml."""

from __future__ import annotations

import os
import re
from pathlib import Path

from shadow_agent import paths

_NAME = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")


def secrets_file() -> Path:
    return paths.secrets_file()


def load_secrets() -> dict[str, str]:
    """Read ~/.config/shadow-agent/secrets.env and export missing keys into os.environ."""
    path = secrets_file()
    found: dict[str, str] = {}
    if not path.is_file():
        return found
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        key = key.strip()
        value = value.strip().strip('"').strip("'")
        if not _NAME.match(key):
            continue
        found[key] = value
        os.environ.setdefault(key, value)
    return found


def secret_names() -> list[str]:
    return sorted(load_secrets())


def has_secret(name: str) -> bool:
    return bool(os.environ.get(name) or load_secrets().get(name))


def set_secret(name: str, value: str) -> None:
    if not _NAME.match(name):
        raise ValueError("API key environment name must look like XAI_API_KEY")
    if not value.strip():
        raise ValueError("API key is empty")
    existing = load_secrets()
    existing[name] = value.strip()
    _write(existing)
    os.environ[name] = value.strip()


def delete_secret(name: str) -> None:
    existing = load_secrets()
    existing.pop(name, None)
    _write(existing)
    os.environ.pop(name, None)


def _write(values: dict[str, str]) -> None:
    path = secrets_file()
    lines = [f"{key}={values[key]}\n" for key in sorted(values)]
    path.write_text("".join(lines), encoding="utf-8")
    path.chmod(0o600)
