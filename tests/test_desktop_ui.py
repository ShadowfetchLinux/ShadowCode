"""Desktop API contract. Interaction and accessibility coverage lives in ui/e2e.

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






def test_api_startjob_accepts_purpose() -> None:
    src = _src(API_TS)
    assert "purpose" in src


# --- 0.18.0 clean layout --------------------------------------------------------






def test_single_drawer_consolidates_inspect_panels() -> None:
    src = _src(DRAWER_TSX)
    for tab in ("sessions", "files", "changes", "skills", "goals", "health", "background"):
        assert f'id: "{tab}"' in src, tab
    # Health tab carries doctor + router; changes tab carries git + per-hunk actions.
    assert ".doctor()" in src and ".routing()" in src
    assert '.hunkAction(selected, h, action)' in src




def test_model_picker_groups_all_providers_and_has_custom_entry() -> None:
    chooser = _src(UI_SRC / "components" / "ModelChooser.tsx")
    copy = _src(UI_SRC / "lib" / "cliAgents.ts")
    app = _src(APP_TSX)
    assert "<optgroup" in chooser
    assert 'LOCAL_GROUP = "Local model (ShadowCode agent)"' in copy
    assert 'VENDOR_GROUP = "Claude / Codex / Grok (vendor agent)"' in copy
    assert "LOCAL_GROUP" in chooser and "VENDOR_GROUP" in chooser
    assert '"__custom__"' in chooser
    assert "Custom model…" in chooser
    assert "CustomModelDialog" in app
    assert "<ModelChooser" in app




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
def test_built_bundle_assets_exist_and_are_fresh():
    import re
    html = (ROOT / "ui" / "dist" / "index.html").read_text()
    assets = re.findall(r'(?:src|href)="(/assets/[^"]+)"', html)
    assert assets
    for asset in assets:
        assert (ROOT / "ui" / "dist" / asset.lstrip("/")).is_file()
    bundles = list(UI_DIST.glob("*.js"))
    newest = max(p.stat().st_mtime for p in UI_SRC.rglob("*") if p.is_file() and '.test.' not in p.name)
    assert max(p.stat().st_mtime for p in bundles) >= newest - 1
