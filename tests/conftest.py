from __future__ import annotations

from pathlib import Path

import pytest

from shadow_agent.config import AppConfig, save_config
from shadow_agent.events import EventBus
from shadow_agent.store import Store


@pytest.fixture
def isolated(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    home = tmp_path / "home"
    home.mkdir()
    monkeypatch.setenv("HOME", str(home))
    monkeypatch.setenv("XDG_CONFIG_HOME", str(tmp_path / "config"))
    monkeypatch.setenv("XDG_DATA_HOME", str(tmp_path / "data"))
    monkeypatch.setenv("XDG_STATE_HOME", str(tmp_path / "state"))
    save_config(AppConfig())
    return tmp_path


@pytest.fixture
def workspace(tmp_path: Path) -> Path:
    path = tmp_path / "workspace"
    path.mkdir()
    return path


@pytest.fixture
def store(isolated: Path) -> Store:
    return Store()


@pytest.fixture
def bus(isolated: Path) -> EventBus:
    return EventBus()
