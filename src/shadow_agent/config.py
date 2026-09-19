"""Load and save ~/.config/shadow-agent/config.yaml plus project overlays."""

from __future__ import annotations

from enum import Enum
from pathlib import Path
from typing import Any

import yaml
from pydantic import BaseModel, Field

from shadow_agent import paths


class PermissionLevel(str, Enum):
    READ_ONLY = "read_only"
    WORKSPACE = "workspace"
    ELEVATED = "elevated"


class ModelConfig(BaseModel):
    default: str = "mock"
    provider: str = "mock"
    endpoint: str = ""
    api_key_env: str = "OPENAI_API_KEY"
    name: str = "mock-coder"
    context_limit: int = 128000


class PermissionsConfig(BaseModel):
    level: PermissionLevel = PermissionLevel.WORKSPACE
    require_approval_for_dangerous: bool = True
    network: bool = False
    allow_root: bool = False


class GitConfig(BaseModel):
    auto_commit: bool = False
    allow_destructive: bool = False


class LoggingConfig(BaseModel):
    level: str = "info"


class UIConfig(BaseModel):
    host: str = "127.0.0.1"
    port: int = 7430
    theme: str = "dark"


class AgentConfig(BaseModel):
    max_steps: int = 32
    tool_timeout_sec: int = 60
    parallel_reads: bool = True
    compact_ratio: float = 0.7


class OnboardingConfig(BaseModel):
    completed: bool = False
    workspace: str = ""


class RoutingConfig(BaseModel):
    enabled: bool = False
    planner: str = "mock"
    coder: str = "mock"
    reviewer: str = "mock"
    tester: str = "mock"


class MCPServerConfig(BaseModel):
    name: str
    command: list[str] | None = None
    url: str | None = None


class MCPConfig(BaseModel):
    servers: list[MCPServerConfig] = Field(default_factory=list)


class AppConfig(BaseModel):
    model: ModelConfig = Field(default_factory=ModelConfig)
    permissions: PermissionsConfig = Field(default_factory=PermissionsConfig)
    git: GitConfig = Field(default_factory=GitConfig)
    logging: LoggingConfig = Field(default_factory=LoggingConfig)
    ui: UIConfig = Field(default_factory=UIConfig)
    agent: AgentConfig = Field(default_factory=AgentConfig)
    routing: RoutingConfig = Field(default_factory=RoutingConfig)
    mcp: MCPConfig = Field(default_factory=MCPConfig)
    onboarding: OnboardingConfig = Field(default_factory=OnboardingConfig)


def _read_yaml(path: Path) -> dict[str, Any]:
    if not path.is_file():
        return {}
    data = yaml.safe_load(path.read_text(encoding="utf-8")) or {}
    if not isinstance(data, dict):
        raise ValueError(f"config must be a mapping: {path}")
    return data


def _deep_merge(base: dict[str, Any], overlay: dict[str, Any]) -> dict[str, Any]:
    out = dict(base)
    for key, value in overlay.items():
        if key in out and isinstance(out[key], dict) and isinstance(value, dict):
            out[key] = _deep_merge(out[key], value)
        else:
            out[key] = value
    return out


def load_config(workspace: Path | None = None) -> AppConfig:
    data: dict[str, Any] = {}
    data = _deep_merge(data, _read_yaml(paths.config_file()))
    if workspace is not None:
        overlay = Path(workspace) / ".shadow" / "config" / "config.yaml"
        data = _deep_merge(data, _read_yaml(overlay))
    return AppConfig.model_validate(data)


def save_config(config: AppConfig) -> Path:
    target = paths.config_file()
    target.write_text(
        yaml.safe_dump(config.model_dump(mode="json"), sort_keys=False),
        encoding="utf-8",
    )
    return target


def ensure_user_config() -> AppConfig:
    if not paths.config_file().is_file():
        save_config(AppConfig())
    return load_config()


def apply_config_patch(values: dict[str, Any]) -> AppConfig:
    cfg = ensure_user_config()
    raw = _deep_merge(cfg.model_dump(mode="json"), values)
    cfg = AppConfig.model_validate(raw)
    save_config(cfg)
    return cfg


def remember_workspace(workspace: Path) -> None:
    path = paths.last_workspace_file()
    path.write_text(str(Path(workspace).resolve()) + "\n", encoding="utf-8")


def last_workspace() -> Path | None:
    path = paths.last_workspace_file()
    if not path.is_file():
        return None
    raw = path.read_text(encoding="utf-8").strip()
    if not raw:
        return None
    candidate = Path(raw).expanduser()
    return candidate if candidate.is_dir() else None


def set_config_value(dotted: str, value: Any) -> AppConfig:
    cfg = ensure_user_config()
    raw = cfg.model_dump(mode="json")
    cursor = raw
    parts = dotted.split(".")
    for part in parts[:-1]:
        nxt = cursor.get(part)
        if not isinstance(nxt, dict):
            nxt = {}
            cursor[part] = nxt
        cursor = nxt
    parsed: Any = value
    if isinstance(value, str):
        lowered = value.lower()
        if lowered in {"true", "false"}:
            parsed = lowered == "true"
        else:
            try:
                parsed = int(value)
            except ValueError:
                parsed = value
    cursor[parts[-1]] = parsed
    cfg = AppConfig.model_validate(raw)
    save_config(cfg)
    return cfg
