"""Provider ping plus local toolchain detection (git / python / docker / node)."""

from __future__ import annotations

import os
import shutil
import socket
import sys
from pathlib import Path
from typing import Any

import httpx

from shadow_agent import __version__, paths
from shadow_agent.config import AppConfig
from shadow_agent.errors import friendly_error
from shadow_agent.secrets import has_secret, load_secrets


def collect_health(config: AppConfig, workspace: Path | None = None) -> dict[str, Any]:
    load_secrets()
    tools = {
        "python": _which_version("python3") or {"ok": True, "path": sys.executable, "detail": sys.version.split()[0]},
        "git": _which_version("git"),
        "docker": _which_version("docker"),
        "node": _which_version("node"),
        "npm": _which_version("npm"),
        "rg": _which_version("rg"),
    }
    provider = ping_provider(config)
    workspace_path = str(workspace) if workspace else ""
    return {
        "ok": True,
        "name": "shadow-agent",
        "version": __version__,
        "workspace": workspace_path,
        "provider": provider,
        "tools": tools,
        "secrets": {"configured": secret_names_present(config), "file": str(paths.secrets_file())},
        "permissions": config.permissions.model_dump(mode="json"),
        "model": {
            "default": config.model.default,
            "provider": config.model.provider,
            "endpoint": config.model.endpoint,
            "name": config.model.name,
            "api_key_env": config.model.api_key_env,
            "key_present": has_secret(config.model.api_key_env) if config.model.provider != "mock" else True,
        },
        "ui": {"host": config.ui.host, "port": config.ui.port, "theme": config.ui.theme},
        "onboarding": config.onboarding.model_dump(mode="json"),
    }


def secret_names_present(config: AppConfig) -> list[str]:
    names = {config.model.api_key_env, "OPENAI_API_KEY", "XAI_API_KEY", "OLLAMA_API_KEY"}
    return sorted(name for name in names if name and has_secret(name))


def ping_provider(config: AppConfig) -> dict[str, Any]:
    provider = (config.model.provider or "mock").lower()
    if provider in {"mock", "test"}:
        return {"ok": True, "name": "mock", "detail": "Offline mock provider. Ready without an API key."}
    endpoint = (config.model.endpoint or "").rstrip("/")
    if not endpoint:
        return {"ok": False, "name": provider, "detail": "No model endpoint is configured."}
    url = endpoint if endpoint.endswith("/models") else endpoint + ("/models" if endpoint.endswith("/v1") else "/v1/models")
    headers = {"Accept": "application/json"}
    key_name = config.model.api_key_env
    key = os.environ.get(key_name, "")
    if key:
        headers["Authorization"] = f"Bearer {key}"
    try:
        response = httpx.get(url, headers=headers, timeout=3.0)
        if response.status_code in {401, 403}:
            return {
                "ok": False,
                "name": provider,
                "detail": "The endpoint answered, but the API key was rejected.",
                "status": response.status_code,
            }
        if response.status_code >= 500:
            return {
                "ok": False,
                "name": provider,
                "detail": f"The endpoint returned HTTP {response.status_code}.",
                "status": response.status_code,
            }
        return {
            "ok": True,
            "name": provider,
            "detail": f"Reached {url} (HTTP {response.status_code}).",
            "status": response.status_code,
        }
    except httpx.ConnectError:
        return {"ok": False, "name": provider, "detail": f"Nothing is listening at {endpoint}."}
    except Exception as exc:
        return {"ok": False, "name": provider, "detail": friendly_error(exc)}


def _which_version(binary: str) -> dict[str, Any]:
    found = shutil.which(binary)
    if not found:
        return {"ok": False, "path": "", "detail": f"{binary} not on PATH"}
    detail = found
    try:
        import subprocess

        proc = subprocess.run([found, "--version"], capture_output=True, text=True, timeout=2, check=False)
        line = (proc.stdout or proc.stderr or "").splitlines()
        if line:
            detail = line[0][:120]
    except Exception:
        pass
    return {"ok": True, "path": found, "detail": detail}


def port_open(host: str, port: int) -> bool:
    try:
        with socket.create_connection((host, port), timeout=0.3):
            return True
    except OSError:
        return False
