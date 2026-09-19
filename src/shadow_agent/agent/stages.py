"""Codex-style agent loop stages.

The Shadow Agent loop is a fixed state machine:

    USER → UNDERSTAND → PLAN → INSPECT → ACT → OBSERVE → VERIFY
            → success? → DONE (yes)
                       → FIX → OBSERVE (no, retry) → VERIFY …

Stages are observable (events + UI status line) and provider-agnostic: the
model only reasons and selects tools; the harness owns the stage machine,
the retry cap, and the verify/fix transition.

Stages
-------
UNDERSTAND  parse user task + constraints + workspace context
PLAN        produce / update the explicit plan + todos
INSPECT     read files / git / search before changing anything
ACT         propose + apply edits / run commands / tool calls
OBSERVE     capture tool results, build/test output, diffs
VERIFY      check success against the task
FIX         diagnose a VERIFY failure and plan a correction; loop back to
            OBSERVE (re-run / re-check) before VERIFY again — never back to
            ACT blindly
DONE        summarize changes, update plan status
"""

from __future__ import annotations

from enum import Enum
from typing import Any


class Stage(str, Enum):
    UNDERSTAND = "UNDERSTAND"
    PLAN = "PLAN"
    INSPECT = "INSPECT"
    ACT = "ACT"
    OBSERVE = "OBSERVE"
    VERIFY = "VERIFY"
    FIX = "FIX"
    DONE = "DONE"


# Ordered for status-line rendering and tests.
STAGE_ORDER: tuple[Stage, ...] = (
    Stage.UNDERSTAND,
    Stage.PLAN,
    Stage.INSPECT,
    Stage.ACT,
    Stage.OBSERVE,
    Stage.VERIFY,
    Stage.FIX,
    Stage.DONE,
)

# Event name for each stage.
STAGE_EVENT: dict[Stage, str] = {
    Stage.UNDERSTAND: "agent.understand",
    Stage.PLAN: "agent.plan",
    Stage.INSPECT: "agent.inspect",
    Stage.ACT: "agent.act",
    Stage.OBSERVE: "agent.observe",
    Stage.VERIFY: "agent.verify",
    Stage.FIX: "agent.fix",
    Stage.DONE: "agent.done",
}


class StageTracker:
    """Tracks the current loop stage, retry counter, and transitions.

    The tracker is the single source of truth for the UI status line. Every
    transition emits a stage event through the provided callback so the
    SQLite event log and the SSE stream stay in sync with the chip the user
    sees.
    """

    def __init__(self, max_fix_retries: int = 3, emit=None) -> None:
        self.max_fix_retries = max(1, int(max_fix_retries))
        self._emit = emit
        self.current: Stage | None = None
        self.fix_retries = 0
        self.history: list[tuple[Stage, int]] = []

    def transition(self, stage: Stage, payload: dict[str, Any] | None = None, *, task_id: str | None = None) -> None:
        """Move to ``stage`` and emit the matching ``agent.<stage>`` event.

        Emits even when transitioning to the same stage (e.g. OBSERVE after a
        re-run) so the UI can update the retry counter and live output.
        """
        self.current = stage
        self.history.append((stage, self.fix_retries))
        if self._emit is not None:
            event_payload = dict(payload or {})
            event_payload.setdefault("stage", stage.value)
            event_payload.setdefault("fix_retries", self.fix_retries)
            event_payload.setdefault("max_fix_retries", self.max_fix_retries)
            self._emit(STAGE_EVENT[stage], event_payload, task_id)

    def begin_fix(self, reason: str, *, task_id: str | None = None, evidence: str = "") -> int:
        """Enter FIX after a VERIFY failure. Returns the new retry count.

        The caller is responsible for re-observing (re-running the test /
        command) before calling VERIFY again. The retry counter caps the
        total FIX attempts; once it exceeds ``max_fix_retries`` the loop
        must surface ``agent.failed`` instead of looping again.
        """
        self.fix_retries += 1
        self.transition(
            Stage.FIX,
            {
                "reason": reason,
                "evidence": evidence[:2000],
                "retry": self.fix_retries,
                "max_fix_retries": self.max_fix_retries,
                "exhausted": self.fix_retries > self.max_fix_retries,
            },
            task_id=task_id,
        )
        return self.fix_retries

    def exhausted(self) -> bool:
        return self.fix_retries > self.max_fix_retries

    def status_chip(self) -> str:
        """Codex-style chip for the status line, e.g. ``VERIFY`` or ``FIX · retry 1/3``."""
        if self.current is None:
            return "IDLE"
        if self.current is Stage.FIX:
            return f"FIX · retry {self.fix_retries}/{self.max_fix_retries}"
        return self.current.value
