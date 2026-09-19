"""USER TASK → UNDERSTAND → PLAN → INSPECT → REASON → TOOL → OBSERVE → VERIFY."""

from __future__ import annotations

from pathlib import Path

from pydantic import BaseModel, Field

from shadow_agent.config import AppConfig, load_config
from shadow_agent.context.engine import ContextEngine
from shadow_agent.context.memory import MemoryStore
from shadow_agent.context.project import ProjectSkills
from shadow_agent.events import EventBus
from shadow_agent.models.interface import ModelProvider
from shadow_agent.models.registry import ModelRegistry
from shadow_agent.models.types import ChatRequest, ToolCall, ToolResult
from shadow_agent.permissions import PermissionGate
from shadow_agent.planning.plan import Plan, initial_plan
from shadow_agent.store import Store
from shadow_agent.system_prompt import SYSTEM_PROMPT
from shadow_agent.tools.registry import ToolRegistry, default_tools
from shadow_agent.tools.sandbox import WorkspaceSandbox
from shadow_agent.verification.recovery import RecoveryPolicy
from shadow_agent.verification.verifier import Verifier


class AgentResult(BaseModel):
    success: bool
    summary: str
    session_id: str
    task_id: str
    steps: int
    plan: Plan
    events: list[str] = Field(default_factory=list)


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
        self.model = model or ModelRegistry().create(self.config)
        self.session_id = session_id or self.store.create_session(str(self.workspace), self.config.model.default)
        self.store.touch_project(self.workspace)

    def run(self, task: str, purpose: str = "coder") -> AgentResult:
        task_id = self.store.create_task(self.session_id, task)
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
        self._emit("agent.planning", {"plan": plan.model_dump()}, task_id)
        context.add_user(task)
        verifier = Verifier(self.workspace)
        recovery = RecoveryPolicy()
        event_names = ["agent.started", "agent.planning"]
        summary = ""
        success = False

        for step in range(1, self.config.agent.max_steps + 1):
            request = ChatRequest(messages=context.messages(), tools=self.tools.specs())
            self._emit("model.request", {"step": step, "model": getattr(self.model, "name", "")}, task_id)
            response = self.model.chat(request)
            self._emit(
                "model.response",
                {
                    "step": step,
                    "text": response.text[:1000],
                    "tool_calls": [c.tool_name for c in response.tool_calls],
                    "finish": response.finish,
                },
                task_id,
            )
            if response.tool_calls:
                context.add_assistant(response.text, response.tool_calls)
                for call in response.tool_calls:
                    result = self._execute(call, task_id, plan, context, verifier, event_names)
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

        if not summary:
            summary = "Completed." if success else "Stopped without a verified result."
        status = "completed" if success else "failed"
        self.store.finish_task(task_id, status, summary)
        self.store.touch_session(self.session_id, status)
        memory.append_task(f"Result ({status}): {summary}")
        if success:
            memory.append_project(f"- Task: {task.splitlines()[0][:160]} → {summary.splitlines()[0][:160]}")
        self._emit("agent.completed", {"success": success, "summary": summary, "steps": step}, task_id)
        event_names.append("agent.completed")
        return AgentResult(
            success=success,
            summary=summary,
            session_id=self.session_id,
            task_id=task_id,
            steps=step,
            plan=plan,
            events=event_names,
        )

    def _execute(
        self,
        call: ToolCall,
        task_id: str,
        plan: Plan,
        context: ContextEngine,
        verifier: Verifier,
        event_names: list[str],
    ) -> ToolResult:
        self._emit("tool.started", {"tool": call.tool_name, "arguments": _safe_args(call)}, task_id)
        result = self.tools.execute(call, self.gate)
        self._emit(
            "tool.completed",
            {
                "tool": call.tool_name,
                "success": result.success,
                "error": result.error,
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

    def _emit(self, event_type: str, payload: dict, task_id: str) -> None:
        event = self.events.emit(event_type, payload, session_id=self.session_id, task_id=task_id)
        self.store.add_event(
            event_type,
            payload,
            session_id=self.session_id,
            task_id=task_id,
            ts=event.get("ts"),
        )


def _looks_finished(text: str) -> bool:
    lowered = text.lower()
    return any(token in lowered for token in ("done.", "completed.", "verified", "all tests passed", "hello, world!"))


def _advance_plan(plan: Plan, call: ToolCall, result: ToolResult) -> None:
    mapping = {
        "list_files": "s1",
        "write_file": "s2",
        "edit_file": "s4",
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
    if "content" in args and isinstance(args["content"], str) and len(args["content"]) > 400:
        args["content"] = args["content"][:400] + "…"
    return args
