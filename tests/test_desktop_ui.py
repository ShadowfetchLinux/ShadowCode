"""Tests for the 0.5.0 Codex light-mode desktop UI restyle.

Covers:
- light theme is the default for the desktop UI
- abilities (Computer Use / Custom) live in Settings / config, not the composer
- Enter submits the prompt unless Shift is held (source-level contract on App.tsx)
- the built UI bundle ships the new light-theme tokens and floating composer
"""
from __future__ import annotations

from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from shadow_agent.api.server import create_app
from shadow_agent.config import AppConfig, apply_config_patch
from shadow_agent.store import Store

ROOT = Path(__file__).resolve().parents[1]
APP_TSX = ROOT / "ui" / "src" / "App.tsx"
API_TS = ROOT / "ui" / "src" / "api.ts"
UI_DIST_JS = ROOT / "ui" / "dist" / "assets"
UI_DIST_CSS = ROOT / "ui" / "dist" / "assets"


def test_desktop_theme_default_is_light() -> None:
    cfg = AppConfig()
    assert cfg.ui.theme == "light", "Codex light mode must be the default desktop theme"


def test_abilities_field_default_none() -> None:
    cfg = AppConfig()
    assert cfg.ui.ability == "none"


def test_config_persists_ability_and_theme(isolated) -> None:
    cfg = apply_config_patch({"ui": {"theme": "dark", "ability": "computer_use"}})
    assert cfg.ui.theme == "dark"
    assert cfg.ui.ability == "computer_use"


def test_onboarding_defaults_light(isolated, workspace) -> None:
    app = create_app(store=Store(), default_workspace=workspace)
    client = TestClient(app)
    body = client.get("/api/onboarding").json()
    assert body["defaults"]["theme"] == "light"
    assert body["defaults"]["ability"] == "none"


def test_config_endpoint_round_trips_ability(isolated, workspace) -> None:
    app = create_app(store=Store(), default_workspace=workspace)
    client = TestClient(app)
    saved = client.put("/api/config", json={"values": {"ui": {"ability": "computer_use"}}, "api_key": "", "api_key_env": ""}).json()
    assert saved["ui"]["ability"] == "computer_use"
    again = client.get("/api/config").json()
    assert again["ui"]["ability"] == "computer_use"


def test_app_tsx_enter_submits_unless_shift() -> None:
    src = APP_TSX.read_text(encoding="utf-8")
    # The composer textarea must submit on Enter and keep Shift+Enter as a newline.
    assert "ev.key === \"Enter\"" in src
    assert "!ev.shiftKey" in src
    # The composer must NOT contain an ability / Computer Use select.
    assert "Computer Use" not in src.split("function Settings(")[0], "abilities must live in Settings, not the composer"
    # Placeholder matches the Codex feel.
    assert "Ask for follow-up" in src


def test_app_tsx_abilities_live_in_settings() -> None:
    src = APP_TSX.read_text(encoding="utf-8")
    settings_block = src.split("function Settings(")[1].split("function Help(")[0]
    assert "Abilities" in settings_block
    assert "computer_use" in settings_block
    assert "custom" in settings_block


def test_api_startjob_accepts_purpose() -> None:
    src = API_TS.read_text(encoding="utf-8")
    assert "purpose" in src
    assert "purpose: string" in src or "purpose=" in src


@pytest.mark.skipif(not UI_DIST_JS.exists() or not UI_DIST_CSS.exists(), reason="UI not built")
def test_built_bundle_has_light_composer_tokens() -> None:
    js = next(UI_DIST_JS.glob("index-*.js")).read_text(encoding="utf-8")
    css = next(UI_DIST_CSS.glob("index-*.css")).read_text(encoding="utf-8")
    # Floating composer + pill user bubble + approval Enter hint.
    for token in ("composer-wrap", "user-pill", "msg-user", "chat-stream", "submit-btn", "mode-select", "mic-btn"):
        assert token in css, f"missing CSS token: {token}"
        assert token in js, f"missing JS class: {token}"
    # Enter-submits handler compiled in.
    assert "shiftKey" in js
    # Light theme is the root palette (white canvas) and dark is the opt-in toggle.
    assert "#ffffff" in css
    assert "data-theme=dark]" in css or '[data-theme="dark"]' in css
    assert "Work locally" in js
