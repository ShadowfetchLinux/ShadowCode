"""Background agent jobs shared by the HTTP API (cancel, stream, current)."""

from __future__ import annotations

import threading
import time
import uuid
from pathlib import Path
from typing import Any

from shadow_agent.agent.loop import AgentResult, AgentRunner
from shadow_agent.approvals import ApprovalHub
from shadow_agent.config import AppConfig, load_config
from shadow_agent.errors import friendly_error
from shadow_agent.events import EventBus
from shadow_agent.notify import notify_done
from shadow_agent.store import Store


class Job:
    def __init__(self, job_id: str, workspace: Path, task: str, session_id: str) -> None:
        self.id = job_id
        self.workspace = workspace
        self.task = task
        self.session_id = session_id
        self.task_id = ""
        self.status = "queued"
        self.summary = ""
        self.error = ""
        self.usage: dict[str, int] = {}
        self.result: AgentResult | None = None
        self.runner: AgentRunner | None = None
        self.model_override: str | None = None
        self.purpose: str = "coder"
        self.started_at = time.time()
        self.finished_at: float | None = None

    def to_dict(self) -> dict[str, Any]:
        payload: dict[str, Any] = {
            "id": self.id,
            "workspace": str(self.workspace),
            "task": self.task,
            "session_id": self.session_id,
            "task_id": self.task_id,
            "status": self.status,
            "summary": self.summary,
            "error": self.error,
            "usage": self.usage,
            "started_at": self.started_at,
            "finished_at": self.finished_at,
        }
        if self.result is not None:
            payload["result"] = self.result.model_dump(mode="json")
        return payload


class JobManager:
    def __init__(self, store: Store, bus: EventBus, approvals: ApprovalHub) -> None:
        self.store = store
        self.bus = bus
        self.approvals = approvals
        self._lock = threading.Lock()
        self._jobs: dict[str, Job] = {}

    def start(
        self,
        workspace: Path,
        task: str,
        session_id: str | None = None,
        config: AppConfig | None = None,
        model: str | None = None,
        purpose: str = "coder",
    ) -> Job:
        workspace = Path(workspace).resolve()
        cfg = config or load_config(workspace)
        sid = session_id or self.store.create_session(str(workspace), cfg.model.default, title=task.splitlines()[0][:80])
        job = Job(uuid.uuid4().hex, workspace, task, sid)
        job.model_override = model
        job.purpose = purpose
        with self._lock:
            self._jobs[job.id] = job
        thread = threading.Thread(target=self._run, args=(job, cfg), name=f"shadow-job-{job.id[:8]}", daemon=True)
        thread.start()
        return job

    def _run(self, job: Job, config: AppConfig) -> None:
        job.status = "running"
        try:
            runner = AgentRunner(
                job.workspace,
                config=config,
                store=self.store,
                events=self.bus,
                session_id=job.session_id,
                approval_hub=self.approvals,
                model_override=job.model_override,
                purpose=job.purpose,
            )
            job.runner = runner
            result = runner.run(job.task)
            job.result = result
            job.task_id = result.task_id
            job.summary = result.summary
            job.usage = result.usage
            if result.cancelled:
                job.status = "cancelled"
            else:
                job.status = "completed" if result.success else "failed"
        except Exception as exc:
            job.status = "failed"
            job.error = friendly_error(exc)
            job.summary = job.error
        finally:
            job.finished_at = time.time()
            job.runner = None
            duration = job.finished_at - job.started_at
            sent = notify_done(
                job.task,
                success=job.status == "completed",
                duration_sec=duration,
                summary=job.summary,
                enabled=config.ui.notify,
                after_sec=config.ui.notify_after_sec,
            )
            if sent:
                self.store.add_event(
                    "notify.sent",
                    {"task": job.task[:120], "status": job.status, "duration_sec": round(duration, 2)},
                    session_id=job.session_id,
                    task_id=job.task_id or None,
                )

    def cancel(self, job_id: str) -> Job:
        job = self.get(job_id)
        if job is None:
            raise KeyError(job_id)
        if job.runner is not None:
            job.runner.cancel()
        if job.status in {"queued", "running"}:
            job.status = "cancelled"
            job.summary = job.summary or "Stopped by the user."
        return job

    def cancel_session(self, session_id: str) -> list[str]:
        stopped: list[str] = []
        for job in self.list_active():
            if job.session_id == session_id:
                self.cancel(job.id)
                stopped.append(job.id)
        return stopped

    def get(self, job_id: str) -> Job | None:
        with self._lock:
            return self._jobs.get(job_id)

    def current(self, session_id: str | None = None) -> Job | None:
        active = self.list_active()
        if session_id:
            active = [job for job in active if job.session_id == session_id]
        return active[0] if active else None

    def list_active(self) -> list[Job]:
        with self._lock:
            return [job for job in self._jobs.values() if job.status in {"queued", "running"}]
