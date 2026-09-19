"""USER TASK → UNDERSTAND → PLAN → INSPECT → REASON → TOOL → OBSERVE → VERIFY."""

from __future__ import annotations

import threading
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path
from typing import Any

from pydantic import BaseModel, Field

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
from shadow_agent.verification.verifier import Verifier

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
        self._emit("agent.started", {"task": task, "workspace": str(self.workspace)}, task_id)
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
        plan = initial_plan(task)
        context.set_plan(plan)
        self.todos = [{"id": step.id, "title": step.title, "status": step.status} for step in plan.steps]
        self._emit("agent.planning", {"plan": plan.model_dump(), "todos": self.todos}, task_id)
        context.add_user(task)
        verifier = Verifier(self.workspace)
        recovery = RecoveryPolicy()
        event_names = ["agent.started", "agent.planning"]
        summary = ""
        success = False
        cancelled = False
        step = 0

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
                context.add_assistant(response.text, response.tool_calls)
                results = self._execute_batch(response.tool_calls, task_id, plan, context, verifier, event_names)
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
                verdict = verifier.verify(task)
                if verdict.ok:
                    success = True
                    break
                self._emit("test.failed", {"reason": verdict.reason}, task_id)
                event_names.append("test.failed")
                note = recovery.note(verdict)
                memory.append_task(note)
                context.add_user(note)
                if recovery.exhausted():
                    summary = f"{summary}\n\nStopped: {verdict.reason}".strip()
                    break
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
        self._emit(
            "agent.completed",
            {"success": success, "summary": summary, "steps": step, "usage": self.usage, "cancelled": cancelled},
            task_id,
        )
        event_names.append("agent.completed")
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
        )

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
        result = self.tools.execute(call, self.gate)
        result = self._maybe_approve(call, result, task_id)
        if not result.success and _retryable(result) and not self.cancelled():
            self._emit("tool.retry", {"tool": call.tool_name, "error": result.error}, task_id)
            result = self.tools.execute(call, self.gate)
        self._emit(
            "tool.completed",
            {
                "tool": call.tool_name,
                "success": result.success,
                "error": friendly_error(result.error) if result.error else "",
                "raw_error": result.error,
                "output_preview": result.output[:500],
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
