from __future__ import annotations

from shadow_agent.config import AppConfig, load_config, set_config_value
from shadow_agent import paths


def test_default_config_is_mock_workspace(isolated):
    cfg = load_config()
    assert cfg.model.default == "mock"
    assert cfg.permissions.level.value == "workspace"
    assert cfg.model.api_key_env == "OPENAI_API_KEY"


def test_set_and_reload(isolated):
    set_config_value("model.default", "ollama")
    set_config_value("ui.port", "7444")
    cfg = load_config()
    assert cfg.model.default == "ollama"
    assert cfg.ui.port == 7444
    assert paths.config_file().is_file()


def test_project_overlay(isolated, workspace):
    shadow = workspace / ".shadow" / "config"
    shadow.mkdir(parents=True)
    (shadow / "config.yaml").write_text("permissions:\n  level: read_only\n", encoding="utf-8")
    cfg = load_config(workspace)
    assert cfg.permissions.level.value == "read_only"
