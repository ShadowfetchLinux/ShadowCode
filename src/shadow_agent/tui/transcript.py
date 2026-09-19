"""Transcript model for the Codex-style TUI.

Renders the scrollback of turns (user / agent / tool / diff / approval) into a
single FormattedTextControl string. Tool calls are collapsible; the agent's
proposed edits surface as Codex-style diff cards with Accept/Reject hints.
"""

from __future__ import annotations

import json
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from shadow_agent.config import AppConfig


@dataclass
class Turn:
    kind: str  # user | agent | tool | diff | approval | result
    text: str = ""
    tool: str = ""
    ok: bool | None = None
    live: bool = False
    collapsed: bool = False
    ts: float = field(default_factory=time.time)
    # Codex-style compact operation card fields (tool turns only).
    icon: str = ""
    headline: str = ""
    full_output: str = ""


class TranscriptModel:
    def __init__(
        self,
        workspace: Path,
        theme,
        cfg: AppConfig,
        store,
        session_id: str,
    ) -> None:
        self.workspace = workspace
        self.theme = theme
        self.cfg = cfg
        self.store = store
        self.session_id = session_id
        self.turns: list[Turn] = []
        self.status = {"model": "", "context_pct": 0, "tokens": 0, "step": 0, "busy": False}
        self.stage = "IDLE"
        self.fix_retries = 0
        self.max_fix_retries = 3
        self._usage_total = 0
        self._step = 0

    # --- status / header ---------------------------------------------------

    def set_status(self, *, model: str | None = None, context_pct: int | None = None, tokens: int | None = None, step: int | None = None, busy: bool | None) -> None:
        if model is not None:
            self.status["model"] = model
        if context_pct is not None:
            self.status["context_pct"] = context_pct
        if tokens is not None:
            self.status["tokens"] = tokens
        if step is not None:
            self.status["step"] = step
        if busy is not None:
            self.status["busy"] = busy

    def header_text(self) -> str:
        t = self.theme
        ws = self.workspace.name
        perm = self.cfg.permissions.level.value
        model = self.status["model"] or f"{self.cfg.model.provider}/{self.cfg.model.name or self.cfg.model.default}"
        return f"  Shadow Agent {__version__()}  ·  {ws}  ·  {model}  ·  {perm}"

    def status_text(self) -> str:
        t = self.theme
        busy = " ● working" if self.status["busy"] else " ○ idle"
        ctx = self.status["context_pct"]
        tok = self.status["tokens"] or self._usage_total
        step = self.status["step"]
        chip = self._stage_chip()
        return f"  {self.status['model']}  ·  {chip}{busy}  ·  ctx {ctx}%  ·  {tok} tok  ·  step {step}  ·  Enter send · Ctrl+J newline · ? help"

    def _stage_chip(self) -> str:
        """Codex-style stage chip for the status line."""
        if self.stage == "FIX":
            return f"[ FIX · retry {self.fix_retries}/{self.max_fix_retries} ]"
        if self.stage and self.stage != "IDLE":
            return f"[ {self.stage} ]"
        return "[ IDLE ]"

    def set_stage(self, stage: str, fix_retries: int = 0, max_fix_retries: int = 3) -> None:
        self.stage = stage or "IDLE"
        if fix_retries:
            self.fix_retries = int(fix_retries)
        if max_fix_retries:
            self.max_fix_retries = int(max_fix_retries)

    # --- turns ------------------------------------------------------------

    def add_user(self, text: str) -> None:
        self.turns.append(Turn(kind="user", text=text))

    def add_agent(self, text: str) -> None:
        if not text:
            return
        self.turns.append(Turn(kind="agent", text=text))

    def add_result(self, summary: str, success: bool, usage: dict[str, int], steps: int) -> None:
        self._usage_total = int(usage.get("total_tokens", 0)) or self._usage_total
        self.status["step"] = steps
        self.turns.append(Turn(kind="result", text=summary, ok=success))

    def add_tool(
        self,
        tool: str,
        ok: bool | None,
        text: str,
        live: bool = False,
        icon: str = "",
        headline: str = "",
        full_output: str = "",
    ) -> None:
        # Codex-style compact card: collapsed by default with a one-line headline.
        # `text` is the raw preview (kept for backward-compat / fallback).
        turn = Turn(
            kind="tool",
            tool=tool,
            ok=ok,
            text=text,
            live=live,
            collapsed=True,
            icon=icon,
            headline=headline or text[:120],
            full_output=full_output or text,
        )
        self.turns.append(turn)

    def toggle_card(self, idx: int) -> None:
        """Expand/collapse the tool card at transcript turn index `idx`."""
        if 0 <= idx < len(self.turns) and self.turns[idx].kind == "tool":
            self.turns[idx].collapsed = not self.turns[idx].collapsed

    def tool_turn_indices(self) -> list[int]:
        return [i for i, t in enumerate(self.turns) if t.kind == "tool"]

    def add_diff_card(self, path: str, diff: str) -> None:
        self.turns.append(Turn(kind="diff", tool=path, text=diff))

    def add_approval(self, command: str, reason: str) -> None:
        self.turns.append(Turn(kind="approval", tool=command, text=reason))

    def clear(self) -> None:
        self.turns.clear()

    def reset(self, session_id: str) -> None:
        self.session_id = session_id
        self.turns.clear()
        self._usage_total = 0
        self._step = 0

    def compact(self) -> None:
        # Summarize older turns into a single agent note (Codex /compact).
        if len(self.turns) <= 4:
            return
        old = self.turns[:-4]
        summary_bits = []
        for turn in old:
            if turn.kind == "user":
                summary_bits.append(f"user: {turn.text[:80]}")
            elif turn.kind == "agent":
                summary_bits.append(f"agent: {turn.text[:80]}")
            elif turn.kind == "tool":
                summary_bits.append(f"tool {turn.tool}: {'ok' if turn.ok else 'fail'}")
        self.turns = [Turn(kind="agent", text="Compacted earlier turns:\n" + "\n".join(summary_bits))] + self.turns[-4:]

    def last_agent_text(self) -> str | None:
        for turn in reversed(self.turns):
            if turn.kind == "agent" and turn.text:
                return turn.text
        return None

    # --- event ingestion ---------------------------------------------------

    def ingest(self, event_type: str, payload: dict, task_id: str | None) -> None:
        if event_type == "agent.started":
            self._step = 0
            self.status["step"] = 0
            self.stage = "IDLE"
            self.fix_retries = 0
        elif event_type in (
            "agent.understand",
            "agent.plan",
            "agent.inspect",
            "agent.act",
            "agent.observe",
            "agent.verify",
            "agent.fix",
            "agent.done",
            "agent.failed",
        ):
            stage = str(payload.get("stage") or event_type.split(".", 1)[1].upper())
            self.set_stage(
                stage,
                fix_retries=int(payload.get("fix_retries") or payload.get("retry") or self.fix_retries),
                max_fix_retries=int(payload.get("max_fix_retries") or self.max_fix_retries),
            )
        elif event_type == "model.delta" and payload.get("text"):
            # token-by-token streaming: append to the last agent turn
            text = str(payload["text"])
            if self.turns and self.turns[-1].kind == "agent":
                self.turns[-1].text += text
            else:
                self.add_agent(text)
        elif event_type == "model.response":
            self._step = int(payload.get("step") or self._step)
            self.status["step"] = self._step
            text = str(payload.get("text") or "")
            if text and (not self.turns or self.turns[-1].kind != "agent" or self.turns[-1].text != text):
                if not (self.turns and self.turns[-1].kind == "agent"):
                    self.add_agent(text)
        elif event_type == "tool.started":
            self.add_tool(
                str(payload.get("tool") or "tool"),
                None,
                json.dumps(payload.get("arguments") or {}),
                live=True,
                icon="●",
                headline=f"{payload.get('tool') or 'tool'} · running",
                full_output="",
            )
        elif event_type == "tool.completed":
            ok = bool(payload.get("success"))
            tool = str(payload.get("tool") or "tool")
            text = str(payload.get("output_preview") or payload.get("error") or "")
            # Prefer the precomputed Codex-style card fields from the agent loop.
            from shadow_agent.op_card import headline_for_event

            icon, headline, full = headline_for_event(payload)
            self.add_tool(tool, ok, text, live=False, icon=icon, headline=headline, full_output=full or text)
        elif event_type == "tool.parallel":
            self.add_agent(f"∥ {payload.get('count')} read tools in parallel")
        elif event_type == "approval.requested":
            self.add_approval(str(payload.get("command") or payload.get("tool") or ""), str(payload.get("reason") or ""))
        elif event_type == "agent.completed":
            usage = payload.get("usage") or {}
            self._usage_total = int(usage.get("total_tokens", 0)) or self._usage_total
            self.status["busy"] = False
            self.status["step"] = int(payload.get("steps") or self.status["step"])
            self.stage = payload.get("stage") or ("DONE" if payload.get("success") else "IDLE")
            self.fix_retries = int(payload.get("fix_retries") or self.fix_retries)
        elif event_type == "model.retry":
            self.add_agent(f"retry {payload.get('attempt')}/{payload.get('max_attempts')} after {payload.get('wait_sec')}s")

    # --- rendering ---------------------------------------------------------

    def render(self) -> list[tuple[str, str]]:
        """Return prompt_toolkit formatted text (list of (style, text) tuples)."""
        t = self.theme
        out: list[tuple[str, str]] = []
        for i, turn in enumerate(self.turns):
            if turn.kind == "user":
                out.append(("class:accent", "  you › "))
                out.append(("", turn.text + "\n\n"))
            elif turn.kind == "agent":
                out.append(("class:muted", "  agent › "))
                out.append(("", turn.text + "\n\n"))
            elif turn.kind == "tool":
                # Codex-style compact card: one summary line + icon, collapsed by default.
                if turn.live:
                    mark = "…"
                    style = "class:muted"
                elif turn.ok is True:
                    mark = turn.icon or "✓"
                    style = "class:ok"
                elif turn.ok is False:
                    mark = turn.icon or "✗"
                    style = "class:danger"
                else:
                    mark = turn.icon or "●"
                    style = "class:muted"
                head = f"  {mark} {turn.headline}" if turn.headline else f"  {mark} {turn.tool}"
                if turn.live:
                    head += " · running"
                out.append((style, head + "\n"))
                if not turn.collapsed and turn.full_output:
                    out.append(("class:muted", "    " + turn.full_output[:1200] + "\n"))
                out.append(("", "\n"))
            elif turn.kind == "diff":
                out.append(("class:accent", f"  propose edit · {turn.tool}\n"))
                for line in turn.text.splitlines()[:200]:
                    if line.startswith("+"):
                        out.append(("class:add", "    " + line + "\n"))
                    elif line.startswith("-"):
                        out.append(("class:del", "    " + line + "\n"))
                    else:
                        out.append(("class:muted", "    " + line + "\n"))
                out.append(("class:muted", "    [y] accept  [n] reject\n\n"))
            elif turn.kind == "approval":
                out.append(("class:warn", f"  ⚠ approve? {turn.tool}\n"))
                out.append(("class:muted", f"    {turn.text}\n"))
                out.append(("class:muted", "    [y] approve  [n] deny\n\n"))
            elif turn.kind == "result":
                style = "class:ok" if turn.ok else "class:danger"
                out.append((style, f"  result › {turn.text}\n\n"))
        if not self.turns:
            out.append(("class:muted", "  Describe a coding task. Try “Create a Python hello-world project”.\n  Type /help for commands.\n\n"))
        return out

    def overlay_text(self, overlay: str | None) -> str:
        if overlay == "help":
            return _help_text()
        if overlay == "models":
            return _models_text(self.cfg, self.store)
        if overlay == "sessions":
            return _sessions_text(self.store)
        return ""


def _help_text() -> str:
    return """\
Shadow Agent — keyboard cheat sheet

  Enter                send task
  Ctrl+J              newline (linefeed)
  Esc                 cancel task / close overlay
  Ctrl+R               rerun last task
  Ctrl+P               session picker
  F2                  model picker
  Ctrl+O               expand/collapse last tool card
  [  /  ]               prev / next tool card
  ↑ / ↓                history navigation
  ?                    this help

Slash commands
  /help /clear /new /model /config /compact /expand /undo /diff /git
  /branch /sessions /resume <id> /pin /cost /doctor /health /ui /quit

Custom commands live in .shadow/commands/*.md and run as tasks.
"""


def _models_text(cfg: AppConfig, store) -> str:
    from shadow_agent.models.registry import ModelRegistry

    reg = ModelRegistry(detect=True)
    lines = [f"default: {cfg.model.default}  provider={cfg.model.provider}", ""]
    for info in reg.list_models():
        mark = "*" if info.id == cfg.model.default else " "
        caps = info.metadata.get("capabilities") or {}
        chips = " ".join(k for k, v in caps.items() if v and k != "completion")
        lines.append(f"{mark} {info.id:24} {info.provider:16} {chips}")
    lines.append("")
    lines.append("Use:  shadow models --use <id>   or   /model in the TUI")
    return "\n".join(lines)


def _sessions_text(store) -> str:
    rows = store.list_sessions(limit=30)
    if not rows:
        return "No sessions yet."
    lines = []
    for row in rows:
        title = row.get("title") or "(untitled)"
        branch = " ↳ branch" if row.get("parent_id") else ""
        lines.append(f"{row['id'][:8]}  {row['status']:10}  {title}{branch}")
    lines.append("")
    lines.append("Resume with:  /resume <id-prefix>")
    return "\n".join(lines)


def __version__() -> str:
    from shadow_agent import __version__ as v

    return v
