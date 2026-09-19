"""Tests for the 0.7.0 Codex-style agent loop.

Verifies the stage machine:
  USER → UNDERSTAND → PLAN → INSPECT → ACT → OBSERVE → VERIFY
    → success? → DONE (yes)
               → FIX → OBSERVE → VERIFY (no, retry, capped at max_fix_retries)

Covers:
  - Stage transition events (agent.understand … agent.done / agent.failed)
  - VERIFY → FIX → OBSERVE → VERIFY retry path
  - Retry cap surfaces agent.failed
  - DONE only after a successful VERIFY
  - Events persisted in SQLite
  - AgentResult carries stage + fix_retries + stage_history
"""

from __future__ import annotations

from pathlib import Path

import pytest

from shadow_agent.agent.loop import AgentRunner
from shadow_agent.agent.stages import Stage, StageTracker
from shadow_agent.config import AppConfig
from shadow_agent.events import EventBus
from shadow_agent.models.adapters.mock import MockProvider
from shadow_agent.models.interface import ModelProvider
from shadow_agent.models.types import ChatRequest, ChatResponse, Message, ToolCall
from shadow_agent.store import Store
from shadow_agent.verification.verifier import Verifier


# --- StageTracker unit tests -------------------------------------------------


def test_stage_tracker_emits_on_transition():
    events: list[tuple[str, dict]] = []
    tracker = StageTracker(max_fix_retries=3, emit=lambda et, p, tid: events.append((et, p)))
    tracker.transition(Stage.UNDERSTAND, {"task": "hi"}, task_id="t1")
    assert tracker.current is Stage.UNDERSTAND
    assert events[-1][0] == "agent.understand"
    assert events[-1][1]["stage"] == "UNDERSTAND"
    assert events[-1][1]["fix_retries"] == 0


def test_stage_tracker_fix_chip_and_exhaustion():
    tracker = StageTracker(max_fix_retries=3)
    assert tracker.status_chip() == "IDLE"
    tracker.transition(Stage.VERIFY)
    assert tracker.status_chip() == "VERIFY"
    tracker.begin_fix("tests failed", evidence="boom")
    assert tracker.fix_retries == 1
    assert tracker.status_chip() == "FIX · retry 1/3"
    assert not tracker.exhausted()
    tracker.begin_fix("still failing")
    tracker.begin_fix("again")
    assert tracker.fix_retries == 3
    # 3 retries is the cap; a 4th begin_fix tips over into exhausted.
    tracker.begin_fix("one more")
    assert tracker.exhausted()


def test_stage_tracker_history_records_retries():
    tracker = StageTracker(max_fix_retries=2)
    tracker.transition(Stage.UNDERSTAND)
    tracker.transition(Stage.PLAN)
    tracker.transition(Stage.INSPECT)
    tracker.transition(Stage.ACT)
    tracker.transition(Stage.OBSERVE)
    tracker.transition(Stage.VERIFY)
    tracker.begin_fix("nope")
    tracker.transition(Stage.OBSERVE)
    tracker.transition(Stage.VERIFY)
    stages = [s for s, _ in tracker.history]
    assert stages == [
        Stage.UNDERSTAND, Stage.PLAN, Stage.INSPECT, Stage.ACT,
        Stage.OBSERVE, Stage.VERIFY, Stage.FIX, Stage.OBSERVE, Stage.VERIFY,
    ]
    # The OBSERVE after FIX records the incremented retry counter.
    assert tracker.history[7] == (Stage.OBSERVE, 1)


# --- Loop integration: stage events + transitions ---------------------------


def _stage_events(bus: EventBus) -> list[str]:
    return [e["type"] for e in bus.history() if e["type"].startswith("agent.")]


def test_loop_emits_codex_stage_events(isolated, workspace, store, bus):
    runner = AgentRunner(workspace, config=AppConfig(), store=store, events=bus, model=MockProvider())
    result = runner.run("Create a Python hello-world project")
    assert result.success, result.summary
    types = _stage_events(bus)
    # The full Codex sequence must be present, in order.
    expected = [
        "agent.started",
        "agent.understand",
        "agent.plan",
        "agent.inspect",
        "agent.act",
        "agent.observe",
        "agent.verify",
        "agent.done",
        "agent.completed",
    ]
    for ev in expected:
        assert ev in types, f"missing {ev}; got {types}"
    # DONE only after VERIFY success — no FIX on a clean run.
    assert "agent.fix" not in types
    assert "agent.failed" not in types
    assert result.stage == "DONE"
    assert result.fix_retries == 0
    # stage_history records the actual transition sequence.
    assert result.stage_history[0] == "UNDERSTAND"
    assert result.stage_history[-1] == "DONE"


def test_loop_fix_path_fires_on_verification_failure(isolated, workspace, store, bus):
    """A model that ships the wrong output first, then corrects it, must drive
    VERIFY → FIX → OBSERVE → VERIFY → DONE."""
    runner = AgentRunner(
        workspace, config=AppConfig(), store=store, events=bus,
        model=_CaseMismatchHelloProvider(),
    )
    result = runner.run("Create a Python hello-world project")
    assert result.success, result.summary
    assert (workspace / "hello.py").is_file()
    types = _stage_events(bus)
    assert "agent.fix" in types, f"no FIX event; got {types}"
    assert "agent.verify" in types
    # VERIFY → FIX → OBSERVE → VERIFY → DONE, in order.
    fix_idx = types.index("agent.fix")
    done_idx = types.index("agent.done")
    assert types.index("agent.verify") < fix_idx < done_idx
    observe_after_fix = [i for i, t in enumerate(types) if t == "agent.observe" and i > fix_idx]
    verify_after_fix = [i for i, t in enumerate(types) if t == "agent.verify" and i > fix_idx]
    assert observe_after_fix, "no OBSERVE after FIX — agent did not re-observe"
    assert verify_after_fix, "no VERIFY after FIX — agent did not re-verify"
    assert observe_after_fix[0] < verify_after_fix[0] < done_idx
    assert result.stage == "DONE"
    assert result.fix_retries == 1
    # The fix actually corrected the file.
    assert "Hello, World!" in (workspace / "hello.py").read_text(encoding="utf-8")


def test_loop_retry_cap_emits_agent_failed(isolated, workspace, store, bus):
    """A model that always claims done but never satisfies verification must
    hit the retry cap and emit agent.failed (not loop forever)."""
    cfg = AppConfig()
    cfg.agent.max_fix_retries = 2
    cfg.agent.max_steps = 12
    runner = AgentRunner(
        workspace, config=cfg, store=store, events=bus,
        model=_AlwaysDoneFailingProvider(),
    )
    result = runner.run("Create a Python hello-world project")
    assert not result.success
    types = _stage_events(bus)
    assert "agent.fix" in types
    assert "agent.failed" in types
    assert "agent.done" not in types
    # Cap was respected: FIX events <= max_fix_retries.
    fix_count = types.count("agent.fix")
    assert fix_count <= cfg.agent.max_fix_retries + 1  # begin_fix tips over
    assert result.fix_retries > cfg.agent.max_fix_retries
    assert result.stage == "FIX"


def test_loop_done_only_after_verify_success(isolated, workspace, store, bus):
    """DONE must not be emitted unless VERIFY succeeded."""
    runner = AgentRunner(workspace, config=AppConfig(), store=store, events=bus, model=MockProvider())
    result = runner.run("Create a Python hello-world project")
    assert result.success
    types = _stage_events(bus)
    done_idx = types.index("agent.done")
    # The VERIFY immediately preceding DONE must be a success.
    verify_indices = [i for i, t in enumerate(types) if t == "agent.verify"]
    assert verify_indices and verify_indices[-1] < done_idx
    # No VERIFY after DONE.
    assert not any(t == "agent.verify" for t in types[done_idx:])


def test_loop_events_persisted_in_sqlite(isolated, workspace, store, bus):
    runner = AgentRunner(workspace, config=AppConfig(), store=store, events=bus, model=MockProvider())
    result = runner.run("Create a Python hello-world project")
    rows = store.list_events(session_id=result.session_id, limit=400)
    types = {r["type"] for r in rows}
    assert "agent.understand" in types
    assert "agent.plan" in types
    assert "agent.inspect" in types
    assert "agent.act" in types
    assert "agent.observe" in types
    assert "agent.verify" in types
    assert "agent.done" in types


def test_loop_stage_chip_visible_in_tui_transcript(isolated, workspace, store, bus):
    """The TUI transcript must reflect the current stage as a Codex-style chip."""
    from shadow_agent.tui.theme import DARK
    from shadow_agent.tui.transcript import TranscriptModel

    cfg = AppConfig()
    sid = store.create_session(str(workspace), "mock", title="t")
    model = TranscriptModel(workspace, DARK, cfg, store, sid)
    model.ingest("agent.started", {}, None)
    model.ingest("agent.understand", {"stage": "UNDERSTAND", "fix_retries": 0, "max_fix_retries": 3}, None)
    assert "[ UNDERSTAND ]" in model.status_text()
    model.ingest("agent.fix", {"stage": "FIX", "fix_retries": 1, "max_fix_retries": 3}, None)
    assert "FIX · retry 1/3" in model.status_text()
    model.ingest("agent.done", {"stage": "DONE"}, None)
    assert "[ DONE ]" in model.status_text()


# --- helpers ----------------------------------------------------------------


class _AlwaysDoneFailingProvider(ModelProvider):
    """A mock that immediately claims done but never writes hello.py.

    The Verifier refuses to declare success because hello.py does not exist,
    so the loop must enter FIX repeatedly until the retry cap fires.
    """

    name = "mock"

    def __init__(self, context_limit: int = 32000) -> None:
        self._context_limit = context_limit

    def get_capabilities(self):
        from shadow_agent.models.types import Capabilities
        return Capabilities(chat=True, stream=True, tools=True, vision=False, provider="mock")

    def get_context_limit(self) -> int:
        return self._context_limit

    def generate(self, prompt: str, **kwargs) -> str:
        return self.chat(ChatRequest(messages=[Message(role="user", content=prompt)])).text

    def stream(self, request: ChatRequest):
        yield self.chat(request).text

    def chat(self, request: ChatRequest) -> ChatResponse:
        # Always finish immediately without writing anything → VERIFY always fails.
        return ChatResponse(
            text="Done.",
            tool_calls=[],
            finish=True,
            usage={"prompt_tokens": 8, "completion_tokens": 4, "total_tokens": 12},
        )
