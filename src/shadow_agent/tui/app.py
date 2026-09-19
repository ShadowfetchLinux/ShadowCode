"""Codex-style terminal UI built on prompt_toolkit + rich.

Layout (top → bottom):
  • header: brand + workspace + model + permission level
  • transcript: scrollback of turns with collapsible tool cards
  • diff card: when the agent proposes an edit, a unified diff card with
    Accept / Reject (y/n) appears inline
  • approval card: dangerous commands surface inline with a risk label
  • status line: model · context % · tokens · step · working indicator
  • composer: centered single-line → multiline input, slash commands,
    Ctrl+Enter to send, Shift+Enter for a newline, arrow history

Keybindings:
  Ctrl+Enter / Ctrl+J  send
  Shift+Enter        newline
  Esc                cancel running task / close overlay
  Ctrl+R             rerun last task
  Ctrl+P             project picker
  ↑ / ↓              history navigation (when composer focused)
  ?                  help overlay
  /                  slash command (handled by the composer)
"""

from __future__ import annotations

import json
import threading
import time
from pathlib import Path
from typing import Any

from prompt_toolkit import Application
from prompt_toolkit.buffer import Buffer
from prompt_toolkit.key_binding import KeyBindings
from prompt_toolkit.layout import (
    ConditionalContainer,
    Float,
    FloatContainer,
    HSplit,
    Layout,
    VSplit,
    Window,
    WindowAlign,
    FormattedTextControl,
)
from prompt_toolkit.styles import Style
from prompt_toolkit.filters import Condition

from shadow_agent import __version__
from shadow_agent.agent.loop import AgentRunner
from shadow_agent.approvals import ApprovalHub
from shadow_agent.commands import CommandRegistry
from shadow_agent.config import ensure_user_config, load_config, remember_workspace
from shadow_agent.events import EventBus
from shadow_agent.models.registry import ModelRegistry
from shadow_agent.secrets import load_secrets
from shadow_agent.store import Store
from shadow_agent.tui.theme import load_theme, system_theme
from shadow_agent.tui.transcript import TranscriptModel


def run_tui(workspace: Path, resume_session: str | None = None) -> None:
    load_secrets()
    workspace = Path(workspace).resolve()
    remember_workspace(workspace)
    cfg = load_config(workspace)
    if not cfg.onboarding.completed:
        cfg = ensure_user_config()
    theme_name = cfg.ui.theme if cfg.ui.theme in {"dark", "light"} else system_theme()
    theme = load_theme(theme_name)

    store = Store()
    bus = EventBus()
    approvals = ApprovalHub(timeout_sec=300.0)
    registry = ModelRegistry(detect=True)
    commands = CommandRegistry(workspace)

    # Pick or resume a session.
    if resume_session:
        sid = _resolve_session(store, resume_session) or store.create_session(str(workspace), cfg.model.default, title="TUI")
    else:
        rows = store.list_sessions(limit=1)
        sid = rows[0]["id"] if rows else store.create_session(str(workspace), cfg.model.default, title="TUI")

    model = TranscriptModel(workspace, theme, cfg, store, sid)
    model.set_status(model=f"{cfg.model.provider}/{cfg.model.name or cfg.model.default}", context_pct=0, tokens=0, step=0, busy=False)

    state = {
        "workspace": workspace,
        "cfg": cfg,
        "store": store,
        "bus": bus,
        "approvals": approvals,
        "registry": registry,
        "commands": commands,
        "session_id": sid,
        "busy": False,
        "runner": None,
        "history": [],
        "history_idx": 0,
        "overlay": None,  # None | "help" | "models" | "sessions"
        "quit": False,
        "app": None,  # filled in after Application is built
    }

    def _emit(event_type: str, payload: dict, task_id: str | None = None) -> None:
        store.add_event(event_type, payload, session_id=sid, task_id=task_id)
        model.ingest(event_type, payload, task_id)

    bus.subscribe(None, lambda et, ev: _emit(ev["type"], ev["payload"], ev.get("task_id")))

    def _run_task(task: str) -> None:
        if state["busy"] or not task.strip():
            return
        state["busy"] = True
        model.set_status(busy=True)
        model.add_user(task)

        def worker() -> None:
            try:
                runner = AgentRunner(
                    workspace,
                    config=load_config(workspace),
                    store=store,
                    events=bus,
                    session_id=sid,
                    approval_hub=approvals,
                )
                state["runner"] = runner
                result = runner.run(task)
                model.add_result(result.summary, result.success, result.usage, result.steps)
            except Exception as exc:  # noqa: BLE001 - surface to the transcript
                model.add_agent(f"Error: {exc}")
            finally:
                state["busy"] = False
                state["runner"] = None
                model.set_status(busy=False)

        threading.Thread(target=worker, daemon=True, name="shadow-tui-task").start()

    def _handle_command(line: str) -> bool:
        """Return True if the line was a slash command and is fully handled."""
        cmd, name, args = commands.parse(line)
        if cmd is None:
            return False
        builtin = commands.is_builtin(name)
        if not builtin:
            # Custom command: expand to a prompt and run as a task.
            _run_task(cmd.render(args))
            return True
        # Built-in dispatch.
        if name in {"quit", "exit"}:
            if state.get("app") is not None:
                state["app"].exit()
            return True
        if name == "help":
            state["overlay"] = "help"
            return True
        if name == "clear":
            model.clear()
            return True
        if name == "new":
            sid2 = store.create_session(str(workspace), cfg.model.default, title="TUI")
            state["session_id"] = sid2
            model.reset(sid2)
            return True
        if name in {"model", "models"}:
            state["overlay"] = "models"
            return True
        if name == "config":
            model.add_agent(_config_text(load_config(workspace)))
            return True
        if name == "compact":
            model.add_agent("Compacting transcript…")
            model.compact()
            return True
        if name == "undo":
            from shadow_agent.checkpoints import restore_last

            res = restore_last(workspace)
            model.add_agent(f"Undo: {res.get('restored') or res.get('error')}")
            return True
        if name == "diff":
            model.add_agent(_git_text(workspace, "diff"))
            return True
        if name == "git":
            model.add_agent(_git_text(workspace, "status"))
            return True
        if name == "branch":
            try:
                new_id = store.branch_session(state["session_id"], "branch")
                state["session_id"] = new_id
                model.reset(new_id)
                model.add_agent(f"Branched session → {new_id[:8]}")
            except KeyError:
                model.add_agent("No session to branch.")
            return True
        if name == "sessions":
            state["overlay"] = "sessions"
            return True
        if name == "resume":
            target = _resolve_session(store, args or "")
            if target:
                state["session_id"] = target
                model.reset(target)
                model.add_agent(f"Resumed session {target[:8]}")
            else:
                model.add_agent(f"No session matching “{args}”.")
            return True
        if name == "pin":
            last = model.last_agent_text()
            if last:
                store.add_pin(state["session_id"], "pin", last)
                model.add_agent("Pinned the last agent message.")
            else:
                model.add_agent("Nothing to pin yet.")
            return True
        if name == "cost":
            cost = store.session_cost(state["session_id"])
            model.add_agent(_cost_text(cost))
            return True
        if name == "doctor":
            from shadow_agent.health import doctor_report, doctor_fix

            report = doctor_report(load_config(workspace), workspace)
            doctor_fix(report, workspace)
            report = doctor_report(load_config(workspace), workspace)
            model.add_agent(_doctor_text(report))
            return True
        if name == "health":
            from shadow_agent.health import collect_health

            payload = collect_health(load_config(workspace), workspace)
            model.add_agent(json.dumps(payload, indent=2))
            return True
        if name == "ui":
            from shadow_agent.desktop import launch_desktop

            model.add_agent("Launching desktop UI in the background…")
            threading.Thread(
                target=launch_desktop,
                args=(workspace, cfg.ui.host, cfg.ui.port),
                daemon=True,
            ).start()
            return True
        model.add_agent(f"Unknown command: /{name}")
        return True

    # --- prompt_toolkit wiring ------------------------------------------------

    composer = Buffer(multiline=True, name="composer")

    def _send() -> None:
        text = composer.text
        if not text.strip():
            return
        state["history"].append(text)
        state["history_idx"] = len(state["history"])
        composer.text = ""
        if text.startswith("/"):
            handled = _handle_command(text)
            if handled:
                return
        _run_task(text)

    kb = KeyBindings()

    # Codex-style composer: Enter sends, Ctrl+J inserts a newline (linefeed).
    # (Shift+Enter is not a distinct key in most terminals; Ctrl+J is the
    # standard "insert linefeed" binding and doubles as the multiline key.)
    @kb.add("enter", filter=~Condition(lambda: state["overlay"] is not None))
    def _(event):
        _send()

    @kb.add("c-j")
    def _(event):
        event.current_buffer.insert_text("\n")

    @kb.add("c-c")
    def _(event):
        if state["busy"] and state["runner"] is not None:
            state["runner"].cancel()
            model.add_agent("Stopping…")
        else:
            state["overlay"] = None

    @kb.add("escape")
    def _(event):
        if state["overlay"]:
            state["overlay"] = None
        elif state["busy"] and state["runner"] is not None:
            state["runner"].cancel()
            model.add_agent("Stopping…")

    @kb.add("c-r")
    def _(event):
        last = state["history"][-1] if state["history"] else ""
        if last:
            _run_task(last)

    @kb.add("c-p")
    def _(event):
        state["overlay"] = "sessions"

    @kb.add("f2")
    def _(event):
        state["overlay"] = "models"

    @kb.add("up")
    def _(event):
        if state["history"] and state["history_idx"] > 0:
            state["history_idx"] -= 1
            composer.text = state["history"][state["history_idx"]]
            composer.cursor_position = len(composer.text)

    @kb.add("down")
    def _(event):
        if state["history_idx"] < len(state["history"]) - 1:
            state["history_idx"] += 1
            composer.text = state["history"][state["history_idx"]]
        else:
            state["history_idx"] = len(state["history"])
            composer.text = ""
        composer.cursor_position = len(composer.text)

    @kb.add("?")
    def _(event):
        state["overlay"] = "help"

    @kb.add("c-d")
    def _(event):
        event.app.exit()

    # Header
    header_control = FormattedTextControl(text=lambda: model.header_text())
    header = Window(content=header_control, height=1, style=f"bg:{theme.panel} fg:{theme.accent_text}")

    # Transcript (scrollable)
    transcript_control = FormattedTextControl(
        text=lambda: model.render(),
        focusable=True,
    )
    transcript_window = Window(content=transcript_control, wrap_lines=True, style=f"fg:{theme.text}")

    # Status line
    status_control = FormattedTextControl(text=lambda: model.status_text())
    status_line = Window(content=status_control, height=1, style=f"bg:{theme.panel} fg:{theme.muted}")

    # Composer
    from prompt_toolkit.layout.controls import BufferControl
    from prompt_toolkit.layout.containers import Float

    composer_window = Window(
        content=BufferControl(buffer=composer, focusable=True),
        height=3,
        style=f"bg:{theme.user_bg} fg:{theme.text}",
        wrap_lines=True,
    )
    composer_label = Window(
        content=FormattedTextControl(text=[("class:prompt", " › ")]),
        width=3,
        style=f"bg:{theme.user_bg} fg:{theme.accent}",
        align=WindowAlign.LEFT,
    )
    composer_row = VSplit([composer_label, composer_window])

    # Overlays (help / models / sessions)
    overlay_body = FormattedTextControl(text=lambda: model.overlay_text(state["overlay"]))
    overlay = ConditionalContainer(
        Window(content=overlay_body, wrap_lines=True, style=f"bg:{theme.panel} fg:{theme.text}", height=20),
        filter=Condition(lambda: state["overlay"] is not None),
    )

    root = FloatContainer(
        HSplit(
            [
                header,
                transcript_window,
                status_line,
                composer_row,
                overlay,
            ],
            style=f"bg:{theme.bg}",
        ),
        floats=[],
    )

    style = Style.from_dict({
        "prompt": f"{theme.accent} bold",
        "status": f"{theme.muted}",
        "accent": f"{theme.accent} bold",
        "ok": f"{theme.ok}",
        "warn": f"{theme.warn}",
        "danger": f"{theme.danger}",
        "add": f"{theme.add}",
        "del": f"{theme.del_}",
        "muted": f"{theme.muted}",
    })

    app = Application(
        layout=Layout(root, focused_element=composer_window),
        key_bindings=kb,
        style=style,
        full_screen=True,
        mouse_support=False,
    )
    state["app"] = app

    try:
        app.run()
    finally:
        if state["busy"] and state["runner"] is not None:
            state["runner"].cancel()


def _resolve_session(store: Store, prefix: str) -> str | None:
    if not prefix:
        return None
    rows = store.list_sessions(limit=50)
    for row in rows:
        if row["id"].startswith(prefix) or row["id"] == prefix:
            return row["id"]
    return None


def _config_text(cfg: Any) -> str:
    return json.dumps(cfg.model_dump(mode="json"), indent=2)


def _git_text(workspace: Path, kind: str) -> str:
    import subprocess

    if kind == "diff":
        proc = subprocess.run(["git", "diff"], cwd=workspace, capture_output=True, text=True, check=False)
        return proc.stdout or "(no changes)"
    proc = subprocess.run(["git", "status", "-sb"], cwd=workspace, capture_output=True, text=True, check=False)
    return proc.stdout or "(not a git repo)"


def _cost_text(cost: dict[str, Any]) -> str:
    usage = cost.get("usage") or {}
    lines = [f"Session tokens: {usage.get('total_tokens', 0)} (prompt {usage.get('prompt_tokens', 0)} / completion {usage.get('completion_tokens', 0)})"]
    for task in cost.get("tasks", []):
        u = task.get("usage") or {}
        lines.append(f"  • {task['task_id'][:8]} [{task['status']}] {u.get('total_tokens', 0)} tok — {task['prompt'][:60]}")
    return "\n".join(lines)


def _doctor_text(report: dict[str, Any]) -> str:
    lines = [f"shadow doctor v{report['version']}"]
    for check in report["checks"]:
        mark = "✓" if check["ok"] else "✗"
        lines.append(f"{mark} {check['label']}")
        if not check["ok"] and check.get("fix"):
            lines.append(f"    fix: {check['fix']}")
    return "\n".join(lines)
