"""Built-in slash-command handlers.

Each handler is `def handler(args: str, ctx: CommandContext) -> CommandResult`.
Both the TUI and the desktop API construct a `CommandContext` and call
`dispatch(args, ctx, name)` so the two UIs share identical behavior.

Handlers return a structured `CommandResult` (text/card/list/diff/approval/
overlay/error) so the UI can render Codex-style cards. They never raise —
errors come back as `kind="error"` cards.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable

from shadow_agent import __version__
from shadow_agent.commands import CommandResult
from shadow_agent.config import (
    AppConfig,
    apply_config_patch,
    load_config,
    save_config,
    set_config_value,
)


@dataclass
class CommandContext:
    """Shared dependencies for slash-command handlers."""

    workspace: Path
    config: AppConfig | None = None
    store: Any = None
    registry: Any = None  # ModelRegistry
    commands: Any = None  # CommandRegistry
    approvals: Any = None  # ApprovalHub (for /commit destructive gating)
    session_id: str = ""
    # TUI-only callbacks the handler can request:
    overlay_opener: Callable[[str], None] | None = None
    compactor: Callable[[], None] | None = None
    clearer: Callable[[], None] | None = None
    new_session: Callable[[], str] | None = None
    branch_session: Callable[[], str] | None = None
    resume_session: Callable[[str], str | None] | None = None
    pin_last: Callable[[str], None] | None = None
    ui_launcher: Callable[[], None] | None = None
    quitter: Callable[[], None] | None = None
    # Read-only extras the API can supply:
    extra: dict[str, Any] = field(default_factory=dict)

    def cfg(self) -> AppConfig:
        if self.config is None:
            self.config = load_config(self.workspace)
        return self.config


HANDLERS: dict[str, Callable[[str, CommandContext], CommandResult]] = {}


def register(name: str) -> Callable[[Callable[[str, CommandContext], CommandResult]], Callable]:
    def deco(fn: Callable[[str, CommandContext], CommandResult]) -> Callable[[str, CommandContext], CommandResult]:
        HANDLERS[name] = fn
        return fn

    return deco


def dispatch(args: str, ctx: CommandContext, name: str) -> CommandResult:
    handler = HANDLERS.get(name)
    if handler is None:
        return CommandResult(handled=False, text=f"Unknown command: /{name}", kind="error")
    try:
        return handler(args, ctx)
    except Exception as exc:  # noqa: BLE001 - never crash the UI over a slash command
        return CommandResult(
            handled=True,
            kind="error",
            headline=f"/{name} failed",
            body=str(exc)[:1000],
        )


def handler_table(ctx: CommandContext) -> dict[str, Callable[[str], CommandResult]]:
    """Bind ctx into each handler so the legacy `dispatch_builtin` API works."""

    def bind(name: str) -> Callable[[str], CommandResult]:
        fn = HANDLERS[name]

        def wrapped(args: str) -> CommandResult:
            return dispatch(args, ctx, name)

        return wrapped

    return {name: bind(name) for name in HANDLERS}


# --- helpers -----------------------------------------------------------------


def _git(workspace: Path, *args: str) -> tuple[int, str, str]:
    proc = subprocess.run(
        ["git", *args],
        cwd=str(workspace),
        capture_output=True,
        text=True,
        check=False,
    )
    return proc.returncode, proc.stdout, proc.stderr


def _is_repo(workspace: Path) -> bool:
    return (workspace / ".git").exists()


def _card(icon: str, headline: str, body: str = "", **extra: Any) -> CommandResult:
    return CommandResult(
        handled=True,
        kind="card",
        icon=icon,
        headline=headline,
        body=body,
        metadata=extra,
    )


def _list_card(headline: str, items: list[dict[str, str]], icon: str = "▸") -> CommandResult:
    return CommandResult(handled=True, kind="list", icon=icon, headline=headline, items=items)


def _text(text: str) -> CommandResult:
    return CommandResult(handled=True, kind="text", text=text)


def _overlay(name: str) -> CommandResult:
    return CommandResult(handled=True, kind="overlay", overlay=name)


def _ok_label(info: dict[str, Any]) -> str:
    if not info:
        return "—"
    if info.get("ok"):
        return f"✓ {info.get('name', '')} — {info.get('detail', '')}".strip(" —")
    return f"✗ {info.get('name', '')} — {info.get('detail', '')}".strip(" —")


# --- brand / orientation -----------------------------------------------------


@register("shadowcode")
def _shadowcode(args: str, ctx: CommandContext) -> CommandResult:
    cfg = ctx.cfg()
    ws = ctx.workspace
    model = f"{cfg.model.provider}/{cfg.model.name or cfg.model.default}"
    perm = cfg.permissions.level.value
    shortcuts = [
        ("Enter", "send task"),
        ("Shift+Enter / Ctrl+J", "newline"),
        ("Esc", "cancel / close overlay"),
        ("Ctrl+K", "command palette (desktop)"),
        ("?", "help overlay"),
        ("/help", "list all commands"),
        ("/status", "agent + provider health"),
        ("/model <id>", "switch model"),
        ("/compact", "compact transcript"),
        ("/diff", "working-tree diff"),
        ("/commit", "stage + commit"),
        ("/undo", "undo last agent file change"),
    ]
    body_lines = [
        f"workspace  {ws}",
        f"model     {model}",
        f"perm      {perm}",
        "",
        "shortcuts",
    ]
    body_lines += [f"  {k:22} {v}" for k, v in shortcuts]
    return _card("◆", f"ShadowCode {__version__}", "\n".join(body_lines))


@register("help")
def _help(args: str, ctx: CommandContext) -> CommandResult:
    if ctx.commands is None:
        from shadow_agent.commands import builtin_commands

        cmds = builtin_commands()
    else:
        cmds = ctx.commands.list()
    items: list[dict[str, str]] = []
    for cmd in cmds:
        suffix = f" {cmd.arg_spec}" if cmd.arg_spec else ""
        items.append({"label": f"/{cmd.name}{suffix}", "value": cmd.description})
    shortcuts = [
        ("Enter", "send task"),
        ("Shift+Enter / Ctrl+J", "newline"),
        ("Esc", "cancel / close overlay"),
        ("Ctrl+K", "command palette (desktop)"),
        ("?", "help overlay"),
    ]
    body = "keybindings\n" + "\n".join(f"  {k:24} {v}" for k, v in shortcuts)
    return CommandResult(
        handled=True,
        kind="list",
        icon="◆",
        headline="Slash commands",
        items=items,
        body=body,
    )


@register("status")
def _status(args: str, ctx: CommandContext) -> CommandResult:
    cfg = ctx.cfg()
    from shadow_agent.health import collect_health, doctor_report

    health = collect_health(cfg, ctx.workspace)
    doc = doctor_report(cfg, ctx.workspace)
    items: list[dict[str, str]] = [
        {"label": "version", "value": __version__},
        {"label": "workspace", "value": str(ctx.workspace)},
        {"label": "model", "value": f"{cfg.model.provider}/{cfg.model.name or cfg.model.default}"},
        {"label": "permission", "value": cfg.permissions.level.value},
        {"label": "provider", "value": _ok_label(health.get("provider", {}))},
        {"label": "python", "value": _ok_label(health.get("tools", {}).get("python", {}))},
        {"label": "git", "value": _ok_label(health.get("tools", {}).get("git", {}))},
        {"label": "doctor", "value": f"{sum(1 for c in doc['checks'] if c['ok'])}/{len(doc['checks'])} checks ok"},
    ]
    if ctx.store is not None and hasattr(ctx.store, "list_tasks"):
        try:
            tasks = ctx.store.list_tasks(limit=5)
            running = [t for t in tasks if t.get("status") == "running"]
            if running:
                items.append({"label": "running jobs", "value": str(len(running))})
        except Exception:
            pass
    if ctx.session_id and ctx.store is not None and hasattr(ctx.store, "session_cost"):
        try:
            cost = ctx.store.session_cost(ctx.session_id)
            tok = int((cost.get("usage") or {}).get("total_tokens", 0))
            items.append({"label": "tokens this session", "value": f"{tok}"})
        except Exception:
            pass
    return _list_card("Agent status", items, icon="◆")


# --- model / context ---------------------------------------------------------


@register("model")
def _model(args: str, ctx: CommandContext) -> CommandResult:
    cfg = ctx.cfg()
    target = args.strip()
    if not target:
        return _card(
            "◆",
            "Active model",
            (
                f"default   {cfg.model.default}\n"
                f"provider  {cfg.model.provider}\n"
                f"name      {cfg.model.name}\n"
                f"endpoint  {cfg.model.endpoint or '(none)'}\n"
                f"key env   {cfg.model.api_key_env}"
            ),
        )
    if ctx.registry is not None:
        info = ctx.registry.get(target)
        if info is None:
            try:
                ctx.registry.refresh_detected()
                info = ctx.registry.get(target)
            except Exception:
                info = None
        if info is not None:
            cfg.model.default = info.id
            cfg.model.provider = info.provider
            cfg.model.endpoint = info.endpoint or cfg.model.endpoint
            cfg.model.name = str(info.metadata.get("model") or info.id)
            if info.metadata.get("api_key_env"):
                cfg.model.api_key_env = str(info.metadata["api_key_env"])
            if info.context_limit:
                cfg.model.context_limit = info.context_limit
            save_config(cfg)
            return _card("✓", f"Switched model → {info.id}", f"provider {info.provider}\nendpoint {info.endpoint or '(default)'}")
    cfg.model.default = target
    if target not in cfg.model.name and not ctx.registry:
        cfg.model.name = target
    save_config(cfg)
    return _card("✓", f"Switched model → {target}", "provider kept as " + cfg.model.provider)


@register("models")
def _models(args: str, ctx: CommandContext) -> CommandResult:
    cfg = ctx.cfg()
    from shadow_agent.models.registry import ModelRegistry

    registry = ctx.registry or ModelRegistry(detect=True)
    items: list[dict[str, str]] = []
    running_providers: set[str] = set()
    try:
        from shadow_agent.models.discovery import detect_providers

        for found in detect_providers():
            if found.running:
                running_providers.add(found.provider)
    except Exception:
        pass
    for info in registry.list_models():
        mark = "*" if info.id == cfg.model.default else " "
        live = "●" if info.provider in running_providers or info.metadata.get("detected") else " "
        items.append(
            {
                "label": f"{mark} {live} {info.id}",
                "value": f"{info.provider}  {info.endpoint or ''}".rstrip(),
            }
        )
    return _list_card(f"Models ({len(items)})", items, icon="◆")


@register("plan")
def _plan(args: str, ctx: CommandContext) -> CommandResult:
    cfg = ctx.cfg()
    plan_text = str((ctx.extra or {}).get("plan") or "")
    todos = (ctx.extra or {}).get("todos") or []
    if args.strip():
        ctx.extra["plan"] = args.strip()
        return _card("✓", "Plan updated", args.strip())
    body_lines = [plan_text or "(no plan yet — run a task)"]
    if todos:
        body_lines += ["", "todos"]
        for t in todos:
            mark = "[x]" if t.get("status") == "done" else "[ ]"
            body_lines.append(f"  {mark} {t.get('title', '')}")
    body_lines += ["", f"compact_ratio  {cfg.agent.compact_ratio}", f"max_steps   {cfg.agent.max_steps}"]
    return _card("◆", "Current plan", "\n".join(body_lines))


@register("compact")
def _compact(args: str, ctx: CommandContext) -> CommandResult:
    if ctx.compactor is not None:
        ctx.compactor()
        return _card("✓", "Compacted transcript", "Older turns were summarized into a single note.")
    return _card("◆", "Compact", "Compaction runs against the live transcript (TUI).")


@register("context")
def _context(args: str, ctx: CommandContext) -> CommandResult:
    cfg = ctx.cfg()
    from shadow_agent.context.engine import estimate_tokens

    items: list[dict[str, str]] = [
        {"label": "context limit", "value": f"{cfg.model.context_limit} tokens"},
        {"label": "compact ratio", "value": f"{cfg.agent.compact_ratio} (auto at {int(cfg.agent.compact_ratio * 100)}%)"},
        {"label": "max steps", "value": str(cfg.agent.max_steps)},
    ]
    if ctx.store is not None and ctx.session_id:
        try:
            events = ctx.store.list_events(session_id=ctx.session_id, limit=400)
            reads = [e for e in events if e["type"] == "tool.completed" and e["payload"].get("tool") == "read_file"]
            items.append({"label": "read_file calls", "value": str(len(reads))})
            tool_results = [e for e in events if e["type"] == "tool.completed"]
            items.append({"label": "tool results", "value": str(len(tool_results))})
            blob = json.dumps([e["payload"] for e in events], default=str)
            items.append({"label": "events size (est.)", "value": f"~{estimate_tokens(blob)} tok"})
        except Exception:
            pass
    return _list_card("Context meter", items, icon="◆")


# --- workspace / git ---------------------------------------------------------


@register("diff")
def _diff(args: str, ctx: CommandContext) -> CommandResult:
    if not _is_repo(ctx.workspace):
        return _card("✗", "Not a git repo", str(ctx.workspace))
    path = args.strip()
    rc, out, err = _git(ctx.workspace, "diff", "--", path) if path else _git(ctx.workspace, "diff")
    if not out.strip():
        _, staged_out, _ = _git(ctx.workspace, "diff", "--cached")
        out = staged_out or out
    if not out.strip():
        return _card("✓", "No changes", "Working tree is clean.")
    return CommandResult(
        handled=True,
        kind="diff",
        path=path or "(working tree)",
        diff=out,
        headline=f"diff {path}".strip(),
    )


@register("review")
def _review(args: str, ctx: CommandContext) -> CommandResult:
    """Lightweight self-review of pending changes.

    Lists changed files and stashes a review prompt on `ctx.extra` so the
    caller can run it as a task. We do NOT auto-dispatch a subagent here
    (that needs a running loop); the UI can offer a "Run review" button.
    """
    if not _is_repo(ctx.workspace):
        return _card("✗", "Not a git repo", str(ctx.workspace))
    _, status_out, _ = _git(ctx.workspace, "status", "--porcelain=v1")
    files = [line[3:] for line in status_out.splitlines() if line.strip() and not line.startswith("##")]
    if not files:
        return _card("✓", "Nothing to review", "Working tree is clean.")
    _, diff_out, _ = _git(ctx.workspace, "diff")
    _, staged_out, _ = _git(ctx.workspace, "diff", "--cached")
    full_diff = (diff_out or "") + (staged_out or "")
    prompt = (
        "Review the pending changes below. For each file, list concrete issues "
        "(bugs, security, style) and a one-line verdict (approve / request-changes). "
        "Do not propose new code; only review.\n\n"
        f"Changed files: {', '.join(files)}\n\n```\n{full_diff[:8000]}\n```"
    )
    ctx.extra["review_prompt"] = prompt
    items = [{"label": f, "value": ""} for f in files]
    return CommandResult(
        handled=True,
        kind="list",
        icon="◆",
        headline="Self-review — pending changes",
        items=items,
        body="Run the review as a task to get model notes. Use `/diff` for the full diff.",
    )


def _shell(workspace: Path, cmd: str, timeout: int = 120) -> tuple[int, str, str]:
    proc = subprocess.run(
        cmd,
        shell=True,
        cwd=str(workspace),
        capture_output=True,
        text=True,
        timeout=timeout,
        check=False,
    )
    return proc.returncode, proc.stdout or "", proc.stderr or ""


@register("test")
def _test(args: str, ctx: CommandContext) -> CommandResult:
    cmd = args.strip()
    if not cmd:
        # Auto-detect: pytest > make test > npm test
        for candidate in ("python3 -m pytest -q", "make test", "npm test"):
            base = candidate.split()[0]
            if shutil.which(base):
                cmd = candidate
                break
        if not cmd:
            return _card("✗", "No test command detected", "Pass one: `/test python3 -m pytest -q`")
    try:
        rc, out, err = _shell(ctx.workspace, cmd, timeout=180)
    except subprocess.TimeoutExpired:
        return _card("✗", f"test · {cmd}", "timed out after 180s")
    ok = rc == 0
    body = (out or "") + (("\n--- stderr ---\n" + err) if err else "")
    if len(body) > 8000:
        body = body[:8000] + "\n… (truncated)"
    return CommandResult(
        handled=True,
        kind="card",
        icon="✓" if ok else "✗",
        headline=f"test · {cmd}",
        body=body or "(no output)",
        metadata={"exit_code": rc, "command": cmd},
    )


@register("run")
def _run(args: str, ctx: CommandContext) -> CommandResult:
    cmd = args.strip()
    if not cmd:
        return _card("✗", "Usage: /run <command>", "Runs a shell command in the workspace.")
    try:
        rc, out, err = _shell(ctx.workspace, cmd, timeout=120)
    except subprocess.TimeoutExpired:
        return _card("✗", f"run · {cmd}", "timed out after 120s")
    ok = rc == 0
    body = (out or "") + (("\n--- stderr ---\n" + err) if err else "")
    if len(body) > 8000:
        body = body[:8000] + "\n… (truncated)"
    return CommandResult(
        handled=True,
        kind="card",
        icon="●" if ok else "✗",
        headline=f"run · {cmd}",
        body=body or "(no output)",
        metadata={"exit_code": rc, "command": cmd},
    )


@register("git")
def _git_cmd(args: str, ctx: CommandContext) -> CommandResult:
    if not _is_repo(ctx.workspace):
        return _card("✗", "Not a git repo", str(ctx.workspace))
    _, status_out, _ = _git(ctx.workspace, "status", "-sb")
    _, log_out, _ = _git(ctx.workspace, "log", "-8", "--oneline", "--decorate")
    _, diff_out, _ = _git(ctx.workspace, "diff", "--stat")
    body = (status_out or "(clean)") + "\n\n" + (diff_out or "") + "\n\n" + (log_out or "")
    return _card("◆", "git status", body)


@register("commit")
def _commit(args: str, ctx: CommandContext) -> CommandResult:
    """Stage all + commit. If the message is missing, generate one from the diff.

    Destructive operations (history-destroying flags) are refused; the
    handler only runs `git add -A` + `git commit -m`. The result is an
    `approval` card so the UI can show the user what will be committed before
    they confirm — but the actual commit happens here, in-process, because
    the slash command surface is synchronous.
    """
    if not _is_repo(ctx.workspace):
        return _card("✗", "Not a git repo", str(ctx.workspace))
    _, status_out, _ = _git(ctx.workspace, "status", "--porcelain=v1")
    files = [line[3:] for line in status_out.splitlines() if line.strip() and not line.startswith("##")]
    if not files:
        return _card("✓", "Nothing to commit", "Working tree is clean.")
    message = args.strip()
    if not message:
        # Generate a message from the diff stat.
        _, stat_out, _ = _git(ctx.workspace, "diff", "--stat")
        first = files[0]
        message = f"shadow: {', '.join(files[:3])}{' …' if len(files) > 3 else ''}"
    # Stage all (workspace paths only — git add -A is safe inside the repo).
    _git(ctx.workspace, "add", "-A")
    rc, out, err = _git(ctx.workspace, "commit", "-m", message)
    if rc != 0:
        return _card("✗", "git commit failed", (err or out or "unknown error"))
    return _card("✓", f"Committed {len(files)} file(s)", f"{message}\n\n{out.strip()}")


@register("undo")
def _undo(args: str, ctx: CommandContext) -> CommandResult:
    from shadow_agent.checkpoints import restore_last

    result = restore_last(ctx.workspace)
    if not result.get("ok"):
        return _card("◆", "Nothing to undo", result.get("error") or "No checkpoint exists for this project yet.")
    restored = result.get("restored") or []
    return _card("✓", f"Undid {len(restored)} file(s)", "\n".join(restored) or "(no files)")


# --- subagents / mcp / memory / settings ------------------------------------


@register("agents")
def _agents(args: str, ctx: CommandContext) -> CommandResult:
    from shadow_agent.agents_dir import DEFAULT_READ_ONLY, DEFAULT_TOOL_ALLOWLIST, SubagentRole

    role_arg = args.strip().lower()
    items: list[dict[str, str]] = []
    for role in SubagentRole:
        tools = ", ".join(DEFAULT_TOOL_ALLOWLIST.get(role, [])) or "(all tools)"
        ro = "read-only" if DEFAULT_READ_ONLY.get(role, False) else "read-write"
        items.append({"label": role.value, "value": f"{ro} · tools: {tools}"})
    if role_arg:
        # Dispatch: stash a subagent task on ctx.extra so the UI can run it.
        ctx.extra["subagent_role"] = role_arg
        return _card("→", f"Dispatch to {role_arg}", "Run a task to spawn this subagent (the UI handles the dispatch).")
    return _list_card("Subagent hooks", items, icon="◆")


@register("mcp")
def _mcp(args: str, ctx: CommandContext) -> CommandResult:
    cfg = ctx.cfg()
    servers = cfg.mcp.servers or []
    if not servers:
        return _card("◆", "No MCP servers configured", "Add servers under `mcp.servers` in config.yaml.")
    items = []
    for s in servers:
        target = s.url or (" ".join(s.command or []))
        items.append({"label": s.name, "value": target or "(no command/url)"})
    return _list_card(f"MCP servers ({len(servers)})", items, icon="◆")


@register("memory")
def _memory(args: str, ctx: CommandContext) -> CommandResult:
    from shadow_agent.context.memory import MemoryStore

    # Task memory needs a task_id; for the slash command we use a session-scoped
    # project memory file under .shadow/memory/project.md.
    note = args.strip()
    mem = MemoryStore(ctx.workspace, task_id=ctx.session_id or "session")
    if note:
        mem.append_project(note)
        return _card("✓", "Project memory updated", note)
    project = mem.load_project()
    task = mem.load_task()
    body = ""
    if project:
        body += "## Project memory\n" + project + "\n"
    if task:
        body += "## Task memory\n" + task
    if not body:
        body = "(no project or task memory yet)"
    return _card("◆", "Memory", body)


@register("settings")
def _settings(args: str, ctx: CommandContext) -> CommandResult:
    parts = args.split(None, 1)
    if len(parts) == 2:
        key, value = parts[0], parts[1]
        cfg = set_config_value(key, value)
        return _card("✓", f"Set {key} = {value}", json.dumps(cfg.model_dump(mode="json"), indent=2)[:2000])
    if len(parts) == 1:
        return _card("✗", "Usage: /settings <key> <value>", f"Got a single token: {parts[0]!r}")
    # No args: open the settings overlay (desktop) or print config (TUI).
    if ctx.overlay_opener is not None:
        ctx.overlay_opener("settings")
        return _overlay("settings")
    cfg = ctx.cfg()
    return _card("◆", "Config", json.dumps(cfg.model_dump(mode="json"), indent=2))


# --- diagnostics: hyperpod-nccl --------------------------------------------


@register("hyperpod-nccl")
def _hyperpod_nccl(args: str, ctx: CommandContext) -> CommandResult:
    """Read-only HyperPod NCCL diagnostic flow.

    This machine is not a HyperPod cluster, so this command:
      1. Parses an optional `cluster region` arg (or prompts for them).
      2. Checks prerequisites: aws, kubectl, session-manager-plugin.
      3. If aws + kubectl are present, attempts the read-only
         `scripts/nccl-diagnose.sh` shipped with the sagemaker-ai skill,
         surfaces [FAIL] lines as cards. Never runs destructive commands.
      4. If prerequisites are missing, prints a prerequisites card and stops.

    Honest and safe: we never claim to have diagnosed a cluster we cannot
    reach, and we never run a command that mutates state.
    """
    # Parse "cluster region" or "cluster --region region".
    parts = args.split()
    cluster = ""
    region = os.environ.get("AWS_DEFAULT_REGION", "")
    i = 0
    while i < len(parts):
        p = parts[i]
        if p in ("--region", "-r") and i + 1 < len(parts):
            region = parts[i + 1]
            i += 2
        elif p in ("--cluster", "-c") and i + 1 < len(parts):
            cluster = parts[i + 1]
            i += 2
        elif not cluster:
            cluster = p
            i += 1
        elif not region:
            # Second positional arg is the region when --region was not used.
            region = p
            i += 1
        else:
            i += 1

    prereqs = {
        "aws": shutil.which("aws"),
        "kubectl": shutil.which("kubectl"),
        "session-manager-plugin": shutil.which("session-manager-plugin"),
        "jq": shutil.which("jq"),
    }
    missing = [name for name, path in prereqs.items() if not path]
    have_aws = bool(prereqs["aws"])
    have_kubectl = bool(prereqs["kubectl"])

    # Locate the diagnostic script bundled with the sagemaker-ai skill.
    script = _resolve_nccl_script()

    if not cluster or not region:
        body = (
            "This command runs the read-only HyperPod NCCL diagnostic.\n\n"
            "Usage:  /hyperpod-nccl <cluster> <region>\n"
            "        /hyperpod-nccl --cluster <name> --region <region>\n\n"
            "Prerequisites:\n"
            "  aws CLI v2.13+   " + _have(prereqs["aws"]) + "\n"
            "  kubectl          " + _have(prereqs["kubectl"]) + "\n"
            "  session-manager-plugin " + _have(prereqs["session-manager-plugin"]) + "\n"
            "  jq, python3, bash 4.2+\n\n"
            "The script never modifies cluster state. It prints each issue as\n"
            "[FAIL] ... → references/<file>.md § <section>.\n"
        )
        if missing:
            body += "\nMissing prerequisites: " + ", ".join(missing) + ". Install them before running the diagnostic."
        return _card("◆", "HyperPod NCCL diagnostic", body)

    if not have_aws or not have_kubectl or not script:
        body = (
            f"Would run: bash {script} --cluster {cluster} --region {region}\n\n"
            "Prerequisites (not all met on this machine):\n"
            f"  aws CLI            {_have(prereqs['aws'])}\n"
            f"  kubectl            {_have(prereqs['kubectl'])}\n"
            f"  session-manager-plugin {_have(prereqs['session-manager-plugin'])}\n"
            f"  jq                 {_have(prereqs['jq'])}\n"
        )
        if not script:
            body += "\nThe nccl-diagnose.sh script is not installed locally. Install the sagemaker-ai skill, or run the script on a host that has it."
        if missing:
            body += "\nMissing: " + ", ".join(missing) + "."
        body += "\n\nNo destructive commands were run. The diagnostic is read-only."
        return _card("◆", "HyperPod NCCL — prerequisites", body)

    # Attempt the read-only diagnostic.
    try:
        proc = subprocess.run(
            ["bash", str(script), "--cluster", cluster, "--region", region],
            cwd=str(ctx.workspace),
            capture_output=True,
            text=True,
            timeout=120,
            check=False,
        )
    except Exception as exc:  # noqa: BLE001 - surface a clean card
        return _card("✗", "Diagnostic failed to launch", str(exc)[:1000])

    out = proc.stdout or ""
    err = proc.stderr or ""
    fails = [line for line in (out + "\n" + err).splitlines() if line.startswith("[FAIL]")]
    body = out
    if err:
        body += "\n--- stderr ---\n" + err
    if len(body) > 8000:
        body = body[:8000] + "\n… (truncated)"
    icon = "✓" if not fails and proc.returncode == 0 else ("✗" if fails else "◆")
    headline = f"HyperPod NCCL · {cluster} · {region}"
    if fails:
        headline += f" · {len(fails)} [FAIL]"
    return CommandResult(
        handled=True,
        kind="card",
        icon=icon,
        headline=headline,
        body=body or "(no output)",
        metadata={"exit_code": proc.returncode, "fails": fails, "cluster": cluster, "region": region},
    )


def _have(path: str | None) -> str:
    return f"✓ {path}" if path else "✗ not found"


def _resolve_nccl_script() -> Path | None:
    """Find the nccl-diagnose.sh bundled with the sagemaker-ai skill."""
    candidates = [
        Path.home() / ".cursor/plugins/cache/cursor-public/sagemaker-ai",
        Path("/usr/share/sagemaker-ai"),
    ]
    for root in candidates:
        if not root.is_dir():
            continue
        for path in root.rglob("scripts/nccl-diagnose.sh"):
            if path.is_file():
                return path
    return None


# --- legacy / session UX (back-compat with the prior TUI) --------------------


@register("clear")
def _clear(args: str, ctx: CommandContext) -> CommandResult:
    if ctx.clearer is not None:
        ctx.clearer()
    return _text("Transcript cleared.")


@register("new")
def _new(args: str, ctx: CommandContext) -> CommandResult:
    if ctx.new_session is not None:
        sid = ctx.new_session()
        return _card("✓", "New session", sid)
    return _card("◆", "New session", "Create a session via the UI.")


@register("branch")
def _branch(args: str, ctx: CommandContext) -> CommandResult:
    if ctx.branch_session is not None:
        sid = ctx.branch_session()
        return _card("✓", "Branched session", sid)
    return _card("◆", "Branch session", "Branch the current session via the UI.")


@register("sessions")
def _sessions(args: str, ctx: CommandContext) -> CommandResult:
    if ctx.overlay_opener is not None:
        ctx.overlay_opener("sessions")
        return _overlay("sessions")
    if ctx.store is not None and hasattr(ctx.store, "list_sessions"):
        rows = ctx.store.list_sessions(limit=30)
        items = [
            {"label": r["id"][:8], "value": f"{r.get('status', '')}  {r.get('title') or '(untitled)'}"}
            for r in rows
        ]
        return _list_card(f"Sessions ({len(rows)})", items, icon="◆")
    return _card("◆", "Sessions", "Open the sessions panel in the UI.")


@register("resume")
def _resume(args: str, ctx: CommandContext) -> CommandResult:
    target = args.strip()
    if not target:
        return _card("✗", "Usage: /resume <id-prefix>", "Resume a session by id prefix.")
    if ctx.resume_session is not None:
        sid = ctx.resume_session(target)
        if sid:
            return _card("✓", f"Resumed {sid[:8]}", sid)
        return _card("✗", f"No session matching {target!r}", "")
    return _card("◆", f"Resume {target}", "Use the UI sessions panel.")


@register("pin")
def _pin(args: str, ctx: CommandContext) -> CommandResult:
    if ctx.pin_last is not None:
        ctx.pin_last("pin")
        return _card("✓", "Pinned last agent message", "")
    return _card("◆", "Pin", "Pinning is wired to the running transcript (TUI).")


@register("cost")
def _cost(args: str, ctx: CommandContext) -> CommandResult:
    if not ctx.session_id or ctx.store is None or not hasattr(ctx.store, "session_cost"):
        return _card("◆", "Cost", "Open a session in the UI to see token usage.")
    cost = ctx.store.session_cost(ctx.session_id)
    usage = cost.get("usage") or {}
    items = [
        {"label": "total tokens", "value": str(usage.get("total_tokens", 0))},
        {"label": "prompt tokens", "value": str(usage.get("prompt_tokens", 0))},
        {"label": "completion tokens", "value": str(usage.get("completion_tokens", 0))},
    ]
    for t in cost.get("tasks", [])[:8]:
        u = t.get("usage") or {}
        items.append({"label": t["task_id"][:8], "value": f"{u.get('total_tokens', 0)} tok  ·  {t.get('prompt', '')[:60]}"})
    return _list_card("Session cost", items, icon="◆")


@register("doctor")
def _doctor(args: str, ctx: CommandContext) -> CommandResult:
    from shadow_agent.health import doctor_fix, doctor_report

    cfg = ctx.cfg()
    report = doctor_report(cfg, ctx.workspace)
    doctor_fix(report, ctx.workspace)
    report = doctor_report(cfg, ctx.workspace)
    items = [
        {"label": "✓" if c["ok"] else "✗", "value": c["label"] + (f"  — fix: {c['fix']}" if not c["ok"] and c.get("fix") else "")}
        for c in report["checks"]
    ]
    return _list_card(f"Doctor · {sum(1 for c in report['checks'] if c['ok'])}/{len(report['checks'])} ok", items, icon="◆")


@register("health")
def _health(args: str, ctx: CommandContext) -> CommandResult:
    from shadow_agent.health import collect_health

    payload = collect_health(ctx.cfg(), ctx.workspace)
    items = [
        {"label": "version", "value": payload.get("version", "")},
        {"label": "workspace", "value": payload.get("workspace", "")},
        {"label": "provider", "value": _ok_label(payload.get("provider", {}))},
    ]
    for name, info in (payload.get("tools") or {}).items():
        items.append({"label": name, "value": _ok_label(info)})
    return _list_card("Health", items, icon="◆")


@register("ui")
def _ui(args: str, ctx: CommandContext) -> CommandResult:
    if ctx.ui_launcher is not None:
        ctx.ui_launcher()
        return _card("✓", "Launching desktop UI", "")
    return _card("◆", "Desktop UI", "Run `shadow ui` to start the desktop UI.")


@register("quit")
def _quit(args: str, ctx: CommandContext) -> CommandResult:
    if ctx.quitter is not None:
        ctx.quitter()
    return CommandResult(handled=True, kind="quit", quit=True)


@register("exit")
def _exit(args: str, ctx: CommandContext) -> CommandResult:
    return _quit(args, ctx)


# --- expand (in-flight TUI feature: collapse/expand the last tool card) -----


@register("expand")
def _expand(args: str, ctx: CommandContext) -> CommandResult:
    """Toggle the last tool card's expanded state.

    The actual toggle lives on the TranscriptModel; the handler exposes it
    via `ctx.extra['expand_last_card']` so the TUI can call it. The desktop
    UI ignores this (cards are already expandable by click).
    """
    cb = (ctx.extra or {}).get("expand_last_card")
    if callable(cb):
        cb()
        return _text("Toggled last tool card.")
    return _card("◆", "Expand", "No tool cards to expand yet.")


# --- ShadowCode 0.8.0 flagship 15-pillar slash-command handlers ---------------
# These wire the flagship CLI features (understand, goal, why, vision, team,
# router, docs, tools, profile, rollback, checkpoints) into the TUI/desktop
# slash surface. Each returns a Codex-style card so the UI renders them
# consistently with the rest of the command set.


@register("understand")
def _understand(args: str, ctx: CommandContext) -> CommandResult:
    from shadow_agent.understand import save_project_map, render_map
    mp = save_project_map(ctx.workspace, task_id="understand")
    return _card("◆", "Project map saved", render_map(mp) + f"\n\nSaved to {ctx.workspace}/.shadow/memory/project.md")


@register("goal")
def _goal(args: str, ctx: CommandContext) -> CommandResult:
    if not args.strip():
        return _card("◆", "Goal Mode", "Usage: /goal <one-line instruction>\nCreates a goal with milestones + progress. Run `shadow goal <task> --run` to execute.")
    from shadow_agent.goal import GoalStore, plan_milestones, render_goal
    store = GoalStore()
    milestones = plan_milestones(args)
    g = store.create_goal(ctx.workspace, args, milestones)
    return _card("◆", f"Goal {g['id'][:8]} created", render_goal(g))


@register("goals")
def _goals(args: str, ctx: CommandContext) -> CommandResult:
    from shadow_agent.goal import GoalStore, render_goal
    store = GoalStore()
    rows = store.list_goals(ctx.workspace)
    if not rows:
        return _card("◆", "Goals", "No goals yet. Create one with /goal <task>.")
    body = "\n\n".join(render_goal(g) for g in rows)
    return _card("◆", f"{len(rows)} goal(s)", body)


@register("team")
def _team(args: str, ctx: CommandContext) -> CommandResult:
    if not args.strip():
        return _card("◆", "Agent Teams", "Usage: /team <task>\nLEAD dispatches to ARCHITECT/CODER/TESTER/SECURITY/REVIEWER and combines.")
    from shadow_agent.teams import AgentTeam
    team = AgentTeam(ctx.workspace, config=ctx.cfg())
    report = team.run(args)
    return _card("◆" if report.success else "✗", "Team report", report.combined)


@register("router")
def _router(args: str, ctx: CommandContext) -> CommandResult:
    if args.strip():
        from shadow_agent.config import save_config
        from shadow_agent.models.routing import apply_override
        new_cfg, message = apply_override(ctx.cfg(), args.strip())
        save_config(new_cfg)
        return _card("◆", "Router override", message)
    router = ModelRouter(ctx.cfg())
    table = router.table_view()
    items = [{"label": purpose, "value": mid} for purpose, mid in table.items()]
    return _list_card("Model router", items, icon="◆")


@register("why")
def _why(args: str, ctx: CommandContext) -> CommandResult:
    from shadow_agent.why import explain_change, render_explanation
    expl = explain_change(ctx.workspace, args.strip() or None, session_id=ctx.session_id or None)
    return _card("◆" if expl.get("ok") else "✗", "/why", render_explanation(expl))


@register("vision")
def _vision(args: str, ctx: CommandContext) -> CommandResult:
    if not args.strip():
        return _card("◆", "Visual debugging", "Usage: /vision <image-path>\nSends a screenshot to a vision model and returns detected issues + suggested files.")
    from shadow_agent.vision import analyze_screenshot, render_analysis
    result = analyze_screenshot(ctx.workspace, args.strip(), ctx.cfg())
    return _card("◆" if result.get("ok") else "✗", "Vision analysis", render_analysis(result))


@register("docs")
def _docs(args: str, ctx: CommandContext) -> CommandResult:
    parts = args.strip().split(None, 1)
    task = parts[0] if parts else ""
    url = parts[1] if len(parts) > 1 else ""
    if not task:
        return _card("◆", "Documentation research", "Usage: /docs <task> [url]\nSEARCH → READ → COMPARE → PLAN → IMPLEMENT → TEST.")
    from shadow_agent.docs_research import plan_doc_research, render_plan
    plan = plan_doc_research(task, url)
    return _card("◆", "Doc research plan", render_plan(plan))


@register("tools")
def _tools(args: str, ctx: CommandContext) -> CommandResult:
    from shadow_agent.tools.marketplace import marketplace_view, render_marketplace
    from shadow_agent.tools.registry import default_tools
    from shadow_agent.tools.sandbox import WorkspaceSandbox
    from shadow_agent.permissions import PermissionGate
    from shadow_agent.config import PermissionLevel
    sandbox = WorkspaceSandbox(ctx.workspace)
    gate = PermissionGate(PermissionLevel.WORKSPACE)
    reg = default_tools(sandbox, gate, 60)
    view = marketplace_view(reg)
    return _card("◆", "Tool marketplace", render_marketplace(view))


@register("profile")
def _profile(args: str, ctx: CommandContext) -> CommandResult:
    from shadow_agent.profiles import apply_profile, current_profile, render_profiles
    from shadow_agent.config import save_config
    if args.strip():
        try:
            cfg, prof = apply_profile(ctx.cfg(), args.strip())
        except ValueError as exc:
            return _card("✗", "Profile", str(exc))
        save_config(cfg)
        return _card("◆", f"Profile → {prof.name}", prof.description)
    cur = current_profile(ctx.cfg())
    return _card("◆", "Permission profiles", render_profiles(cur))


@register("rollback")
def _rollback(args: str, ctx: CommandContext) -> CommandResult:
    if not args.strip():
        return _card("◆", "Rollback", "Usage: /rollback <checkpoint-name>\nUse /checkpoints to list them.")
    from shadow_agent.checkpoints import rollback_named
    result = rollback_named(ctx.workspace, args.strip())
    if not result.get("ok"):
        return _card("✗", "Rollback failed", result.get("error", "unknown error"))
    body = "\n".join(f"  • {p}" for p in result.get("restored", [])[:12])
    return _card("✓", f"Restored {result['files']} file(s)", body)


@register("checkpoints")
def _checkpoints(args: str, ctx: CommandContext) -> CommandResult:
    from shadow_agent.checkpoints import list_named_checkpoints
    rows = list_named_checkpoints(ctx.workspace)
    if not rows:
        return _card("◆", "Checkpoints", "No named checkpoints yet.")
    items = [{"label": r["name"], "value": f"{r['files']} file(s)  git={r.get('git_head', '')[:8]}"} for r in rows]
    return _list_card("Named checkpoints", items, icon="◆")
