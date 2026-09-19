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
from shadow_agent.models.discovery import detect_providers
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


def doctor_report(config: AppConfig, workspace: Path | None = None) -> dict[str, Any]:
    """Deeper install/config checks with auto-fix suggestions. Each check:
    {id, ok, label, detail, fix}."""
    load_secrets()
    checks: list[dict[str, Any]] = []

    def add(check_id: str, ok: bool, label: str, detail: str = "", fix: str = "") -> None:
        checks.append({"id": check_id, "ok": bool(ok), "label": label, "detail": detail, "fix": fix if not ok else ""})

    # Python version
    py_ok = sys.version_info >= (3, 10)
    add("python", py_ok, "Python >= 3.10", sys.version.split()[0], "Install Python 3.12 (sudo apt install python3.12).")

    # Config file parses
    cfg_file = paths.config_file()
    try:
        from shadow_agent.config import load_config as _load

        _load()
        add("config", True, "config.yaml parses", str(cfg_file))
    except Exception as exc:  # noqa: BLE001
        add("config", False, "config.yaml parses", str(exc), f"Fix or delete {cfg_file} and run `shadow ui` to regenerate.")

    # Secrets file permissions
    sec_file = paths.secrets_file()
    if sec_file.is_file():
        mode = sec_file.stat().st_mode & 0o777
        add(
            "secrets-perms",
            mode == 0o600,
            "secrets.env is mode 600",
            oct(mode),
            f"Run: chmod 600 {sec_file}",
        )
    else:
        add("secrets-perms", True, "secrets.env is mode 600", "no secrets file yet")

    # CLI wrapper installed
    wrapper = Path.home() / ".local" / "bin" / "shadow"
    wrapper_ok = wrapper.is_file() and os.access(wrapper, os.X_OK)
    add("wrapper", wrapper_ok, "~/.local/bin/shadow installed", str(wrapper), "Run scripts/install-linux.sh from the repo.")

    # Desktop entry + icon
    desktop = Path.home() / ".local" / "share" / "applications" / "shadow-agent.desktop"
    desktop_ok = desktop.is_file() and "Icon=shadow-agent" in desktop.read_text(encoding="utf-8", errors="ignore")
    add("desktop-entry", desktop_ok, "Desktop entry with Icon=shadow-agent", str(desktop), "Run scripts/install-linux.sh to reinstall the launcher.")
    icon_dir = Path.home() / ".local" / "share" / "icons" / "hicolor" / "scalable" / "apps" / "shadow-agent.svg"
    add("icon", icon_dir.is_file(), "App icon installed", str(icon_dir), "Run scripts/install-linux.sh to reinstall icons.")

    # UI port
    port = int(config.ui.port)
    if port_open(config.ui.host, port):
        add("port", True, f"Port {port} reachable", "UI server appears to be running")
    else:
        add("port", True, f"Port {port} free", "nothing bound; `shadow ui` can start")

    # Provider ping
    provider = ping_provider(config)
    add("provider", provider["ok"], f"Provider {provider['name']}", provider.get("detail", ""), _provider_fix(config))

    # Local server discovery (only fails when the configured provider is a local one that is down)
    detected = detect_providers()
    running = [d for d in detected if d.running]
    local_providers = {"ollama", "local", "llamacpp", "vllm"}
    if config.model.provider in local_providers:
        match = next((d for d in running if d.provider == config.model.provider), None)
        if match is None:
            add(
                "local-server",
                False,
                f"Local server for {config.model.provider}",
                "no server detected",
                _provider_fix(config),
            )
        else:
            names = [m.id for m in match.models]
            want = config.model.name
            if want and names and want not in names:
                add(
                    "local-model",
                    False,
                    f"Model {want} installed",
                    f"available: {', '.join(names)}",
                    f"Pull it (e.g. `ollama pull {want}`) or pick one in Settings.",
                )
            else:
                add("local-model", True, f"Model {want or '(any)'} installed", ", ".join(names) or "server reports no models")
    else:
        add("local-server", True, "Local model servers", f"{len(running)} running" if running else "none running (optional)")

    # Workspace writable
    if workspace is not None:
        ws = Path(workspace)
        writable = ws.is_dir() and os.access(ws, os.W_OK)
        add("workspace", writable, "Workspace writable", str(ws), "Open an existing, writable folder.")
        db_file = paths.db_file()
        try:
            db_file.parent.mkdir(parents=True, exist_ok=True)
            with db_file.open("a", encoding="utf-8"):
                pass
            add("state-db", True, "State DB writable", str(db_file))
        except OSError as exc:
            add("state-db", False, "State DB writable", str(exc), f"Check permissions on {db_file.parent}.")

    ok = all(item["ok"] for item in checks)
    suggestions = [f"{item['label']}: {item['fix']}" for item in checks if not item["ok"] and item["fix"]]
    return {"ok": ok, "version": __version__, "checks": checks, "suggestions": suggestions}


def _provider_fix(config: AppConfig) -> str:
    provider = (config.model.provider or "").lower()
    if provider == "ollama":
        return "Start Ollama (`ollama serve` or the systemd user service) and pull a model, e.g. `ollama pull qwen3:14b`."
    if provider == "local":
        return "Start LM Studio (or any local /v1 server) on port 1234, or switch provider in Settings."
    if provider == "llamacpp":
        return "Start llama.cpp server on port 8080 (llama-server --port 8080 -m model.gguf)."
    if provider == "vllm":
        return "Start vLLM on port 8000 (vllm serve <model>)."
    if provider in {"openai_compatible", "openai", "grok", "xai"}:
        env = config.model.api_key_env or "OPENAI_API_KEY"
        return f"Check the endpoint and that {env} is set in ~/.config/shadow-agent/secrets.env."
    return "Open Settings and pick a reachable provider."


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
