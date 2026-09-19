"""shadow | shadow /path | shadow run \"…\" | shadow models | shadow config | shadow ui | shadow health"""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path
from typing import Optional

import typer
from rich.console import Console

_COMMANDS = {"run", "models", "config", "ui", "health", "doctor", "export", "sessions"}

from shadow_agent import __version__, paths
from shadow_agent.agent.loop import AgentRunner
from shadow_agent.config import ensure_user_config, last_workspace, load_config, remember_workspace, set_config_value
from shadow_agent.events import EventBus
from shadow_agent.export import export_session
from shadow_agent.health import collect_health
from shadow_agent.models.registry import ModelRegistry
from shadow_agent.secrets import load_secrets
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


def _ui_workspace(project: Optional[Path]) -> Path:
    if project is not None:
        return _workspace(project)
    env_ws = os.environ.get("SHADOW_AGENT_WORKSPACE")
    if env_ws:
        return _workspace(Path(env_ws))
    remembered = last_workspace()
    if remembered is not None:
        return remembered
    return Path.cwd().resolve()


@app.callback(invoke_without_command=True)
def main(
    ctx: typer.Context,
    version: bool = typer.Option(False, "--version", help="Show version and exit"),
    project: Optional[Path] = typer.Option(None, "--project", "-p", help="Workspace path"),
) -> None:
    load_secrets()
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
    workspace = _ui_workspace(project)
    remember_workspace(workspace)
    cfg = load_config(workspace)
    bind_host = host or cfg.ui.host
    bind_port = port or cfg.ui.port
    if no_browser:
        from shadow_agent.api.server import serve

        serve(bind_host, bind_port, workspace)
        return
    from shadow_agent.desktop import launch_desktop

    launch_desktop(workspace, bind_host, bind_port)


@app.command()
def health(
    project: Optional[Path] = typer.Option(None, "--project", "-p"),
) -> None:
    """Ping the configured provider and detect git / python / docker."""
    workspace = _ui_workspace(project)
    cfg = load_config(workspace)
    payload = collect_health(cfg, workspace)
    console.print(json.dumps(payload, indent=2))
    provider_ok = bool(payload.get("provider", {}).get("ok"))
    raise typer.Exit(0 if payload.get("ok") and provider_ok else 1)


@app.command()
def doctor(
    project: Optional[Path] = typer.Option(None, "--project", "-p"),
) -> None:
    """Alias for health."""
    health(project)


@app.command()
def sessions() -> None:
    """List recent sessions."""
    store = Store()
    rows = store.list_sessions()
    if not rows:
        console.print("No sessions yet. Run a task or open the UI.")
        return
    for row in rows:
        title = row.get("title") or "(untitled)"
        console.print(f"{row['id'][:8]}  {row['status']:10}  {title}  {row['workspace']}")


@app.command("export")
def export_cmd(
    session_id: Optional[str] = typer.Option(None, "--session", "-s"),
    fmt: str = typer.Option("md", "--format"),
    output: Optional[Path] = typer.Option(None, "--output", "-o"),
) -> None:
    """Write a session transcript to Markdown or JSON."""
    store = Store()
    sid = session_id
    if not sid:
        rows = store.list_sessions(limit=1)
        if not rows:
            console.print("No sessions to export.")
            raise typer.Exit(1)
        sid = rows[0]["id"]
    try:
        body, _media = export_session(store, sid, fmt=fmt)
    except KeyError:
        console.print("Session not found.")
        raise typer.Exit(1)
    if output:
        output.write_text(body, encoding="utf-8")
        console.print(f"wrote {output}")
        return
    console.print(body)


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
        elif etype == "tool.parallel":
            console.print(f"  ∥ {payload.get('count')} read tools")
        elif etype in {"test.failed", "test.passed"}:
            console.print(f"[yellow]{etype}[/yellow]")
        elif etype == "model.delta":
            pass
        elif etype == "agent.completed":
            usage = payload.get("usage") or {}
            extra = f"  tokens={usage.get('total_tokens', 0)}" if usage else ""
            console.print(f"[bold]{etype}[/bold] success={payload.get('success')}{extra}")

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
    if not overlay.onboarding.completed:
        console.print("First run: type a task, or run [bold]shadow ui[/bold] for the 60-second setup wizard.")
    console.print("Type a task, or /quit  /models  /config  /ui  /health")
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
        if line in {"/health", "/doctor"}:
            try:
                health(workspace)
            except typer.Exit:
                pass
            continue
        if line == "/ui":
            ui(workspace, None, None, False)
            return
        _run_task(workspace, line)


def entry() -> None:
    """Allow `shadow /path/to/project` without stealing subcommands."""
    load_secrets()
    args = sys.argv[1:]
    if args and not args[0].startswith("-") and args[0] not in _COMMANDS:
        candidate = Path(args[0]).expanduser()
        if candidate.is_dir():
            os.environ["SHADOW_AGENT_WORKSPACE"] = str(candidate.resolve())
            sys.argv = [sys.argv[0], *args[1:]]
    app()
