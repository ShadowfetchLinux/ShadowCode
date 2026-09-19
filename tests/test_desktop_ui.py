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


def test_config_endpoint_round_trips_theme_toggle(isolated, workspace) -> None:
    """Regression for 0.5.0 bug: the Settings theme toggle must actually
    write ui.theme to config.yaml and round-trip through /api/config so the
    React app can switch data-theme live without a restart."""
    app = create_app(store=Store(), default_workspace=workspace)
    client = TestClient(app)
    # Default for a fresh install is light.
    assert client.get("/api/config").json()["ui"]["theme"] == "light"
    # User toggles to dark in Settings.
    saved = client.put("/api/config", json={"values": {"ui": {"theme": "dark"}}, "api_key": "", "api_key_env": ""}).json()
    assert saved["ui"]["theme"] == "dark"
    # Persists across a fresh load.
    assert client.get("/api/config").json()["ui"]["theme"] == "dark"
    # Toggle back to light.
    saved = client.put("/api/config", json={"values": {"ui": {"theme": "light"}}, "api_key": "", "api_key_env": ""}).json()
    assert saved["ui"]["theme"] == "light"
    assert client.get("/api/config").json()["ui"]["theme"] == "light"


def test_existing_dark_theme_is_respected(isolated, workspace) -> None:
    """Per the 0.5.1 policy: an explicit theme:dark in config is respected,
    not auto-migrated. Only unset/empty themes default to light."""
    from shadow_agent.config import load_config
    cfg = apply_config_patch({"ui": {"theme": "dark"}})
    assert cfg.ui.theme == "dark"
    reloaded = load_config()
    assert reloaded.ui.theme == "dark"


def test_app_tsx_enter_submits_unless_shift() -> None:
    src = APP_TSX.read_text(encoding="utf-8")
    # The composer textarea must submit on Enter and keep Shift+Enter as a newline.
    assert "ev.key === \"Enter\"" in src
    assert "!ev.shiftKey" in src
    # The composer must NOT contain an ability / Computer Use select.
    assert "Computer Use" not in src.split("function Settings(")[0], "abilities must live in Settings, not the composer"
    # Placeholder matches the Codex feel.
    assert "Ask for follow-up" in src


def test_app_tsx_composer_clears_after_submit() -> None:
    """Regression: 0.5.0 left the submitted prompt in the composer after Enter.

    runTask must capture the text, push the user bubble, then clear the
    composer state (task + chips) immediately — before awaiting the job.
    """
    src = APP_TSX.read_text(encoding="utf-8")
    run_block = src.split("async function runTask(")[1].split("async function stopAgent")[0]
    # The user bubble is pushed from the captured `text`, not from `task`.
    assert "const text = composeTask()" in run_block
    assert "{ kind: \"user\", text }" in run_block
    # The composer is cleared inside runTask (not only on success).
    assert "setTask(\"\")" in run_block
    assert "setChips([])" in run_block
    # The clear must come before the awaited api.startJob so a failed dispatch
    # still leaves the composer empty.
    assert run_block.index("setTask(\"\")") < run_block.index("api.startJob")


def test_app_tsx_theme_defaults_light_when_unset() -> None:
    """The React app must treat a missing/empty theme as light, not dark."""
    src = APP_TSX.read_text(encoding="utf-8")
    # The theme effect: cfg.ui?.theme || "light" — dark only when explicit.
    assert '?.theme || "light"' in src
    assert 'dataset.theme' in src


def test_index_html_defaults_light() -> None:
    """index.html must ship data-theme="light" so first paint is light even
    before the config fetch resolves (no dark flash for new installs)."""
    html = (ROOT / "ui" / "index.html").read_text(encoding="utf-8")
    assert 'data-theme="light"' in html


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


@pytest.mark.skipif(not UI_DIST_JS.exists() or not UI_DIST_CSS.exists(), reason="UI not built")
def test_built_bundle_has_composer_clear_and_light_default() -> None:
    """The rebuilt bundle must ship the 0.5.1 fixes: composer clears on
    submit, and index.html defaults to data-theme='light'."""
    js = next(UI_DIST_JS.glob("index-*.js")).read_text(encoding="utf-8")
    # The composer clear compiles to two empty setters right before the
    # awaited startJob call: `Fl(""),Na([])` (setTask(""), setChips([])).
    # Minified names vary per build, but the pattern `("")]` + `([])`
    # immediately preceding the call site is stable. The api.ts definition
    # (`startJob:(b,O,...)=>`) appears first; the call site is `.startJob(`.
    call_idx = js.index(".startJob(")
    window = js[max(0, call_idx - 120):call_idx]
    assert '("")' in window, "runTask must clear the task text before dispatching"
    assert "([])" in window, "runTask must clear the chips before dispatching"
    # index.html ships the explicit light default.
    html = (ROOT / "ui" / "dist" / "index.html").read_text(encoding="utf-8")
    assert 'data-theme="light"' in html
    # Bundle must be newer than the source so the server ships the fix.
    src_mtime = (ROOT / "ui" / "src" / "App.tsx").stat().st_mtime
    bundle_mtime = next(UI_DIST_JS.glob("index-*.js")).stat().st_mtime
    assert bundle_mtime >= src_mtime, "ui/dist is stale — rebuild before shipping"
