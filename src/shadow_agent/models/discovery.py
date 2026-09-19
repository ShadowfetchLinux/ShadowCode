"""Auto-detect running local model servers and list their installed models.

Probes Ollama (:11434), LM Studio (:1234), llama.cpp server (:8080), and
vLLM (:8000) with short timeouts so the UI can offer one-click selection.
"""

from __future__ import annotations

import socket
import time
from typing import Any

import httpx
from pydantic import BaseModel, Field


class DetectedModel(BaseModel):
    id: str
    name: str
    size_bytes: int = 0
    context_limit: int = 0
    capabilities: dict[str, bool] = Field(default_factory=dict)
    detail: str = ""


class DetectedProvider(BaseModel):
    provider: str
    label: str
    endpoint: str
    running: bool = False
    latency_ms: float = 0.0
    models: list[DetectedModel] = Field(default_factory=list)
    detail: str = ""


PROBES: list[dict[str, Any]] = [
    {
        "provider": "ollama",
        "label": "Ollama",
        "endpoint": "http://127.0.0.1:11434/v1",
        "tags_url": "http://127.0.0.1:11434/api/tags",
        "kind": "ollama",
    },
    {
        "provider": "local",
        "label": "LM Studio / local /v1",
        "endpoint": "http://127.0.0.1:1234/v1",
        "tags_url": "http://127.0.0.1:1234/v1/models",
        "kind": "openai_models",
    },
    {
        "provider": "llamacpp",
        "label": "llama.cpp server",
        "endpoint": "http://127.0.0.1:8080/v1",
        "tags_url": "http://127.0.0.1:8080/v1/models",
        "kind": "openai_models",
    },
    {
        "provider": "vllm",
        "label": "vLLM",
        "endpoint": "http://127.0.0.1:8000/v1",
        "tags_url": "http://127.0.0.1:8000/v1/models",
        "kind": "openai_models",
    },
]


def _port_open(url: str, timeout: float = 0.4) -> bool:
    try:
        host, _, rest = url.partition("://")
        hostport = rest.split("/")[0]
        hostname, _, port = hostport.partition(":")
        with socket.create_connection((hostname, int(port or 80)), timeout=timeout):
            return True
    except (OSError, ValueError):
        return False


def _parse_ollama(data: dict[str, Any]) -> list[DetectedModel]:
    models: list[DetectedModel] = []
    for item in data.get("models") or []:
        details = item.get("details") or {}
        caps_raw = item.get("capabilities") or []
        caps = {
            "tools": "tools" in caps_raw,
            "thinking": "thinking" in caps_raw,
            "vision": "vision" in caps_raw,
            "completion": "completion" in caps_raw or True,
        }
        size = int(item.get("size") or 0)
        param = details.get("parameter_size") or ""
        quant = details.get("quantization_level") or ""
        models.append(
            DetectedModel(
                id=str(item.get("name") or item.get("model") or ""),
                name=str(item.get("name") or ""),
                size_bytes=size,
                context_limit=int(details.get("context_length") or 0),
                capabilities=caps,
                detail=" · ".join(bit for bit in (param, quant, _human_size(size)) if bit),
            )
        )
    return [m for m in models if m.id]


def _parse_openai_models(data: dict[str, Any]) -> list[DetectedModel]:
    models: list[DetectedModel] = []
    for item in data.get("data") or []:
        mid = str(item.get("id") or "")
        if mid:
            models.append(DetectedModel(id=mid, name=mid, capabilities={"tools": True, "completion": True}))
    return models


def _human_size(size: int) -> str:
    if size <= 0:
        return ""
    gib = size / float(1024**3)
    return f"{gib:.1f} GiB"


def detect_providers(timeout: float = 1.2) -> list[DetectedProvider]:
    """Probe every known local server. Never raises; offline servers report running=False."""
    found: list[DetectedProvider] = []
    for probe in PROBES:
        result = DetectedProvider(
            provider=probe["provider"],
            label=probe["label"],
            endpoint=probe["endpoint"],
        )
        if not _port_open(probe["tags_url"]):
            result.detail = "not running"
            found.append(result)
            continue
        started = time.monotonic()
        try:
            response = httpx.get(probe["tags_url"], timeout=timeout)
            result.latency_ms = round((time.monotonic() - started) * 1000, 1)
            if response.status_code >= 400:
                result.detail = f"HTTP {response.status_code}"
                found.append(result)
                continue
            data = response.json()
            if probe["kind"] == "ollama":
                result.models = _parse_ollama(data)
            else:
                result.models = _parse_openai_models(data)
            result.running = True
            count = len(result.models)
            result.detail = f"{count} model{'s' if count != 1 else ''} installed" if count else "running, no models reported"
        except Exception as exc:  # noqa: BLE001 - detection must never crash the API
            result.detail = f"unreachable: {exc.__class__.__name__}"
        found.append(result)
    return found


def detected_model_entries(found: list[DetectedProvider]) -> list[dict[str, Any]]:
    """Flatten detected models into registry-ready dicts."""
    entries: list[dict[str, Any]] = []
    for provider in found:
        if not provider.running:
            continue
        for model in provider.models:
            entries.append(
                {
                    "id": model.id,
                    "name": f"{model.name} ({provider.label})",
                    "provider": provider.provider,
                    "endpoint": provider.endpoint,
                    "context_limit": model.context_limit or 128000,
                    "metadata": {
                        "model": model.id,
                        "detected": True,
                        "capabilities": model.capabilities,
                        "size_bytes": model.size_bytes,
                        "detail": model.detail,
                    },
                }
            )
    return entries
