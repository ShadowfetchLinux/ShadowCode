"""shadow | shadow /path | shadow run \"…\" | shadow models | shadow config | shadow ui | shadow health"""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path
from typing import Optional

import typer
from rich.console import Console

_COMMANDS = {"run", "models", "config", "ui", "tui", "health", "doctor", "export", "sessions", "mcp", "background", "plugin", "rewind", "skill", "goal", "goals", "status", "jobs", "tools", "profile", "understand", "why", "vision", "docs", "rollback", "checkpoints", "team"}

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
    help="ShadowCode — Linux-native autonomous coding-agent harness (brand: ShadowCode; binary: shadow).",
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
    # Codex-style terminal UI is the default landing experience when a tty is
    # attached; headless callers (cron, pipes) fall back to the desktop UI.
    if sys.stdin.isatty() and sys.stdout.isatty():
        from shadow_agent.tui.app import run_tui

        run_tui(workspace)
    else:
        _interactive(workspace)


@app.command()
def run(
    task: str = typer.Argument(..., help="Task for the agent loop"),
    project: Optional[Path] = typer.Option(None, "--project", "-p", help="Workspace path"),
    agent: Optional[str] = typer.Option(None, "--agent", help="Dispatch through a subagent role (architect, coder, security, tester, researcher, linux-expert, reviewer)"),
    json_out: bool = typer.Option(False, "--json", help="Print structured JSON result and exit non-zero on failure"),
    interactive: bool = typer.Option(False, "--interactive", help="Force interactive prompts even when stdin is not a tty"),
) -> None:
    """Run one task through the agent loop (non-interactive by default).

    CI mode: ``shadow run --json "review this pull request"`` exits 0 on
    success and 1 on failure with structured JSON on stdout. ``--agent security``
    dispatches through the named subagent role.
    """
    workspace = _workspace(project)
    if agent:
        result = _run_task_with_agent(workspace, task, agent, json_out=json_out)
    else:
        result = _run_task(workspace, task, json_out=json_out)
    if json_out:
        console.print(json.dumps(result.model_dump(mode="json"), indent=2, default=str))
    raise typer.Exit(0 if result.success else 1)


@app.command()
def models(
    detect: bool = typer.Option(True, "--detect/--no-detect", help="Probe local servers and list installed models"),
    use: Optional[str] = typer.Option(None, "--use", "-u", help="Set the default model to this id (free-text ok)"),
    provider: Optional[str] = typer.Option(None, "--provider", help="Provider for --use when the id is custom"),
    endpoint: Optional[str] = typer.Option(None, "--endpoint", help="Endpoint for --use when the id is custom"),
) -> None:
    """List registered + detected models across providers, or set the default."""
    cfg = ensure_user_config()
    if use:
        from shadow_agent.models.registry import ModelRegistry as _Reg

        registry = _Reg()
        info = registry.get(use)
        if info is None:
            prov = (provider or cfg.model.provider).lower()
            if not prov:
                raise typer.BadParameter("unknown model; pass --provider to register a custom one")
            info = registry.register_custom(use, prov, name=use, endpoint=endpoint or "", api_key_env="", context_limit=0)
        cfg.model.default = info.id
        cfg.model.provider = info.provider
        cfg.model.endpoint = endpoint or info.endpoint or cfg.model.endpoint
        cfg.model.name = str(info.metadata.get("model") or info.id)
        if info.metadata.get("api_key_env"):
            cfg.model.api_key_env = str(info.metadata["api_key_env"])
        if info.context_limit:
            cfg.model.context_limit = info.context_limit
        from shadow_agent.config import save_config

        save_config(cfg)
        console.print(f"default model → {cfg.model.default}  provider={cfg.model.provider}")
        raise typer.Exit()
    registry = ModelRegistry(detect=detect)
    console.print(f"default: {cfg.model.default}  provider={cfg.model.provider}")
    if cfg.model.endpoint:
        console.print(f"endpoint: {cfg.model.endpoint}  key_env={cfg.model.api_key_env}")
    for info in registry.list_models():
        mark = "*" if info.id == cfg.model.default else " "
        detected_tag = "detected" if info.metadata.get("detected") else ("custom" if info.metadata.get("custom") else "builtin")
        console.print(f"{mark} {info.id:24} {info.provider:18} {detected_tag:8} {info.endpoint}")
    if detect:
        from shadow_agent.models.discovery import detect_providers

        console.print("\n[bold]Local servers[/bold]")
        for found in detect_providers():
            if not found.running:
                console.print(f"  [dim]· {found.label}: {found.detail}[/dim]")
                continue
            console.print(f"  [green]●[/green] {found.label} ({found.endpoint}) — {found.detail}")
            for model in found.models:
                caps = ",".join(k for k, v in model.capabilities.items() if v and k != "completion")
                suffix = f"  [{caps}]" if caps else ""
                console.print(f"      {model.id}  {model.detail}{suffix}")


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
    json_out: bool = typer.Option(False, "--json", help="Print the raw report as JSON"),
    fix: bool = typer.Option(False, "--fix", help="Apply safe auto-fixes (chmod secrets, reinstall wrapper/icon/desktop entry)"),
) -> None:
    """Deep install/config checks with auto-fix suggestions."""
    from shadow_agent.health import doctor_report, doctor_fix

    workspace = _ui_workspace(project)
    cfg = load_config(workspace)
    report = doctor_report(cfg, workspace)
    if fix:
        applied = doctor_fix(report, workspace)
        if applied:
            console.print("[green]Applied auto-fixes:[/green]")
            for item in applied:
                console.print(f"  • {item}")
        else:
            console.print("[dim]No auto-fixable issues found.[/dim]")
        report = doctor_report(cfg, workspace)
    if json_out:
        console.print(json.dumps(report, indent=2))
    else:
        console.print(f"[bold]shadow doctor[/bold]  v{report['version']}")
        for check in report["checks"]:
            mark = "[green]✓[/green]" if check["ok"] else "[red]✗[/red]"
            detail = f"  [dim]{check['detail']}[/dim]" if check.get("detail") else ""
            console.print(f"{mark} {check['label']}{detail}")
            if not check["ok"] and check.get("fix"):
                console.print(f"    [yellow]fix:[/yellow] {check['fix']}")
        if report["ok"]:
            console.print("[green]All checks passed.[/green]")
        else:
            console.print(f"[red]{len(report['suggestions'])} issue(s) need attention.[/red]")
    raise typer.Exit(0 if report["ok"] else 1)


@app.command()
def sessions() -> None:
    """List recent sessions (with branch marker and token totals)."""
    store = Store()
    rows = store.list_sessions()
    if not rows:
        console.print("No sessions yet. Run a task or open the UI.")
        return
    for row in rows:
        title = row.get("title") or "(untitled)"
        branch = " ↳ branch" if row.get("parent_id") else ""
        usage = row.get("usage_json") or ""
        tok = ""
        if usage:
            try:
                tok = f"  {json.loads(usage).get('total_tokens', 0)} tok"
            except json.JSONDecodeError:
                tok = ""
        console.print(f"{row['id'][:8]}  {row['status']:10}  {title}{branch}{tok}  {row['workspace']}")


@app.command()
def tui(
    project: Optional[Path] = typer.Option(None, "--project", "-p"),
    session: Optional[str] = typer.Option(None, "--session", "-s", help="Resume a session by id (prefix match)"),
) -> None:
    """Open the Codex-style terminal UI."""
    workspace = _ui_workspace(project)
    from shadow_agent.tui.app import run_tui

    run_tui(workspace, resume_session=session)


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


# --- mcp --------------------------------------------------------------------

mcp_app = typer.Typer(help="Expose ShadowCode as an MCP (Model Context Protocol) server.", no_args_is_help=True)


@mcp_app.command("serve")
def mcp_serve(
    http: Optional[str] = typer.Option(None, "--http", help="Bind an HTTP/SSE server on host:port (e.g. 127.0.0.1:7431). Omit for stdio."),
    project: Optional[Path] = typer.Option(None, "--project", "-p", help="Workspace path the server defaults to."),
) -> None:
    """Run the ShadowCode MCP server (stdio by default, or HTTP/SSE with --http)."""
    from shadow_agent.mcp_server.server import serve_http, serve_stdio

    workspace = _ui_workspace(project)
    if http:
        host, _, port_str = http.partition(":")
        host = host or "127.0.0.1"
        port = int(port_str) if port_str else 7431
        serve_http(host=host, port=port, workspace=workspace)
    else:
        serve_stdio(workspace=workspace)


@mcp_app.command("register")
def mcp_register(
    host: str = typer.Option("127.0.0.1", "--host", help="Host for the HTTP/SSE block."),
    port: int = typer.Option(7431, "--port", help="Port for the HTTP/SSE block."),
    no_token: bool = typer.Option(False, "--no-token", help="Do not create a token if none exists."),
) -> None:
    """Print JSON config blocks to paste into Claude Code / Cursor / Codex."""
    from shadow_agent.mcp_server.register import print_register_blocks

    print_register_blocks(host=host, port=port, ensure=not no_token)


app.add_typer(mcp_app, name="mcp")


def _run_task(workspace: Path, task: str, json_out: bool = False):
    ensure_user_config()
    cfg = load_config(workspace)
    store = Store()
    bus = EventBus()

    def printer(_etype: str, event: dict) -> None:
        if json_out:
            return  # suppress all event printing in JSON mode
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
            icon = payload.get("icon") or ("✓" if payload.get("success") else "✗")
            headline = payload.get("headline") or payload.get("tool") or ""
            console.print(f"  {icon} {headline}  [{ok}]")
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
    if not json_out:
        console.print("")
        console.print(result.summary)
    return result


def _run_task_with_agent(workspace: Path, task: str, agent: str, json_out: bool = False):
    """Dispatch a task through a single subagent role (CI mode)."""
    from shadow_agent.agent.loop import AgentRunner
    from shadow_agent.agents_dir import SubagentRole

    ensure_user_config()
    cfg = load_config(workspace)
    store = Store()
    bus = EventBus()
    try:
        role = SubagentRole(agent)
    except ValueError:
        valid = ", ".join(r.value for r in SubagentRole)
        raise typer.BadParameter(f"unknown agent role: {agent!r}. Choose one of: {valid}")
    from shadow_agent.models.adapters.mock import MockProvider

    runner = AgentRunner(workspace, config=cfg, store=store, events=bus, model=MockProvider())
    from shadow_agent.agent.subagents import SubagentHost

    host = SubagentHost(runner)
    sub = host.spawn(role, task)
    # Wrap as an AgentResult-shaped dict for the CLI.
    from shadow_agent.agent.loop import AgentResult

    child = sub.result
    return AgentResult(
        success=child.get("success", True),
        summary=sub.notes or child.get("summary", ""),
        session_id=child.get("session_id", runner.session_id),
        task_id=child.get("task_id", ""),
        steps=child.get("steps", 0),
        plan=child.get("plan", {"steps": []}),
        events=child.get("events", []),
        todos=child.get("todos", []),
        usage=child.get("usage", {}),
        cancelled=child.get("cancelled", False),
        stage=child.get("stage", "DONE"),
        fix_retries=child.get("fix_retries", 0),
        stage_history=child.get("stage_history", []),
    )


def _interactive(workspace: Path) -> None:
    cfg = ensure_user_config()
    overlay = load_config(workspace)
    console.print(f"ShadowCode {__version__}  ·  {workspace}")
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


# --- ShadowCode 0.8.0 second-wave commands -----------------------------------


@app.command()
def background(
    project: Optional[Path] = typer.Option(None, "--project", "-p"),
    action: str = typer.Argument("list", help="list | start | stop"),
    name: Optional[str] = typer.Option(None, "--name", help="Task name (for start)"),
    command: Optional[str] = typer.Option(None, "--command", help="Shell command (for start)"),
    task_id: Optional[str] = typer.Argument(None, help="Task id (for stop)"),
) -> None:
    """Manage background tasks: `shadow background start --name dev --command 'npm run dev'`."""
    from shadow_agent.background import BackgroundManager

    workspace = _workspace(project) if project else Path.cwd()
    mgr = BackgroundManager()
    if action == "list":
        console.print(mgr.render_panel())
        return
    if action == "start":
        if not command:
            raise typer.BadParameter("--command is required for start")
        task = mgr.start(name or "background", command, cwd=workspace)
        console.print(f"#{task.id} {task.name} {task.status.value} pid {task.pid}")
        return
    if action == "stop":
        if not task_id:
            rows = mgr.list()
            if not rows:
                console.print("No background tasks.")
                return
            task_id = rows[0].id
        stopped = mgr.stop(task_id)
        if stopped is None:
            console.print(f"[red]no task {task_id}[/red]")
            raise typer.Exit(1)
        console.print(f"#{stopped.id} {stopped.name} {stopped.status.value}")
        return
    raise typer.BadParameter(f"unknown action: {action}. Use list | start | stop")


@app.command()
def plugin(
    action: str = typer.Argument("list", help="list | install | remove | registry"),
    name: Optional[str] = typer.Argument(None, help="Plugin name (for install/remove)"),
) -> None:
    """Manage plugins: `shadow plugin install python-expert`."""
    from shadow_agent.plugin_registry import PluginRegistry

    reg = PluginRegistry()
    if action == "list":
        installed = reg.list_installed()
        if not installed:
            console.print("No plugins installed. Try `shadow plugin registry`.")
            return
        for m in installed:
            console.print(f"  {m.name:20} {m.version:10} {m.description}")
        return
    if action == "registry":
        for name_ in reg.list_registry():
            mark = "✓" if reg.is_installed(name_) else " "
            console.print(f"  {mark} {name_}")
        return
    if action == "install":
        if not name:
            raise typer.BadParameter("plugin name is required")
        try:
            manifest = reg.install(name)
        except KeyError as exc:
            console.print(f"[red]{exc}[/red]")
            raise typer.Exit(1)
        console.print(f"[green]installed[/green] {manifest.name} {manifest.version}")
        console.print(f"  {manifest.description}")
        return
    if action == "remove":
        if not name:
            raise typer.BadParameter("plugin name is required")
        if reg.remove(name):
            console.print(f"[green]removed[/green] {name}")
        else:
            console.print(f"[red]{name} not installed[/red]")
            raise typer.Exit(1)
        return
    raise typer.BadParameter(f"unknown action: {action}. Use list | install | remove | registry")


@app.command()
def rewind(
    project: Optional[Path] = typer.Option(None, "--project", "-p"),
    checkpoint_id: Optional[str] = typer.Argument(None, help="Checkpoint id (e.g. 001)"),
    dimensions: str = typer.Option("files", "--dimensions", "-d", help="Comma-separated: files,conversation,agent_state,memory,git"),
    json_out: bool = typer.Option(False, "--json"),
) -> None:
    """Rewind to a named checkpoint. `shadow rewind 001 -d files,memory`."""
    from shadow_agent.rewind import RewindStore

    workspace = _workspace(project)
    store = RewindStore(workspace)
    if checkpoint_id is None:
        rows = store.list()
        if not rows:
            console.print("No checkpoints yet. Create one with /checkpoint <label>.")
            return
        for r in rows:
            console.print(f"  {r['id']}  {r['label']:30}  files={r.get('file_count', 0)}  ts={r.get('ts', 0):.0f}")
        return
    dims = [d.strip() for d in dimensions.split(",") if d.strip()]
    from shadow_agent.store import Store as _Store

    result = store.restore(checkpoint_id, dimensions=dims, store=_Store())
    if json_out:
        console.print(json.dumps(result, indent=2, default=str))
    else:
        if result.get("ok"):
            for dim, rest in result.get("restored", {}).items():
                ok = rest.get("ok") if isinstance(rest, dict) else rest
                console.print(f"  {dim}: {'✓' if ok else '✗'}")
        else:
            console.print(f"[red]{result.get('error')}[/red]")
            raise typer.Exit(1)


@app.command()
def skill(
    project: Optional[Path] = typer.Option(None, "--project", "-p"),
    name: Optional[str] = typer.Argument(None, help="Skill name to run or list"),
    list_only: bool = typer.Option(False, "--list", help="List saved skills"),
) -> None:
    """Run or list saved ShadowCode skills."""
    from shadow_agent.self_skill import list_skills, load_skill

    workspace = _workspace(project)
    if list_only or name is None:
        skills = list_skills(workspace)
        if not skills:
            console.print("No skills saved. Create one with /skill-create <name>.")
            return
        for s in skills:
            console.print(f"  {s['name']:20}  {s['path']}")
        return
    body = load_skill(workspace, name)
    if body is None:
        console.print(f"[red]no skill named {name}[/red]")
        raise typer.Exit(1)
    console.print(f"[green]running skill[/green] {name}")
    result = _run_task(workspace, body)
    raise typer.Exit(0 if result.success else 1)


# --- ShadowCode 0.14.0 flagship 15-pillar commands ---------------------------------
# These complement the parallel agents' commands (background/plugin/rewind/skill)
# with the rest of the 15-pillar vision: goal, understand, why, vision, team,
# profile, tools, status, jobs, docs, rollback, checkpoints. Additive only.


@app.command()
def goal(
    task: str = typer.Argument(..., help="One-line instruction to turn into a goal"),
    project: Optional[Path] = typer.Option(None, "--project", "-p"),
    run: bool = typer.Option(False, "--run", help="Run the agent through the milestones now"),
) -> None:
    """Turn a one-line instruction into a project with milestones + progress."""
    from shadow_agent.goal import GoalStore, plan_milestones, render_goal

    workspace = _workspace(project)
    store = GoalStore()
    milestones = plan_milestones(task)
    g = store.create_goal(workspace, task, milestones)
    console.print(render_goal(g))
    if run:
        console.print("\n[bold]Running goal through the agent…[/bold]")
        _run_goal(workspace, g["id"])
    raise typer.Exit(0)


@app.command()
def goals(
    project: Optional[Path] = typer.Option(None, "--project", "-p"),
) -> None:
    """List goals for this workspace."""
    from shadow_agent.goal import GoalStore, render_goal

    workspace = _workspace(project)
    store = GoalStore()
    rows = store.list_goals(workspace)
    if not rows:
        console.print("No goals yet. Create one with `shadow goal <task>`.")
        raise typer.Exit()
    for g in rows:
        console.print(render_goal(g))
        console.print("")
    raise typer.Exit(0)


@app.command()
def status(
    project: Optional[Path] = typer.Option(None, "--project", "-p"),
) -> None:
    """Show project, agent active, runtime, and milestone checklist."""
    from shadow_agent.jobs import JobManager, render_status

    workspace = _workspace(project)
    mgr = JobManager()
    snap = mgr.snapshot(workspace)
    console.print(render_status(snap))
    raise typer.Exit(0)


@app.command()
def jobs(
    project: Optional[Path] = typer.Option(None, "--project", "-p"),
) -> None:
    """List long-running jobs (survive the UI closing)."""
    from shadow_agent.jobs import JobStore, render_status

    workspace = _workspace(project) if project else None
    store = JobStore()
    rows = store.list(workspace=workspace)
    snap = {"workspace": str(workspace) if workspace else "", "active_jobs": sum(1 for r in rows if r["status"] == "running"), "jobs": rows}
    console.print(render_status(snap))
    raise typer.Exit(0)


@app.command()
def tools(
    project: Optional[Path] = typer.Option(None, "--project", "-p"),
    install: Optional[str] = typer.Option(None, "--install", help="Install a tool pack by name"),
) -> None:
    """List installed + available tools (the marketplace)."""
    from shadow_agent.tools.marketplace import install_pack, marketplace_view, render_marketplace
    from shadow_agent.tools.registry import default_tools
    from shadow_agent.tools.sandbox import WorkspaceSandbox
    from shadow_agent.permissions import PermissionGate
    from shadow_agent.config import PermissionLevel

    workspace = _workspace(project)
    sandbox = WorkspaceSandbox(workspace)
    gate = PermissionGate(PermissionLevel.WORKSPACE)
    reg = default_tools(sandbox, gate, 60)
    if install:
        added = install_pack(reg, install)
        console.print(f"Installed {added} tool(s) from pack '{install}'.")
        return
    view = marketplace_view(reg)
    console.print(render_marketplace(view))
    raise typer.Exit(0)


@app.command()
def profile(
    name: Optional[str] = typer.Argument(None, help="safe | developer | autonomous | locked"),
    project: Optional[Path] = typer.Option(None, "--project", "-p"),
) -> None:
    """Show or switch the permission profile."""
    from shadow_agent.profiles import apply_profile, current_profile, render_profiles
    from shadow_agent.config import save_config

    workspace = _workspace(project) if project else None
    cfg = load_config(workspace) if workspace else ensure_user_config()
    if name:
        try:
            cfg, prof = apply_profile(cfg, name)
        except ValueError as exc:
            console.print(f"[red]{exc}[/red]")
            raise typer.Exit(1)
        save_config(cfg)
        console.print(f"Profile → {prof.name}: {prof.description}")
        raise typer.Exit(0)
    cur = current_profile(cfg)
    console.print(render_profiles(cur))
    raise typer.Exit(0)


@app.command()
def understand(
    project: Optional[Path] = typer.Option(None, "--project", "-p"),
) -> None:
    """Analyze the repo and save a project map to .shadow/memory/project.md."""
    from shadow_agent.understand import save_project_map, render_map

    workspace = _workspace(project)
    mp = save_project_map(workspace)
    console.print(render_map(mp))
    console.print(f"\n[dim]Saved to {workspace}/.shadow/memory/project.md[/dim]")
    raise typer.Exit(0)


@app.command()
def why(
    target: Optional[str] = typer.Argument(None, help="File path or task id to explain"),
    project: Optional[Path] = typer.Option(None, "--project", "-p"),
) -> None:
    """Explain a change: files changed, reason, related commits."""
    from shadow_agent.why import explain_change, render_explanation

    workspace = _workspace(project)
    expl = explain_change(workspace, target)
    console.print(render_explanation(expl))
    raise typer.Exit(0 if expl.get("ok") else 1)


@app.command()
def vision(
    image: str = typer.Argument(..., help="Path to the screenshot to analyze"),
    project: Optional[Path] = typer.Option(None, "--project", "-p"),
) -> None:
    """Send a screenshot to a vision model for debugging analysis."""
    from shadow_agent.vision import analyze_screenshot, render_analysis

    workspace = _workspace(project)
    cfg = load_config(workspace)
    result = analyze_screenshot(workspace, image, cfg)
    console.print(render_analysis(result))
    raise typer.Exit(0 if result.get("ok") else 1)


@app.command()
def docs(
    task: str = typer.Argument(..., help="What to implement, e.g. 'implement GTK 4.20 support'"),
    url: Optional[str] = typer.Option(None, "--url", help="Documentation URL to read"),
    project: Optional[Path] = typer.Option(None, "--project", "-p"),
) -> None:
    """Research docs and plan a migration: SEARCH → READ → COMPARE → PLAN → IMPLEMENT → TEST."""
    from shadow_agent.docs_research import plan_doc_research, render_plan

    _workspace(project)
    plan = plan_doc_research(task, url or "")
    console.print(render_plan(plan))
    raise typer.Exit(0)


@app.command()
def rollback(
    name: str = typer.Argument(..., help="Named checkpoint to restore"),
    project: Optional[Path] = typer.Option(None, "--project", "-p"),
) -> None:
    """Restore files from a named checkpoint."""
    from shadow_agent.checkpoints import rollback_named

    workspace = _workspace(project)
    result = rollback_named(workspace, name)
    if result.get("ok"):
        console.print(f"[green]Restored {result['files']} file(s) from checkpoint '{name}'[/green]")
        for p in result.get("restored", [])[:12]:
            console.print(f"  • {p}")
        if result.get("git_head"):
            console.print(f"[dim]git HEAD at checkpoint: {result['git_head'][:8]}[/dim]")
    else:
        console.print(f"[red]{result.get('error')}[/red]")
        raise typer.Exit(1)
    raise typer.Exit(0)


@app.command()
def checkpoints(
    project: Optional[Path] = typer.Option(None, "--project", "-p"),
) -> None:
    """List named checkpoints in this workspace."""
    from shadow_agent.checkpoints import list_named_checkpoints

    workspace = _workspace(project)
    rows = list_named_checkpoints(workspace)
    if not rows:
        console.print("No named checkpoints. Create one with the agent or `shadow rewind`.")
        raise typer.Exit(0)
    for r in rows:
        console.print(f"  • {r['name']:20}  {r['files']} file(s)  git={r.get('git_head', '')[:8]}")
    raise typer.Exit(0)


@app.command()
def team(
    task: str = typer.Argument(..., help="Task for the agent team"),
    project: Optional[Path] = typer.Option(None, "--project", "-p"),
) -> None:
    """Run a multi-agent team (LEAD + ARCHITECT/CODER/TESTER/SECURITY/REVIEWER)."""
    from shadow_agent.teams import AgentTeam

    workspace = _workspace(project)
    cfg = load_config(workspace)
    team = AgentTeam(workspace, config=cfg)
    report = team.run(task)
    console.print(report.combined)
    raise typer.Exit(0 if report.success else 1)


def _run_goal(workspace: Path, goal_id: str) -> None:
    """Run an agent through each milestone of a goal, marking them done as VERIFY passes."""
    from shadow_agent.goal import GoalStore, render_goal

    store = GoalStore()
    goal = store.get_goal(goal_id)
    if not goal:
        console.print(f"[red]goal {goal_id} not found[/red]")
        return
    for milestone in goal.get("milestones", []):
        if milestone["status"] == "done":
            continue
        store.update_milestone(goal_id, milestone["id"], "in_progress")
        task_text = f"{goal['instruction']}\nMilestone: {milestone['title']}"
        result = _run_task(workspace, task_text)
        new_status = "done" if result.success else "failed"
        store.update_milestone(goal_id, milestone["id"], new_status, task_id=result.task_id, detail=result.summary[:200])
        if not result.success:
            console.print(f"[red]Milestone failed: {milestone['title']}[/red]")
            break
    g = store.get_goal(goal_id)
    if g:
        console.print("\n[bold]Goal progress:[/bold]")
        console.print(render_goal(g))


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
