"""Tests for the desktop UI contract (0.5.x light mode → 0.18.0 clean layout).

Covers:
- light theme is the default; explicit dark is respected and round-trips
- abilities live in Settings, not the composer
- Enter submits the prompt (Shift+Enter newline) and the composer clears
  *before* the job is dispatched
- 0.18.0 clean layout: drawer closed by default, single right-hand drawer with
  tabs, top bar + transcript + composer + thin status line as the default view
- model picker lists every provider group plus a free-text "Custom model…" entry
- the built bundle ships all of the above
"""
from __future__ import annotations

from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from shadow_agent.api.server import create_app
from shadow_agent.config import AppConfig, apply_config_patch
from shadow_agent.store import Store

ROOT = Path(__file__).resolve().parents[1]
UI_SRC = ROOT / "ui" / "src"
APP_TSX = UI_SRC / "App.tsx"
API_TS = UI_SRC / "api.ts"
SETTINGS_TSX = UI_SRC / "components" / "Settings.tsx"
DRAWER_TSX = UI_SRC / "components" / "Drawer.tsx"
CARDS_TSX = UI_SRC / "components" / "cards.tsx"
ONBOARDING_TSX = UI_SRC / "components" / "Onboarding.tsx"
UI_DIST = ROOT / "ui" / "dist" / "assets"


def _src(path: Path) -> str:
    return path.read_text(encoding="utf-8")


# --- theme + abilities --------------------------------------------------------


def test_desktop_theme_default_is_light() -> None:
    assert AppConfig().ui.theme == "light"


def test_abilities_field_default_none() -> None:
    assert AppConfig().ui.ability == "none"


def test_config_persists_ability_and_theme(isolated) -> None:
    cfg = apply_config_patch({"ui": {"theme": "dark", "ability": "computer_use"}})
    assert cfg.ui.theme == "dark"
    assert cfg.ui.ability == "computer_use"


def test_onboarding_defaults_light(isolated, workspace) -> None:
    client = TestClient(create_app(store=Store(), default_workspace=workspace, detect=False))
    body = client.get("/api/onboarding").json()
    assert body["defaults"]["theme"] == "light"
    assert body["defaults"]["ability"] == "none"


def test_config_endpoint_round_trips_theme_toggle(isolated, workspace) -> None:
    client = TestClient(create_app(store=Store(), default_workspace=workspace, detect=False))
    assert client.get("/api/config").json()["ui"]["theme"] == "light"
    saved = client.put("/api/config", json={"values": {"ui": {"theme": "dark"}}, "api_key": "", "api_key_env": ""}).json()
    assert saved["ui"]["theme"] == "dark"
    assert client.get("/api/config").json()["ui"]["theme"] == "dark"
    saved = client.put("/api/config", json={"values": {"ui": {"theme": "light"}}, "api_key": "", "api_key_env": ""}).json()
    assert saved["ui"]["theme"] == "light"


def test_existing_dark_theme_is_respected(isolated, workspace) -> None:
    from shadow_agent.config import load_config

    apply_config_patch({"ui": {"theme": "dark"}})
    assert load_config().ui.theme == "dark"


def test_app_tsx_theme_defaults_light_when_unset() -> None:
    src = _src(APP_TSX)
    assert '?.theme || "light"' in src
    assert "dataset.theme" in src


def test_index_html_defaults_light_and_is_branded() -> None:
    html = (ROOT / "ui" / "index.html").read_text(encoding="utf-8")
    assert 'data-theme="light"' in html
    assert "<title>ShadowCode</title>" in html


def test_abilities_live_in_settings_not_composer() -> None:
    assert "Computer Use" not in _src(APP_TSX), "abilities must live in Settings, not the composer"
    settings = _src(SETTINGS_TSX)
    assert "Abilities" in settings
    assert "computer_use" in settings
    assert "custom" in settings


# --- composer: Enter sends + clears ---------------------------------------------


def test_app_tsx_enter_submits_unless_shift() -> None:
    src = _src(APP_TSX)
    assert 'ev.key === "Enter"' in src
    assert "!ev.shiftKey" in src
    assert "Ask for follow-up" in src


def test_app_tsx_composer_clears_before_dispatch() -> None:
    src = _src(APP_TSX)
    run_block = src.split("async function runTask(")[1].split("async function stopAgent")[0]
    assert "const text = composeTask()" in run_block
    assert '{ kind: "user", text }' in run_block
    assert 'setTask("")' in run_block
    assert "setChips([])" in run_block
    assert run_block.index('setTask("")') < run_block.index("api.startJob")


def test_api_startjob_accepts_purpose() -> None:
    src = _src(API_TS)
    assert "purpose" in src


# --- 0.18.0 clean layout --------------------------------------------------------


def test_drawer_is_closed_by_default() -> None:
    src = _src(APP_TSX)
    assert "const [drawerOpen, setDrawerOpen] = useState(false)" in src
    # The default view carries an explicit class the served HTML/JS can be checked for.
    assert '"drawer-open" : "drawer-closed"' in src
    # The drawer is not even mounted while closed.
    assert "{drawerOpen && (" in src


def test_default_view_is_topbar_transcript_composer_statusline() -> None:
    src = _src(APP_TSX)
    for cls in ('className="top"', 'className="chat-stream"', 'className="composer"', 'className="statusline"'):
        assert cls in src, cls
    # Legacy three-column panels are gone.
    for gone in ('className="left"', 'className="right"', "RECENT FOLDERS", "Work locally", "INSPECT", "WORKSPACE"):
        assert gone not in src, f"legacy panel leaked into the clean layout: {gone}"


def test_single_drawer_consolidates_inspect_panels() -> None:
    src = _src(DRAWER_TSX)
    for tab in ("sessions", "files", "changes", "skills", "goals", "health", "background"):
        assert f'id: "{tab}"' in src, tab
    # Health tab carries doctor + router; changes tab carries git + per-hunk actions.
    assert "api.doctor()" in src and "api.routing()" in src
    assert 'hunkAction(selected, h, action)' in src


def test_command_palette_and_drawer_shortcuts() -> None:
    src = _src(APP_TSX)
    assert 'key === "k"' in src and 'setOverlay("palette")' in src
    assert 'key === "b"' in src and "setDrawerOpen((v) => !v)" in src


def test_model_picker_groups_all_providers_and_has_custom_entry() -> None:
    src = _src(APP_TSX)
    assert "<optgroup" in src
    assert '"__custom__"' in src
    assert "Custom model…" in src
    assert "CustomModelDialog" in src


def test_op_card_stays_collapsed_and_has_rewind_and_diff_actions() -> None:
    cards = _src(CARDS_TSX)
    assert "collapsed === false" in cards
    assert "↶ Rewind" in cards
    assert "Review diff" in cards
    # Approval card: subtle, Allow ↵ / Cancel Esc.
    assert "Allow <span" in cards and "↵" in cards
    assert "Cancel <span" in cards and "Esc" in cards


def test_onboarding_is_single_screen_one_click() -> None:
    src = _src(ONBOARDING_TSX)
    assert "Test & start" in src
    assert "model-tile" in src
    # No multi-step wizard state.
    assert "setStep(" not in src


def test_settings_overlay_grouped_sections() -> None:
    src = _src(SETTINGS_TSX)
    for label in ("Model", "Permissions", "Appearance", "Hooks", "MCP", "Plugins"):
        assert f'label: "{label}"' in src, label


def test_desktop_notification_on_completion() -> None:
    src = _src(APP_TSX)
    assert "new Notification(" in src
    assert "Notification.requestPermission" in src


# --- API contracts behind the picker -------------------------------------------


def test_api_models_covers_every_builtin_provider(isolated, workspace) -> None:
    client = TestClient(create_app(store=Store(), default_workspace=workspace, detect=False))
    models = client.get("/api/models").json()["models"]
    providers = {m["provider"] for m in models}
    assert {"mock", "openai_compatible", "local", "ollama", "llamacpp", "vllm"} <= providers


def test_api_providers_lists_all_targets(isolated, workspace) -> None:
    client = TestClient(create_app(store=Store(), default_workspace=workspace, detect=False))
    ids = {p["id"] for p in client.get("/api/providers").json()["providers"]}
    assert ids == {"ollama", "openai_compatible", "local", "llamacpp", "vllm", "mock"}


def test_api_select_free_text_model_for_any_provider(isolated, workspace) -> None:
    client = TestClient(create_app(store=Store(), default_workspace=workspace, detect=False))
    saved = client.post("/api/models/select", json={"id": "my-finetune-7b", "provider": "vllm"}).json()
    assert saved["model"]["default"] == "my-finetune-7b"
    assert saved["model"]["provider"] == "vllm"
    assert saved["model"]["endpoint"] == "http://127.0.0.1:8000/v1"
    ids = {m["id"] for m in client.get("/api/models").json()["models"]}
    assert "my-finetune-7b" in ids


# --- built bundle --------------------------------------------------------------


@pytest.mark.skipif(not UI_DIST.exists(), reason="UI not built")
def test_built_bundle_ships_clean_layout() -> None:
    js = next(UI_DIST.glob("index-*.js")).read_text(encoding="utf-8")
    css = next(UI_DIST.glob("index-*.css")).read_text(encoding="utf-8")
    for token in ("composer-wrap", "chat-stream", "statusline", "drawer-tabs", "op-card", "submit-btn", "mode-select", "mic-btn"):
        assert token in css, f"missing CSS token: {token}"
        assert token in js, f"missing JS class: {token}"
    assert "drawer-closed" in js
    assert "Custom model" in js
    assert "shiftKey" in js
    assert "Work locally" not in js
    assert "#ffffff" in css
    assert '[data-theme="dark"]' in css or "data-theme=dark]" in css


@pytest.mark.skipif(not UI_DIST.exists(), reason="UI not built")
def test_built_bundle_is_fresh_and_light() -> None:
    js_path = next(UI_DIST.glob("index-*.js"))
    js = js_path.read_text(encoding="utf-8")
    call_idx = js.index(".startJob(")
    window = js[max(0, call_idx - 160):call_idx]
    assert '("")' in window, "runTask must clear the task text before dispatching"
    assert "([])" in window, "runTask must clear the chips before dispatching"
    html = (ROOT / "ui" / "dist" / "index.html").read_text(encoding="utf-8")
    assert 'data-theme="light"' in html
    newest_src = max(p.stat().st_mtime for p in UI_SRC.rglob("*") if p.is_file())
    assert js_path.stat().st_mtime >= newest_src - 1, "ui/dist is stale — rebuild before shipping"
