"""Codex-style agent loop.

USER → UNDERSTAND → PLAN → INSPECT → ACT → OBSERVE → VERIFY
  → success? → DONE (yes)
             → FIX → OBSERVE (no, retry) → VERIFY …

The harness owns the stage machine, the retry cap, and the verify/fix
transition. The model only reasons and selects tools.
"""

from __future__ import annotations

import threading
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path
from typing import Any

from pydantic import BaseModel, Field

from shadow_agent.agent.stages import Stage, StageTracker
from shadow_agent.approvals import ApprovalHub
from shadow_agent.checkpoints import CheckpointStore
from shadow_agent.config import AppConfig, load_config, remember_workspace
from shadow_agent.context.engine import ContextEngine, estimate_tokens
from shadow_agent.context.memory import MemoryStore
from shadow_agent.context.project import ProjectSkills
from shadow_agent.errors import friendly_error
from shadow_agent.events import EventBus
from shadow_agent.models.interface import ModelProvider
from shadow_agent.models.registry import ModelRegistry
from shadow_agent.models.types import ChatRequest, ToolCall, ToolResult
from shadow_agent.permissions import READ_TOOLS, PermissionGate
from shadow_agent.planning.plan import Plan, initial_plan
from shadow_agent.store import Store
from shadow_agent.system_prompt import SYSTEM_PROMPT
from shadow_agent.tools.registry import ToolRegistry, default_tools
from shadow_agent.tools.sandbox import WorkspaceSandbox
from shadow_agent.verification.recovery import RecoveryPolicy
from shadow_agent.verification.verifier import VerificationResult, Verifier

WRITE_PARALLEL_UNSAFE = {
    "write_file",
    "edit_file",
    "apply_patch",
    "delete_file",
    "move_file",
    "exec",
    "kill",
    "git_add",
    "git_commit",
    "git_checkout",
    "git_reset",
    "git_clean",
}


class AgentResult(BaseModel):
    success: bool
    summary: str
    session_id: str
    task_id: str
    steps: int
    plan: Plan
    events: list[str] = Field(default_factory=list)
    todos: list[dict[str, Any]] = Field(default_factory=list)
    usage: dict[str, int] = Field(default_factory=dict)
    cancelled: bool = False
    stage: str = "DONE"
    fix_retries: int = 0
    stage_history: list[str] = Field(default_factory=list)


class AgentRunner:
    def __init__(
        self,
        workspace: Path,
        config: AppConfig | None = None,
        store: Store | None = None,
        events: EventBus | None = None,
        model: ModelProvider | None = None,
        tools: ToolRegistry | None = None,
        gate: PermissionGate | None = None,
        session_id: str | None = None,
        approval_hub: ApprovalHub | None = None,
        model_override: str | None = None,
        purpose: str = "coder",
    ) -> None:
        self.workspace = Path(workspace).resolve()
        self.config = config or load_config(self.workspace)
        self.store = store or Store()
        self.events = events or EventBus()
        self.sandbox = WorkspaceSandbox(self.workspace)
        self.gate = gate or PermissionGate(
            self.config.permissions.level,
            require_approval_for_dangerous=self.config.permissions.require_approval_for_dangerous,
            network=self.config.permissions.network,
            allow_root=self.config.permissions.allow_root,
            auto_approve=False,
        )
        self.tools = tools or default_tools(self.sandbox, self.gate, self.config.agent.tool_timeout_sec)
        self.model = model or self._resolve_model(model_override, purpose)
        self.session_id = session_id or self.store.create_session(str(self.workspace), self.config.model.default)
        self.store.touch_project(self.workspace)
        remember_workspace(self.workspace)
        self.approval_hub = approval_hub
        self._cancel = threading.Event()
        self.usage = {"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0}
        self.todos: list[dict[str, Any]] = []
        self.checkpoints: CheckpointStore | None = None
        # Hooks: deterministic lifecycle events. Built-ins + project .shadowcode/hooks/.
        from shadow_agent.hooks import HookContext, default_registry, outcomes_blocked

        self.hooks = default_registry(self.workspace)
        self._hook_outcomes_blocked = outcomes_blocked  # for tests / clarity

    def _resolve_model(self, model_override: str | None, purpose: str) -> ModelProvider:
        """Pick the provider: explicit override > routing hint for this purpose > default."""
        registry = ModelRegistry()
        chosen = model_override
        if not chosen and self.config.routing.enabled:
            hint = getattr(self.config.routing, purpose, "") or ""
            if hint and hint != "mock":
                chosen = hint
        if chosen and registry.get(chosen) is None:
            # Allow detected-but-unregistered ids (e.g. an Ollama tag) by probing once.
            registry.refresh_detected()
        return registry.create(self.config, model_id=chosen)

    def cancel(self) -> None:
        self._cancel.set()

    def cancelled(self) -> bool:
        return self._cancel.is_set()

    def run(self, task: str, purpose: str = "coder") -> AgentResult:
        task_id = self.store.create_task(self.session_id, task)
        title = task.splitlines()[0][:80]
        self.store.set_session_title(self.session_id, title)
        self.checkpoints = CheckpointStore(self.workspace, task_id)
        self.stages = StageTracker(
            max_fix_retries=int(self.config.agent.max_fix_retries),
            emit=self._emit,
        )
        self._emit("agent.started", {"task": task, "workspace": str(self.workspace)}, task_id)
        # Self-skilling: observe the task so the workflow detector can flag repeats.
        try:
            from shadow_agent.self_skill import WorkflowDetector

            WorkflowDetector(paths.state_dir()).observe(task)
        except Exception:  # noqa: BLE001 — detector is best-effort
            pass
        memory = MemoryStore(self.workspace, task_id)
        memory.append_task(f"Goal: {task}")
        skills = ProjectSkills(self.workspace)
        context = ContextEngine(limit=self.model.get_context_limit())
        system = SYSTEM_PROMPT
        extra = skills.prompt_block()
        if extra:
            system += "\n\n" + extra
        context.set_system(system)
        context.set_memory(memory.combined())

        # UNDERSTAND: parse the user task + constraints + workspace context.
        self.stages.transition(
            Stage.UNDERSTAND,
            {"task": task, "workspace": str(self.workspace), "purpose": purpose},
            task_id=task_id,
        )

        # PLAN: produce the explicit plan + todos.
        plan = initial_plan(task)
        context.set_plan(plan)
        self.todos = [{"id": step.id, "title": step.title, "status": step.status} for step in plan.steps]
        self.stages.transition(
            Stage.PLAN,
            {"plan": plan.model_dump(), "todos": self.todos},
            task_id=task_id,
        )
        # Legacy alias: older UI/CLI/tests still listen for agent.planning.
        self._emit("agent.planning", {"plan": plan.model_dump(), "todos": self.todos}, task_id)
        context.add_user(task)
        verifier = Verifier(self.workspace)
        event_names = ["agent.started", "agent.understand", "agent.plan", "agent.planning"]
        summary = ""
        success = False
        cancelled = False
        step = 0
        last_verify_ok = False

        for step in range(1, self.config.agent.max_steps + 1):
            if self.cancelled():
                cancelled = True
                summary = "Stopped by the user."
                break
            before_notes = len(context.compressed_notes)
            request = ChatRequest(messages=context.messages(), tools=self.tools.specs())
            if len(context.compressed_notes) > before_notes:
                self._emit(
                    "context.compacted",
                    {"notes": len(context.compressed_notes), "tokens": estimate_tokens("".join(m.content for m in request.messages))},
                    task_id,
                )
                # Hook: on_compaction — fires when the context engine compacts turns.
                try:
                    from shadow_agent.hooks import HookContext

                    ctx = HookContext(
                        workspace=self.workspace,
                        event="on_compaction",
                        task_id=task_id,
                        session_id=self.session_id,
                        extra={"notes": len(context.compressed_notes)},
                    )
                    outcomes = self.hooks.fire("on_compaction", ctx)
                    if outcomes:
                        self._emit("hook.fired", {"event": "on_compaction", "outcomes": [o.to_dict() for o in outcomes]}, task_id)
                except Exception:  # noqa: BLE001
                    pass
            self._emit("model.request", {"step": step, "model": getattr(self.model, "name", "")}, task_id)
            try:
                response = self._chat_with_retry(request, task_id, step)
            except Exception as exc:
                summary = friendly_error(exc)
                self._emit("model.error", {"step": step, "error": summary, "raw": str(exc)}, task_id)
                break
            self._add_usage(response.usage)
            if response.text:
                self._emit("model.delta", {"step": step, "text": response.text}, task_id)
            self._emit(
                "model.response",
                {
                    "step": step,
                    "text": response.text[:1000],
                    "tool_calls": [c.tool_name for c in response.tool_calls],
                    "finish": response.finish,
                    "usage": response.usage,
                },
                task_id,
            )
            if response.tool_calls:
                # Classify the turn: INSPECT (read-only) or ACT (write/exec).
                stage = self._classify_turn(response.tool_calls)
                self.stages.transition(
                    stage,
                    {"step": step, "tools": [c.tool_name for c in response.tool_calls]},
                    task_id=task_id,
                )
                if stage is Stage.INSPECT:
                    event_names.append("agent.inspect")
                elif stage is Stage.ACT:
                    event_names.append("agent.act")
                context.add_assistant(response.text, response.tool_calls)
                results = self._execute_batch(response.tool_calls, task_id, plan, context, verifier, event_names)
                # OBSERVE: capture tool results, build/test output, diffs.
                self.stages.transition(
                    Stage.OBSERVE,
                    {
                        "step": step,
                        "tools": [c.tool_name for c in response.tool_calls],
                        "ok": all(r.success for r in results),
                        "results": [
                            {"tool": c.tool_name, "success": r.success, "preview": (r.output or r.error or "")[:500]}
                            for c, r in zip(response.tool_calls, results, strict=False)
                        ],
                    },
                    task_id=task_id,
                )
                event_names.append("agent.observe")
                for call, result in zip(response.tool_calls, results, strict=False):
                    context.add_tool_result(result, call.tool_name)
                    if call.tool_name == "update_plan":
                        plan.update(
                            step_id=str(call.arguments.get("step_id") or "") or None,
                            status=call.arguments.get("status"),  # type: ignore[arg-type]
                            title=call.arguments.get("title"),
                            detail=call.arguments.get("detail"),
                        )
                        context.set_plan(plan)
                        self._emit("plan.updated", {"plan": plan.model_dump()}, task_id)
                    if call.tool_name == "update_todos":
                        raw = call.arguments.get("todos") or []
                        if isinstance(raw, list):
                            self.todos = [item if isinstance(item, dict) else {"title": str(item)} for item in raw]
                            self._emit("todos.updated", {"todos": self.todos}, task_id)
                continue

            summary = response.text or summary
            context.add_assistant(response.text)
            if response.finish or _looks_finished(response.text):
                # VERIFY: check success against the task.
                verdict = verifier.verify(task)
                self.stages.transition(
                    Stage.VERIFY,
                    {"ok": verdict.ok, "reason": verdict.reason, "evidence": verdict.evidence[:1000]},
                    task_id=task_id,
                )
                event_names.append("agent.verify")
                last_verify_ok = verdict.ok
                if verdict.ok:
                    success = True
                    self.stages.transition(
                        Stage.DONE,
                        {"summary": summary, "steps": step},
                        task_id=task_id,
                    )
                    event_names.append("agent.done")
                    break
                self._emit("test.failed", {"reason": verdict.reason}, task_id)
                event_names.append("test.failed")
                # FIX: diagnose failure, plan a correction. Loop back to OBSERVE
                # (re-run / re-check) — never back to ACT blindly. The recovery
                # note instructs the model to re-run verification, which the
                # next iteration will OBSERVE before VERIFY again.
                self.stages.begin_fix(
                    verdict.reason,
                    task_id=task_id,
                    evidence=verdict.evidence,
                )
                event_names.append("agent.fix")
                if self.stages.exhausted():
                    # Retry cap exceeded: surface a clear failure to the user.
                    self._emit(
                        "agent.failed",
                        {
                            "reason": verdict.reason,
                            "evidence": verdict.evidence[:2000],
                            "fix_retries": self.stages.fix_retries,
                            "max_fix_retries": self.stages.max_fix_retries,
                        },
                        task_id,
                    )
                    event_names.append("agent.failed")
                    summary = f"{summary}\n\nStopped: verification failed {self.stages.fix_retries} times ({verdict.reason})".strip()
                    break
                note = _fix_note(verdict, self.stages.fix_retries, self.stages.max_fix_retries)
                memory.append_task(note)
                context.add_user(note)
                continue
            if not response.text:
                context.add_user("No tool was selected and no message was returned. Inspect, then act or finish.")

        if cancelled:
            success = False
            if not summary:
                summary = "Stopped by the user."
        if not summary:
            summary = "Completed." if success else "Stopped without a verified result."
        status = "cancelled" if cancelled else ("completed" if success else "failed")
        self.store.finish_task(task_id, status, summary)
        self.store.touch_session(self.session_id, status)
        self.store.add_usage(self.session_id, task_id, self.usage)
        memory.append_task(f"Result ({status}): {summary}")
        if success:
            memory.append_project(f"- Task: {task.splitlines()[0][:160]} → {summary.splitlines()[0][:160]}")
        if not success and not cancelled and not last_verify_ok and "agent.failed" not in event_names:
            # Loop ended without a successful VERIFY and without an explicit
            # failure emission (e.g. max_steps hit). Surface it as failed.
            self._emit(
                "agent.failed",
                {
                    "reason": "max_steps reached without verification",
                    "fix_retries": self.stages.fix_retries,
                    "max_fix_retries": self.stages.max_fix_retries,
                },
                task_id,
            )
            event_names.append("agent.failed")
        self._emit(
            "agent.completed",
            {"success": success, "summary": summary, "steps": step, "usage": self.usage, "cancelled": cancelled,
             "stage": self.stages.current.value if self.stages.current else "DONE",
             "fix_retries": self.stages.fix_retries},
            task_id,
        )
        event_names.append("agent.completed")
        # Hook: on_complete — fires when the loop finishes (success or not).
        try:
            from shadow_agent.hooks import HookContext

            ctx = HookContext(
                workspace=self.workspace,
                event="on_complete",
                task_id=task_id,
                session_id=self.session_id,
                extra={"success": success, "summary": summary, "steps": step},
            )
            outcomes = self.hooks.fire("on_complete", ctx)
            if outcomes:
                self._emit("hook.fired", {"event": "on_complete", "outcomes": [o.to_dict() for o in outcomes]}, task_id)
        except Exception:  # noqa: BLE001 — hooks must not crash the loop
            pass
        return AgentResult(
            success=success,
            summary=summary,
            session_id=self.session_id,
            task_id=task_id,
            steps=step,
            plan=plan,
            events=event_names,
            todos=self.todos,
            usage=self.usage,
            cancelled=cancelled,
            stage=self.stages.current.value if self.stages.current else "DONE",
            fix_retries=self.stages.fix_retries,
            stage_history=[s.value for s, _ in self.stages.history],
        )

    @staticmethod
    def _classify_turn(calls: list[ToolCall]) -> Stage:
        """INSPECT if every call is read-only; ACT if any call mutates."""
        from shadow_agent.permissions import READ_TOOLS
        if all(call.tool_name in READ_TOOLS for call in calls):
            return Stage.INSPECT
        return Stage.ACT

    def _execute_batch(
        self,
        calls: list[ToolCall],
        task_id: str,
        plan: Plan,
        context: ContextEngine,
        verifier: Verifier,
        event_names: list[str],
    ) -> list[ToolResult]:
        if (
            self.config.agent.parallel_reads
            and len(calls) > 1
            and all(call.tool_name in READ_TOOLS or call.tool_name not in WRITE_PARALLEL_UNSAFE for call in calls)
            and all(call.tool_name in READ_TOOLS for call in calls)
        ):
            self._emit("tool.parallel", {"tools": [c.tool_name for c in calls], "count": len(calls)}, task_id)
            results: dict[int, ToolResult] = {}
            with ThreadPoolExecutor(max_workers=min(8, len(calls))) as pool:
                futures = {
                    pool.submit(self._execute, call, task_id, plan, context, verifier, event_names): idx
                    for idx, call in enumerate(calls)
                }
                for future in as_completed(futures):
                    results[futures[future]] = future.result()
            return [results[idx] for idx in range(len(calls))]
        return [self._execute(call, task_id, plan, context, verifier, event_names) for call in calls]

    def _execute(
        self,
        call: ToolCall,
        task_id: str,
        plan: Plan,
        context: ContextEngine,
        verifier: Verifier,
        event_names: list[str],
    ) -> ToolResult:
        if self.cancelled():
            return ToolResult(id=call.id, success=False, error="Stopped by the user.", metadata={"cancelled": True})
        if self.checkpoints is not None:
            self.checkpoints.record_call(call.tool_name, call.arguments)
        self._emit("tool.started", {"tool": call.tool_name, "arguments": _safe_args(call)}, task_id)
        # Hook: before_command — fires for exec/terminal tools; can block dangerous commands.
        if call.tool_name in {"exec", "kill"}:
            from shadow_agent.hooks import HookContext

            ctx = HookContext(
                workspace=self.workspace,
                event="before_command",
                tool=call.tool_name,
                arguments=call.arguments,
                command=str(call.arguments.get("command") or ""),
                task_id=task_id,
                session_id=self.session_id,
            )
            outcomes = self.hooks.fire("before_command", ctx)
            if self._hook_outcomes_blocked(outcomes):
                msg = "; ".join(o.message for o in outcomes if o.block)
                self._emit("hook.blocked", {"tool": call.tool_name, "message": msg, "outcomes": [o.to_dict() for o in outcomes]}, task_id)
                return ToolResult(id=call.id, success=False, error=f"blocked by hook: {msg}", metadata={"blocked_by_hook": True})
            if outcomes:
                self._emit("hook.fired", {"event": "before_command", "outcomes": [o.to_dict() for o in outcomes]}, task_id)
        result = self.tools.execute(call, self.gate)
        result = self._maybe_approve(call, result, task_id)
        if not result.success and _retryable(result) and not self.cancelled():
            self._emit("tool.retry", {"tool": call.tool_name, "error": result.error}, task_id)
            result = self.tools.execute(call, self.gate)
        # Hook: after_edit — fires after write_file / edit_file / apply_patch.
        if call.tool_name in {"write_file", "edit_file", "apply_patch"} and result.success:
            from shadow_agent.hooks import HookContext

            ctx = HookContext(
                workspace=self.workspace,
                event="after_edit",
                tool=call.tool_name,
                arguments=call.arguments,
                result=result,
                task_id=task_id,
                session_id=self.session_id,
            )
            outcomes = self.hooks.fire("after_edit", ctx)
            if outcomes:
                self._emit("hook.fired", {"event": "after_edit", "outcomes": [o.to_dict() for o in outcomes]}, task_id)
        # Hook: after_test — fires after a test command finishes.
        if call.tool_name == "exec":
            command = str(call.arguments.get("command") or "")
            if _is_test_command(command):
                from shadow_agent.hooks import HookContext

                ctx = HookContext(
                    workspace=self.workspace,
                    event="after_test",
                    tool=call.tool_name,
                    arguments=call.arguments,
                    result=result,
                    command=command,
                    task_id=task_id,
                    session_id=self.session_id,
                )
                outcomes = self.hooks.fire("after_test", ctx)
                if outcomes:
                    self._emit("hook.fired", {"event": "after_test", "outcomes": [o.to_dict() for o in outcomes]}, task_id)
        # Hook: before_commit — fires before git_commit.
        if call.tool_name == "git_commit":
            from shadow_agent.hooks import HookContext

            ctx = HookContext(
                workspace=self.workspace,
                event="before_commit",
                tool=call.tool_name,
                arguments=call.arguments,
                task_id=task_id,
                session_id=self.session_id,
            )
            outcomes = self.hooks.fire("before_commit", ctx)
            if outcomes:
                self._emit("hook.fired", {"event": "before_commit", "outcomes": [o.to_dict() for o in outcomes]}, task_id)
        # Hook: on_error — fires when a tool call errors.
        if not result.success and result.error:
            from shadow_agent.hooks import HookContext

            ctx = HookContext(
                workspace=self.workspace,
                event="on_error",
                tool=call.tool_name,
                arguments=call.arguments,
                result=result,
                error=result.error,
                task_id=task_id,
                session_id=self.session_id,
            )
            outcomes = self.hooks.fire("on_error", ctx)
            if outcomes:
                self._emit("hook.fired", {"event": "on_error", "outcomes": [o.to_dict() for o in outcomes]}, task_id)
        # Codex-style compact operation card: one-line headline + expandable full output.
        from shadow_agent.op_card import summarize as _op_summarize

        card = _op_summarize(call.tool_name, call.arguments, result)
        self._emit(
            "tool.completed",
            {
                "tool": call.tool_name,
                "success": result.success,
                "error": friendly_error(result.error) if result.error else "",
                "raw_error": result.error,
                "output_preview": result.output[:500],
                "icon": card.icon,
                "headline": card.headline,
                "output_full": card.full_output,
                "arguments": _safe_args(call),
            },
            task_id,
        )
        kind = verifier.observe(call, result)
        if kind:
            self._emit(kind, {"command": call.arguments.get("command"), "success": result.success}, task_id)
            event_names.append(kind)
        _advance_plan(plan, call, result)
        context.set_plan(plan)
        return result

    def _maybe_approve(self, call: ToolCall, result: ToolResult, task_id: str) -> ToolResult:
        if result.success or not result.metadata.get("needs_approval") or self.approval_hub is None:
            return result
        payload = {
            "session_id": self.session_id,
            "task_id": task_id,
            "tool": call.tool_name,
            "arguments": _safe_args(call),
            "reason": result.error,
            "command": call.arguments.get("command"),
        }
        self._emit("approval.requested", payload, task_id)
        decision = self.approval_hub.request(payload)
        self._emit("approval.resolved", {"decision": decision, "command": payload.get("command")}, task_id)
        if decision != "approve":
            return ToolResult(
                id=call.id,
                success=False,
                error="You denied this command.",
                metadata={"denied": True, "decision": decision},
            )
        previous = self.gate.auto_approve
        self.gate.auto_approve = True
        try:
            return self.tools.execute(call, self.gate)
        finally:
            self.gate.auto_approve = previous

    def _chat_with_retry(self, request: ChatRequest, task_id: str, step: int):
        """Retry transient provider failures (429/5xx/connection) with exponential backoff."""
        attempts = max(1, int(self.config.agent.model_retries))
        backoff = max(0.0, float(self.config.agent.retry_backoff_sec))
        last_exc: Exception | None = None
        for attempt in range(1, attempts + 1):
            if self.cancelled():
                raise RuntimeError("Stopped by the user.")
            try:
                return self.model.chat(request)
            except Exception as exc:  # noqa: BLE001 - classified below
                last_exc = exc
                if not _provider_retryable(exc) or attempt >= attempts:
                    raise
                wait = backoff * (2 ** (attempt - 1))
                self._emit(
                    "model.retry",
                    {
                        "step": step,
                        "attempt": attempt,
                        "max_attempts": attempts,
                        "wait_sec": wait,
                        "error": friendly_error(exc),
                    },
                    task_id,
                )
                if wait > 0:
                    time.sleep(wait)
        assert last_exc is not None
        raise last_exc

    def _add_usage(self, usage: dict[str, int]) -> None:
        for key, value in (usage or {}).items():
            self.usage[key] = int(self.usage.get(key, 0)) + int(value)

    def _emit(self, event_type: str, payload: dict, task_id: str) -> None:
        event = self.events.emit(event_type, payload, session_id=self.session_id, task_id=task_id)
        self.store.add_event(
            event_type,
            payload,
            session_id=self.session_id,
            task_id=task_id,
            ts=event.get("ts"),
        )


def _provider_retryable(exc: Exception) -> bool:
    """Classify transient provider/transport failures worth retrying."""
    try:
        import httpx
    except ImportError:  # pragma: no cover - httpx is a hard dependency
        httpx = None  # type: ignore[assignment]
    if httpx is not None:
        if isinstance(exc, (httpx.ConnectError, httpx.ReadTimeout, httpx.WriteTimeout, httpx.ConnectTimeout, httpx.RemoteProtocolError)):
            return True
        if isinstance(exc, httpx.HTTPStatusError):
            status = exc.response.status_code
            return status == 429 or status >= 500
    text = str(exc).lower()
    return any(token in text for token in ("connection reset", "temporarily unavailable", "service unavailable", "too many requests"))


def _looks_finished(text: str) -> bool:
    lowered = text.lower()
    return any(token in lowered for token in ("done.", "completed.", "verified", "all tests passed", "hello, world!"))


def _fix_note(verdict: VerificationResult, attempt: int, max_attempts: int) -> str:
    """Recovery note appended after a VERIFY failure.

    Instructs the model to OBSERVE the failure's effect (re-run the test /
    command) before claiming success again — never to ACT blindly. The retry
    counter is shown so the model knows how close it is to the cap.
    """
    return (
        f"VERIFICATION FAILED (FIX · retry {attempt}/{max_attempts}). "
        f"Reason: {verdict.reason}. Evidence:\n{verdict.evidence[:2000]}\n"
        "Re-run the test or command first to OBSERVE the actual output, "
        "then apply a structured fix, then re-run verification. "
        "Do not claim success without a fresh OBSERVE→VERIFY."
    )


def _advance_plan(plan: Plan, call: ToolCall, result: ToolResult) -> None:
    mapping = {
        "list_files": "s1",
        "write_file": "s2",
        "edit_file": "s4",
        "apply_patch": "s4",
        "exec": None,
    }
    step_id = mapping.get(call.tool_name)
    if call.tool_name == "exec":
        command = str(call.arguments.get("command") or "")
        if "pytest" in command:
            step_id = "s5" if result.success else "s2"
        elif "hello" in command:
            step_id = "s3"
    if step_id:
        plan.update(step_id, status="done" if result.success else "failed", detail=call.tool_name)
        plan.mark_next()


def _safe_args(call: ToolCall) -> dict:
    args = dict(call.arguments)
    for key in ("content", "patch", "diff", "old_string", "new_string"):
        if key in args and isinstance(args[key], str) and len(args[key]) > 400:
            args[key] = args[key][:400] + "…"
    return args


def _retryable(result: ToolResult) -> bool:
    text = (result.error or "").lower()
    return any(token in text for token in ("timed out", "timeout", "temporarily", "connection reset"))


def _is_test_command(command: str) -> bool:
    import re

    return bool(re.search(r"\bpytest\b|\bpython3?\s+-m\s+pytest\b|\bpython3?\s+-m\s+unittest\b", command))
