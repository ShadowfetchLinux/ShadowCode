from __future__ import annotations

from pathlib import Path

import pytest

from shadow_agent.config import AppConfig, save_config
from shadow_agent.events import EventBus
from shadow_agent.store import Store


@pytest.fixture(autouse=True)
def _never_touch_real_xdg(tmp_path_factory: pytest.TempPathFactory, monkeypatch: pytest.MonkeyPatch) -> None:
    """Every test runs against throwaway XDG dirs.

    Before 0.18.0 a handful of tests that did not request ``isolated`` wrote
    ``onboarding.workspace`` / ``trusted_workspaces`` / ``last-workspace`` into
    the developer's real ``~/.config/shadow-agent`` and ``~/.local/state``. The
    autouse guard makes leakage impossible regardless of which fixtures a test
    asks for. ``HOME`` is left alone so user-site Python packages still import.
    """
    root = tmp_path_factory.mktemp("xdg")
    monkeypatch.setenv("XDG_CONFIG_HOME", str(root / "config"))
    monkeypatch.setenv("XDG_DATA_HOME", str(root / "data"))
    monkeypatch.setenv("XDG_STATE_HOME", str(root / "state"))
    # Never fire real desktop notifications or open browsers from the suite.
    monkeypatch.setenv("SHADOW_AGENT_NO_NOTIFY", "1")
    # doctor --fix must never run npm or the install script from the suite.
    monkeypatch.setenv("SHADOW_AGENT_DOCTOR_NO_INSTALL", "1")


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
