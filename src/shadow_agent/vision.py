"""Visual debugging — send a screenshot to a vision model.

`inspect this screenshot` (or `/vision <path>`) loads an image, sends it to a
vision-capable provider with a structured prompt, and returns detected
issues + suggested files for the coder to fix.

If no vision model is configured, this is a graceful no-op: it returns a
note telling the user how to enable vision, and never blocks the agent.
"""

from __future__ import annotations

import base64
import mimetypes
from pathlib import Path
from typing import Any

from shadow_agent.config import AppConfig
from shadow_agent.models.interface import ModelProvider
from shadow_agent.models.registry import ModelRegistry
from shadow_agent.models.types import ChatRequest, Message


VISION_PROMPT = """You are a vision-capable debugging assistant. Analyze this screenshot and report:

1. DETECTED ISSUES: list each visible problem (error message, traceback, broken layout, failing test, exception, etc.) with a short label.
2. SUGGESTED FILES: for each issue, name the source file most likely responsible (best guess from the visible stack trace / UI / output).
3. NEXT STEP: one concrete next step the coder should take.

Format as:

ISSUES:
- <issue>: <suggested file>
NEXT: <one-line next step>
"""


def _encode_image(path: Path) -> tuple[str, str]:
    mime = mimetypes.guess_type(str(path))[0] or "image/png"
    data = base64.b64encode(path.read_bytes()).decode("ascii")
    return mime, data


def _vision_provider(config: AppConfig, registry: ModelRegistry | None = None) -> ModelProvider | None:
    """Find a vision-capable provider, or None if none is configured."""
    reg = registry or ModelRegistry(detect=True)
    # 1. Explicit routing.vision setting.
    if config.routing.enabled and config.routing.vision:
        info = reg.get(config.routing.vision)
        if info and info.metadata.get("vision"):
            return reg.create(config, config.routing.vision)
    # 2. Any detected model that advertises vision.
    for info in reg.list_models():
        caps = info.metadata.get("capabilities") or {}
        if caps.get("vision") or info.metadata.get("vision"):
            return reg.create(config, info.id)
    # 3. An Ollama model whose name suggests vision (llava, moondream, etc.).
    for info in reg.list_models():
        name = (info.id or "").lower()
        if any(tag in name for tag in ("llava", "moondream", "vision", "minicpm-v", "qwen2-vl", "qwen3-vl")):
            return reg.create(config, info.id)
    return None


def analyze_screenshot(workspace: Path, image_path: str, config: AppConfig | None = None, model: ModelProvider | None = None) -> dict[str, Any]:
    """Analyze a screenshot. Graceful no-op if no vision model is available."""
    path = Path(image_path)
    if not path.is_absolute():
        path = Path(workspace) / path
    if not path.is_file():
        return {"ok": False, "error": f"image not found: {path}"}
    cfg = config or AppConfig()
    provider = model or _vision_provider(cfg)
    if provider is None:
        return {
            "ok": False,
            "skipped": True,
            "error": "no vision model configured. Set routing.vision to a vision-capable model id (e.g. llava, qwen2-vl) or install one via `ollama pull llava`.",
        }
    caps = provider.get_capabilities()
    if not caps.vision:
        return {
            "ok": False,
            "skipped": True,
            "error": f"configured model {provider.name} is not vision-capable. Pick a vision model in Settings.",
        }
    mime, data = _encode_image(path)
    # OpenAI-compatible vision message format.
    content = [
        {"type": "text", "text": VISION_PROMPT},
        {"type": "image_url", "image_url": {"url": f"data:{mime};base64,{data}"}},
    ]
    request = ChatRequest(messages=[Message(role="user", content=content)], max_tokens=512, temperature=0.1)  # type: ignore[arg-type]
    try:
        response = provider.chat(request)
    except Exception as exc:  # noqa: BLE001
        return {"ok": False, "error": f"vision request failed: {exc}"}
    text = response.text or ""
    return {
        "ok": True,
        "model": provider.name,
        "analysis": text,
        "issues": _parse_issues(text),
        "suggested_files": _parse_files(text),
        "next_step": _parse_next(text),
    }


def _parse_issues(text: str) -> list[dict[str, str]]:
    issues: list[dict[str, str]] = []
    for line in text.splitlines():
        line = line.strip()
        if line.startswith("- ") and ":" in line:
            body = line[2:]
            label, _, rest = body.partition(":")
            issues.append({"issue": label.strip(), "file": rest.strip()})
    return issues


def _parse_files(text: str) -> list[str]:
    files: list[str] = []
    for issue in _parse_issues(text):
        if issue["file"] and "/" in issue["file"] or issue["file"].endswith(".py") or issue["file"].endswith(".ts"):
            files.append(issue["file"])
    return files


def _parse_next(text: str) -> str:
    for line in text.splitlines():
        if line.lower().startswith("next:"):
            return line[5:].strip()
    return ""


def render_analysis(result: dict[str, Any]) -> str:
    if not result.get("ok"):
        if result.get("skipped"):
            return f"Vision: skipped — {result.get('error')}"
        return f"Vision: error — {result.get('error')}"
    lines = [f"Vision analysis  ·  model: {result.get('model')}", ""]
    lines.append(result.get("analysis", ""))
    return "\n".join(lines)
