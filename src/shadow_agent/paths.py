"""XDG paths for Shadow Agent. Never assume Windows/macOS layouts."""

from __future__ import annotations

import os
from pathlib import Path

APP_NAME = "shadow-agent"


def home() -> Path:
    return Path(os.environ.get("HOME") or Path.home()).expanduser()


def xdg_config_home() -> Path:
    raw = os.environ.get("XDG_CONFIG_HOME")
    return Path(raw).expanduser() if raw else home() / ".config"


def xdg_data_home() -> Path:
    raw = os.environ.get("XDG_DATA_HOME")
    return Path(raw).expanduser() if raw else home() / ".local" / "share"


def xdg_state_home() -> Path:
    raw = os.environ.get("XDG_STATE_HOME")
    return Path(raw).expanduser() if raw else home() / ".local" / "state"


def config_dir() -> Path:
    path = xdg_config_home() / APP_NAME
    path.mkdir(parents=True, exist_ok=True)
    return path


def data_dir() -> Path:
    path = xdg_data_home() / APP_NAME
    path.mkdir(parents=True, exist_ok=True)
    return path


def state_dir() -> Path:
    path = xdg_state_home() / APP_NAME
    path.mkdir(parents=True, exist_ok=True)
    return path


def config_file() -> Path:
    return config_dir() / "config.yaml"


def db_file() -> Path:
    return state_dir() / "shadow-agent.db"


def log_dir() -> Path:
    path = state_dir() / "logs"
    path.mkdir(parents=True, exist_ok=True)
    return path


def events_log() -> Path:
    return log_dir() / "events.jsonl"


def task_dir(task_id: str) -> Path:
    path = state_dir() / "tasks" / task_id
    path.mkdir(parents=True, exist_ok=True)
    return path


def plugins_dir() -> Path:
    path = config_dir() / "plugins"
    path.mkdir(parents=True, exist_ok=True)
    return path
