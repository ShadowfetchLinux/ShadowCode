from __future__ import annotations

from typer.testing import CliRunner

from shadow_agent.cli import app

runner = CliRunner()


def test_cli_models_and_config(isolated):
    result = runner.invoke(app, ["models"])
    assert result.exit_code == 0
    assert "mock" in result.stdout
    shown = runner.invoke(app, ["config"])
    assert shown.exit_code == 0
    assert "shadow-agent" in shown.stdout or "mock" in shown.stdout


def test_cli_help_and_health(isolated, workspace):
    shown = runner.invoke(app, ["--help"])
    assert shown.exit_code == 0
    assert "health" in shown.stdout
    assert "ui" in shown.stdout
    check = runner.invoke(app, ["health", "-p", str(workspace)])
    assert check.exit_code == 0
    assert "mock" in check.stdout


def test_cli_run_hello(isolated, workspace):
    result = runner.invoke(app, ["run", "Create a Python hello-world project", "-p", str(workspace)])
    assert result.exit_code == 0, result.stdout
    assert (workspace / "hello.py").is_file()
