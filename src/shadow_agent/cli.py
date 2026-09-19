"""shadow | shadow /path | shadow run \"…\" | shadow models | shadow config | shadow ui"""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path
from typing import Optional

import typer
from rich.console import Console

_COMMANDS = {"run", "models", "config", "ui"}

from shadow_agent import __version__, paths
from shadow_agent.agent.loop import AgentRunner
from shadow_agent.config import ensure_user_config, load_config, set_config_value
from shadow_agent.events import EventBus
from shadow_agent.models.registry import ModelRegistry
from shadow_agent.store import Store

app = typer.Typer(
    add_completion=False,
    no_args_is_help=False,
    help="Shadow Agent — Linux-native autonomous coding harness.",
)
console = Console()


def _workspace(project: Optional[Path]) -> Path:
    path = (project or Path.cwd()).expanduser().resolve()
    if not path.is_dir():
        raise typer.BadParameter(f"not a directory: {path}")
    return path


@app.callback(invoke_without_command=True)
def main(
    ctx: typer.Context,
    version: bool = typer.Option(False, "--version", help="Show version and exit"),
    project: Optional[Path] = typer.Option(None, "--project", "-p", help="Workspace path"),
) -> None:
    if version:
        console.print(f"shadow-agent {__version__}")
        raise typer.Exit()
    if ctx.invoked_subcommand is not None:
        return
    env_ws = os.environ.get("SHADOW_AGENT_WORKSPACE")
    workspace = _workspace(project or (Path(env_ws) if env_ws else Path.cwd()))
    _interactive(workspace)


@app.command()
def run(
    task: str = typer.Argument(..., help="Task for the agent loop"),
    project: Optional[Path] = typer.Option(None, "--project", "-p", help="Workspace path"),
) -> None:
    """Run one task through the agent loop and print the result."""
    workspace = _workspace(project)
    result = _run_task(workspace, task)
    raise typer.Exit(0 if result.success else 1)


@app.command()
def models() -> None:
    """List registered models and the active default."""
    cfg = ensure_user_config()
    registry = ModelRegistry()
    console.print(f"default: {cfg.model.default}  provider={cfg.model.provider}")
    if cfg.model.endpoint:
        console.print(f"endpoint: {cfg.model.endpoint}  key_env={cfg.model.api_key_env}")
    for info in registry.list_models():
        mark = "*" if info.id == cfg.model.default else " "
        console.print(f"{mark} {info.id:10} {info.provider:20} {info.endpoint}")


@app.command("config")
def config_cmd(
    key: Optional[str] = typer.Argument(None, help="Dotted key to set, e.g. model.default"),
    value: Optional[str] = typer.Argument(None, help="Value to assign"),
) -> None:
    """Show or set ~/.config/shadow-agent/config.yaml."""
    if key and value is not None:
        cfg = set_config_value(key, value)
        console.print(f"set {key} = {value}")
    else:
        cfg = ensure_user_config()
        console.print(f"file: {paths.config_file()}")
        console.print(json.dumps(cfg.model_dump(mode="json"), indent=2))


@app.command()
def ui(
    project: Optional[Path] = typer.Option(None, "--project", "-p"),
    host: Optional[str] = typer.Option(None),
    port: Optional[int] = typer.Option(None),
    no_browser: bool = typer.Option(False, "--no-browser"),
) -> None:
    """Start the desktop Agent API + UI on loopback."""
    workspace = _workspace(project) if project else Path.cwd()
    cfg = load_config(workspace)
    bind_host = host or cfg.ui.host
    bind_port = port or cfg.ui.port
    if no_browser:
        from shadow_agent.api.server import serve

        serve(bind_host, bind_port, workspace)
        return
    from shadow_agent.desktop import launch_desktop

    launch_desktop(workspace, bind_host, bind_port)


def _run_task(workspace: Path, task: str):
    ensure_user_config()
    cfg = load_config(workspace)
    store = Store()
    bus = EventBus()

    def printer(_etype: str, event: dict) -> None:
        payload = event.get("payload") or {}
        etype = event["type"]
        if etype == "agent.started":
            console.print(f"[bold]started[/bold]  {payload.get('task')}")
        elif etype == "agent.planning":
            console.print("[cyan]planning[/cyan]")
        elif etype == "tool.started":
            console.print(f"  → {payload.get('tool')} {payload.get('arguments', {})}")
        elif etype == "tool.completed":
            ok = "ok" if payload.get("success") else "fail"
            console.print(f"  ← {payload.get('tool')} [{ok}]")
        elif etype in {"test.failed", "test.passed"}:
            console.print(f"[yellow]{etype}[/yellow]")
        elif etype == "agent.completed":
            console.print(f"[bold]{etype}[/bold] success={payload.get('success')}")

    bus.subscribe(None, printer)
    runner = AgentRunner(workspace, config=cfg, store=store, events=bus)
    result = runner.run(task)
    console.print("")
    console.print(result.summary)
    return result


def _interactive(workspace: Path) -> None:
    cfg = ensure_user_config()
    overlay = load_config(workspace)
    console.print(f"Shadow Agent {__version__}  ·  {workspace}")
    console.print(f"model={overlay.model.default}  provider={overlay.model.provider}  level={overlay.permissions.level.value}")
    console.print("Type a task, or /quit  /models  /config  /ui")
    while True:
        try:
            line = console.input("[bold]>[/bold] ").strip()
        except (EOFError, KeyboardInterrupt):
            console.print("")
            return
        if not line:
            continue
        if line in {"/quit", "/exit", "quit"}:
            return
        if line == "/models":
            models()
            continue
        if line == "/config":
            config_cmd(None, None)
            continue
        if line == "/ui":
            ui(workspace, None, None, False)
            return
        _run_task(workspace, line)


def entry() -> None:
    """Allow `shadow /path/to/project` without stealing subcommands."""
    args = sys.argv[1:]
    if args and not args[0].startswith("-") and args[0] not in _COMMANDS:
        candidate = Path(args[0]).expanduser()
        if candidate.is_dir():
            os.environ["SHADOW_AGENT_WORKSPACE"] = str(candidate.resolve())
            sys.argv = [sys.argv[0], *args[1:]]
    app()
