"""Agent API used by both the CLI and the desktop UI."""

from __future__ import annotations

import json
import time
from pathlib import Path
from typing import Any

from fastapi import FastAPI, HTTPException
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import FileResponse, PlainTextResponse, StreamingResponse
from fastapi.staticfiles import StaticFiles
from pydantic import BaseModel, Field

from shadow_agent import __version__, paths
from shadow_agent.agent.loop import AgentRunner
from shadow_agent.approvals import ApprovalHub
from shadow_agent.checkpoints import last_checkpoint, restore_last
from shadow_agent.commands import CommandContext, CommandRegistry, dispatch as dispatch_command
from shadow_agent.config import (
    AppConfig,
    PermissionLevel,
    apply_config_patch,
    ensure_user_config,
    last_workspace,
    load_config,
    remember_workspace,
    save_config,
    set_config_value,
)
from shadow_agent.errors import friendly_error, friendly_http
from shadow_agent.events import EventBus
from shadow_agent.export import export_session
from shadow_agent.health import collect_health, doctor_report
from shadow_agent.models.discovery import detect_providers
from shadow_agent.models.registry import ModelRegistry, probe_provider
from shadow_agent.runtime import JobManager
from shadow_agent.secrets import has_secret, load_secrets, set_secret
from shadow_agent.store import Store
from shadow_agent.tools.sandbox import SandboxError, WorkspaceSandbox

UI_DIST = Path(__file__).resolve().parents[3] / "ui" / "dist"

PROVIDER_PRESETS: dict[str, dict[str, str]] = {
    "mock": {"provider": "mock", "default": "mock", "name": "mock-coder", "endpoint": "", "api_key_env": "OPENAI_API_KEY"},
    "grok": {
        "provider": "openai_compatible",
        "default": "grok",
        "name": "grok-4",
        "endpoint": "https://api.x.ai/v1",
        "api_key_env": "XAI_API_KEY",
    },
    "openai": {
        "provider": "openai_compatible",
        "default": "openai",
        "name": "gpt-4.1",
        "endpoint": "https://api.openai.com/v1",
        "api_key_env": "OPENAI_API_KEY",
    },
    "ollama": {
        "provider": "ollama",
        "default": "ollama",
        "name": "gpt-oss:20b",
        "endpoint": "http://127.0.0.1:11434/v1",
        "api_key_env": "OLLAMA_API_KEY",
    },
    "local": {
        "provider": "local",
        "default": "local",
        "name": "local-model",
        "endpoint": "http://127.0.0.1:1234/v1",
        "api_key_env": "OPENAI_API_KEY",
    },
    "llamacpp": {
        "provider": "llamacpp",
        "default": "llamacpp",
        "name": "local-model",
        "endpoint": "http://127.0.0.1:8080/v1",
        "api_key_env": "OPENAI_API_KEY",
    },
    "vllm": {
        "provider": "vllm",
        "default": "vllm",
        "name": "local-model",
        "endpoint": "http://127.0.0.1:8000/v1",
        "api_key_env": "OPENAI_API_KEY",
    },
}


class RunBody(BaseModel):
    task: str
    workspace: str | None = None
    session_id: str | None = None
    model: str | None = None
    purpose: str = "coder"


class ModelTestBody(BaseModel):
    provider: str = "mock"
    name: str = ""
    endpoint: str = ""
    api_key_env: str = ""
    context_limit: int = 128000


class ModelSelectBody(BaseModel):
    id: str
    name: str = ""
    provider: str = ""
    endpoint: str = ""


class TrustBody(BaseModel):
    path: str


class ExecBody(BaseModel):
    command: str
    timeout: int = 60


class HunkBody(BaseModel):
    path: str
    hunk: dict[str, Any]
    action: str = "reject"  # "reject" (revert in worktree) | "accept" (stage hunk)


class SessionBody(BaseModel):
    workspace: str
    title: str = ""


class ConfigPatch(BaseModel):
    values: dict[str, Any] = Field(default_factory=dict)
    api_key: str = ""
    api_key_env: str = ""


class OnboardBody(BaseModel):
    workspace: str
    provider: str = "mock"
    model: str = ""
    name: str = ""
    endpoint: str = ""
    api_key: str = ""
    api_key_env: str = ""
    permission_level: str = "workspace"
    theme: str = "light"
    ability: str = "none"
    network: bool = False


class ApprovalBody(BaseModel):
    decision: str


class SkillsBody(BaseModel):
    content: str = ""
    name: str = ""


class AttachBody(BaseModel):
    path: str = ""
    text: str = ""
    filename: str = ""


class GitCommitBody(BaseModel):
    message: str
    paths: list[str] = Field(default_factory=list)


class CommandRunBody(BaseModel):
    name: str
    args: str = ""
    session_id: str | None = None


class ProjectBody(BaseModel):
    path: str


def create_app(store: Store | None = None, default_workspace: Path | None = None, detect: bool = True) -> FastAPI:
    load_secrets()
    store = store or Store()
    bus = EventBus()
    approvals = ApprovalHub()
    jobs = JobManager(store, bus, approvals)
    registry = ModelRegistry()
    for info in registry.list_models():
        store.upsert_model(info.id, info.name, info.provider, info.endpoint, info.context_limit, info.metadata)
    detect_cache: dict[str, Any] = {"at": 0.0, "providers": []}

    def detected(force: bool = False) -> list:
        if force or time.time() - detect_cache["at"] > 10:
            detect_cache["providers"] = detect_providers()
            detect_cache["at"] = time.time()
            for entry in registry.refresh_detected():
                store.upsert_model(entry.id, entry.name, entry.provider, entry.endpoint, entry.context_limit, entry.metadata)
        return detect_cache["providers"]

    if detect:
        detected(force=True)

    app = FastAPI(title="ShadowCode", version=__version__)
    app.add_middleware(
        CORSMiddleware,
        allow_origins=["*"],
        allow_methods=["*"],
        allow_headers=["*"],
    )
    workspace = default_workspace or last_workspace()
    runtime: dict[str, Any] = {
        "store": store,
        "bus": bus,
        "workspace": workspace,
        "jobs": jobs,
        "approvals": approvals,
    }

    def emit_approval(item) -> None:
        store.add_event("approval.requested", item.to_dict(), session_id=item.payload.get("session_id"), task_id=item.payload.get("task_id"))

    approvals.on_request = emit_approval

    @app.get("/api/health")
    def health() -> dict[str, Any]:
        cfg = load_config(runtime.get("workspace"))
        payload = collect_health(cfg, runtime.get("workspace"))
        payload["ok"] = True
        return payload

    @app.get("/api/onboarding")
    def get_onboarding() -> dict[str, Any]:
        cfg = ensure_user_config()
        suggested = runtime.get("workspace") or last_workspace() or Path.home()
        found = detected()
        running = {item.provider: item for item in found if item.running}
        default_provider = cfg.model.provider if cfg.model.provider != "mock" else ("ollama" if "ollama" in running else "mock")
        return {
            "completed": cfg.onboarding.completed,
            "suggested_workspace": str(suggested),
            "providers": [
                {"id": key, **value, "needs_key": key not in {"mock", "ollama", "local", "llamacpp", "vllm"}}
                for key, value in PROVIDER_PRESETS.items()
            ],
            "detected": [item.model_dump() for item in found],
            "levels": ["read_only", "workspace", "elevated"],
            "defaults": {
                "provider": default_provider,
                "permission_level": "workspace",
                "theme": "light",
                "ability": "none",
            },
        }

    @app.post("/api/onboarding")
    def post_onboarding(body: OnboardBody) -> dict[str, Any]:
        workspace = Path(body.workspace).expanduser().resolve()
        if not workspace.is_dir():
            raise HTTPException(400, friendly_error("workspace not found"))
        preset = PROVIDER_PRESETS.get(body.provider, PROVIDER_PRESETS["mock"])
        key_env = body.api_key_env or preset["api_key_env"]
        if body.api_key.strip():
            try:
                set_secret(key_env, body.api_key.strip())
            except ValueError as exc:
                raise HTTPException(400, friendly_error(exc)) from exc
        try:
            level = PermissionLevel(body.permission_level)
        except ValueError as exc:
            raise HTTPException(400, "permission_level must be read_only, workspace, or elevated") from exc
        cfg = ensure_user_config()
        cfg.model.provider = body.provider if body.provider in PROVIDER_PRESETS else preset["provider"]
        cfg.model.default = body.model or preset["default"]
        cfg.model.name = body.name or preset["name"]
        cfg.model.endpoint = body.endpoint or preset["endpoint"]
        # When a local server is detected and no explicit model was chosen, prefer a real installed model.
        if not body.name and cfg.model.provider in {"ollama", "local", "llamacpp", "vllm"}:
            for item in detected():
                if item.provider == cfg.model.provider and item.running and item.models:
                    cfg.model.name = item.models[0].id
                    cfg.model.default = item.models[0].id
                    break
        cfg.model.api_key_env = key_env
        cfg.permissions.level = level
        cfg.permissions.network = body.network
        cfg.ui.theme = body.theme or "light"
        cfg.ui.ability = body.ability or "none"
        cfg.onboarding.completed = True
        cfg.onboarding.workspace = str(workspace)
        if str(workspace) not in cfg.trusted_workspaces:
            cfg.trusted_workspaces.append(str(workspace))
        save_config(cfg)
        runtime["workspace"] = workspace
        remember_workspace(workspace)
        store.touch_project(workspace)
        sid = store.create_session(str(workspace), cfg.model.default, title="Welcome")
        return {"ok": True, "workspace": str(workspace), "session_id": sid, "config": _public_config(cfg)}

    @app.get("/api/config")
    def get_config() -> dict[str, Any]:
        cfg = load_config(runtime.get("workspace"))
        return _public_config(cfg)

    @app.put("/api/config")
    def put_config(body: ConfigPatch) -> dict[str, Any]:
        values = dict(body.values)
        key_env = body.api_key_env or str((values.get("model") or {}).get("api_key_env") or "")
        if body.api_key.strip():
            env_name = key_env or load_config().model.api_key_env
            try:
                set_secret(env_name, body.api_key.strip())
            except ValueError as exc:
                raise HTTPException(400, friendly_error(exc)) from exc
        values.pop("api_key", None)
        cfg = apply_config_patch(values)
        return _public_config(cfg)

    @app.post("/api/config/set")
    def config_set(key: str, value: str) -> dict[str, Any]:
        return _public_config(set_config_value(key, value))

    @app.get("/api/models")
    def models(refresh: bool = False) -> dict[str, Any]:
        found = detected(force=refresh)
        running = {item.provider for item in found if item.running}
        out = []
        for m in registry.list_models():
            data = m.model_dump()
            data["detected"] = bool(m.metadata.get("detected")) or m.provider in running
            out.append(data)
        return {"models": out}

    @app.get("/api/providers/detect")
    def providers_detect(refresh: bool = False) -> dict[str, Any]:
        found = detected(force=refresh)
        return {"providers": [item.model_dump() for item in found]}

    @app.post("/api/models/test")
    def models_test(body: ModelTestBody) -> dict[str, Any]:
        preset = PROVIDER_PRESETS.get(body.provider, {})
        provider = body.provider or preset.get("provider", "mock")
        name = body.name or preset.get("name", "")
        endpoint = body.endpoint or preset.get("endpoint", "")
        api_key_env = body.api_key_env or preset.get("api_key_env", "OPENAI_API_KEY")
        if provider in {"ollama", "local", "llamacpp", "vllm"} and not name:
            for item in detected():
                if item.provider == provider and item.running and item.models:
                    name = item.models[0].id
                    break
        result = probe_provider(
            provider,
            name=name,
            endpoint=endpoint,
            api_key_env=api_key_env,
            context_limit=body.context_limit,
        )
        return result

    @app.post("/api/models/select")
    def models_select(body: ModelSelectBody) -> dict[str, Any]:
        info = registry.get(body.id)
        if info is None:
            detected(force=True)
            info = registry.get(body.id)
        if info is None:
            # Free-text model: the user typed an id that is neither builtin nor
            # detected. Accept it as a custom entry only when a provider is
            # explicitly supplied (the UI/CLI always sends one for free-text).
            if not body.provider:
                raise HTTPException(404, friendly_http(404, f"unknown model: {body.id}"))
            info = registry.register_custom(
                body.id,
                body.provider.lower(),
                name=body.name or body.id,
                endpoint=body.endpoint or "",
                api_key_env="",
                context_limit=0,
            )
        cfg = ensure_user_config()
        cfg.model.default = body.id
        cfg.model.provider = (info.provider if info else body.provider) or cfg.model.provider
        cfg.model.endpoint = body.endpoint or (info.endpoint if info else "") or cfg.model.endpoint
        cfg.model.name = body.name or (str(info.metadata.get("model") or info.id) if info else body.id)
        if info and info.metadata.get("api_key_env"):
            cfg.model.api_key_env = str(info.metadata["api_key_env"])
        if info and info.context_limit:
            cfg.model.context_limit = info.context_limit
        save_config(cfg)
        return _public_config(cfg)

    @app.post("/api/models/register")
    def models_register(body: ModelSelectBody) -> dict[str, Any]:
        """Register a free-text custom model for any provider and select it."""
        provider = (body.provider or "openai_compatible").lower()
        info = registry.register_custom(
            body.id,
            provider,
            name=body.name or body.id,
            endpoint=body.endpoint or "",
            api_key_env="",
            context_limit=0,
        )
        store.upsert_model(info.id, info.name, info.provider, info.endpoint, info.context_limit, info.metadata)
        cfg = ensure_user_config()
        cfg.model.default = info.id
        cfg.model.provider = info.provider
        cfg.model.endpoint = body.endpoint or cfg.model.endpoint
        cfg.model.name = body.name or info.id
        save_config(cfg)
        return _public_config(cfg)

    @app.get("/api/projects")
    def projects() -> dict[str, Any]:
        return {"projects": store.list_projects()}

    @app.post("/api/projects")
    def open_project(body: ProjectBody) -> dict[str, Any]:
        workspace = Path(body.path).expanduser().resolve()
        if not workspace.is_dir():
            raise HTTPException(400, friendly_error("workspace not found"))
        cfg = ensure_user_config()
        current = runtime.get("workspace")
        already = str(workspace) in cfg.trusted_workspaces or (current is not None and Path(current) == workspace)
        if not already:
            # First open of a new folder: the UI must show the trust dialog.
            return {
                "needs_trust": True,
                "path": str(workspace),
                "name": workspace.name,
                "permissions": cfg.permissions.model_dump(mode="json"),
            }
        runtime["workspace"] = workspace
        remember_workspace(workspace)
        pid = store.touch_project(workspace)
        sid = store.create_session(str(workspace), load_config(workspace).model.default, title=workspace.name)
        return {"id": pid, "path": str(workspace), "session_id": sid, "needs_trust": False}

    @app.post("/api/projects/trust")
    def trust_project(body: TrustBody) -> dict[str, Any]:
        workspace = Path(body.path).expanduser().resolve()
        if not workspace.is_dir():
            raise HTTPException(400, friendly_error("workspace not found"))
        cfg = ensure_user_config()
        if str(workspace) not in cfg.trusted_workspaces:
            cfg.trusted_workspaces.append(str(workspace))
            save_config(cfg)
        runtime["workspace"] = workspace
        remember_workspace(workspace)
        pid = store.touch_project(workspace)
        sid = store.create_session(str(workspace), load_config(workspace).model.default, title=workspace.name)
        return {"ok": True, "id": pid, "path": str(workspace), "session_id": sid, "trusted": cfg.trusted_workspaces}

    @app.post("/api/sessions")
    def create_session(body: SessionBody) -> dict[str, Any]:
        workspace = Path(body.workspace).expanduser().resolve()
        if not workspace.is_dir():
            raise HTTPException(400, friendly_error("workspace not found"))
        runtime["workspace"] = workspace
        remember_workspace(workspace)
        sid = store.create_session(str(workspace), load_config(workspace).model.default, title=body.title)
        store.touch_project(workspace)
        return {"id": sid, "workspace": str(workspace)}

    @app.get("/api/sessions")
    def sessions() -> dict[str, Any]:
        return {"sessions": store.list_sessions()}

    @app.get("/api/sessions/{session_id}")
    def session(session_id: str) -> dict[str, Any]:
        row = store.get_session(session_id)
        if not row:
            raise HTTPException(404, friendly_http(404, "session not found"))
        row["tasks"] = store.list_tasks(session_id)
        row["events"] = store.list_events(session_id=session_id, limit=400)
        return row

    @app.get("/api/sessions/{session_id}/export")
    def session_export(session_id: str, format: str = "md"):
        try:
            body, media = export_session(store, session_id, fmt=format)
        except KeyError as exc:
            raise HTTPException(404, friendly_http(404, "session not found")) from exc
        filename = f"shadow-session-{session_id[:8]}.{ 'json' if format == 'json' else 'md'}"
        return PlainTextResponse(body, media_type=media, headers={"Content-Disposition": f'attachment; filename="{filename}"'})

    @app.post("/api/sessions/{session_id}/branch")
    def session_branch(session_id: str, body: SessionBody | None = None) -> dict[str, Any]:
        title = (body.title if body else "") or ""
        try:
            new_id = store.branch_session(session_id, title)
        except KeyError as exc:
            raise HTTPException(404, friendly_http(404, "session not found")) from exc
        return {"id": new_id, "parent_id": session_id}

    @app.get("/api/sessions/{session_id}/cost")
    def session_cost(session_id: str) -> dict[str, Any]:
        if not store.get_session(session_id):
            raise HTTPException(404, friendly_http(404, "session not found"))
        return store.session_cost(session_id)

    @app.get("/api/sessions/{session_id}/pins")
    def session_pins(session_id: str) -> dict[str, Any]:
        if not store.get_session(session_id):
            raise HTTPException(404, friendly_http(404, "session not found"))
        return {"pins": store.list_pins(session_id)}

    @app.post("/api/sessions/{session_id}/pins")
    def session_add_pin(session_id: str, body: SkillsBody) -> dict[str, Any]:
        if not store.get_session(session_id):
            raise HTTPException(404, friendly_http(404, "session not found"))
        pid = store.add_pin(session_id, body.name or "pin", body.content)
        return {"ok": True, "id": pid}

    @app.delete("/api/sessions/{session_id}/pins/{pin_id}")
    def session_delete_pin(session_id: str, pin_id: int) -> dict[str, Any]:
        store.delete_pin(int(pin_id))
        return {"ok": True}

    @app.post("/api/sessions/{session_id}/run")
    def run_session(session_id: str, body: RunBody) -> dict[str, Any]:
        row = store.get_session(session_id)
        if not row:
            raise HTTPException(404, friendly_http(404, "session not found"))
        workspace = Path(body.workspace or row["workspace"])
        runtime["workspace"] = workspace
        runner = AgentRunner(
            workspace,
            store=store,
            events=bus,
            session_id=session_id,
            approval_hub=approvals,
            model_override=body.model,
            purpose=body.purpose,
        )
        result = runner.run(body.task)
        return result.model_dump(mode="json")

    @app.post("/api/run")
    def run_direct(body: RunBody) -> dict[str, Any]:
        workspace = Path(body.workspace or runtime.get("workspace") or Path.cwd()).resolve()
        if not workspace.is_dir():
            raise HTTPException(400, friendly_error("workspace not found"))
        runtime["workspace"] = workspace
        runner = AgentRunner(
            workspace,
            store=store,
            events=bus,
            session_id=body.session_id,
            approval_hub=approvals,
            model_override=body.model,
            purpose=body.purpose,
        )
        result = runner.run(body.task)
        return result.model_dump(mode="json")

    @app.post("/api/jobs")
    def start_job(body: RunBody) -> dict[str, Any]:
        workspace = Path(body.workspace or runtime.get("workspace") or Path.cwd()).resolve()
        if not workspace.is_dir():
            raise HTTPException(400, friendly_error("workspace not found"))
        runtime["workspace"] = workspace
        remember_workspace(workspace)
        store.touch_project(workspace)
        job = jobs.start(workspace, body.task, session_id=body.session_id, model=body.model, purpose=body.purpose)
        return job.to_dict()

    @app.get("/api/jobs")
    def list_jobs() -> dict[str, Any]:
        return {"jobs": [job.to_dict() for job in jobs.list_active()]}

    @app.get("/api/jobs/current")
    def current_job(session_id: str | None = None) -> dict[str, Any]:
        job = jobs.current(session_id)
        return {"job": job.to_dict() if job else None}

    @app.get("/api/jobs/{job_id}")
    def get_job(job_id: str) -> dict[str, Any]:
        job = jobs.get(job_id)
        if not job:
            raise HTTPException(404, "job not found")
        return job.to_dict()

    @app.post("/api/jobs/{job_id}/cancel")
    def cancel_job(job_id: str) -> dict[str, Any]:
        try:
            return jobs.cancel(job_id).to_dict()
        except KeyError as exc:
            raise HTTPException(404, "job not found") from exc

    @app.post("/api/run/cancel")
    def cancel_current(session_id: str | None = None) -> dict[str, Any]:
        job = jobs.current(session_id)
        if not job:
            return {"ok": True, "stopped": []}
        jobs.cancel(job.id)
        return {"ok": True, "stopped": [job.id]}

    @app.get("/api/jobs/{job_id}/events")
    def job_events(job_id: str) -> StreamingResponse:
        def gen():
            seen = 0
            while True:
                job = jobs.get(job_id)
                if job is None:
                    yield f"data: {json.dumps({'type': 'error', 'payload': {'error': 'unknown job'}})}\n\n"
                    return
                rows = store.list_events(session_id=job.session_id, limit=800)
                if len(rows) > seen:
                    for row in rows[seen:]:
                        yield f"data: {json.dumps(row, default=str)}\n\n"
                    seen = len(rows)
                if job.status in {"completed", "failed", "cancelled"} and len(rows) <= seen:
                    yield f"data: {json.dumps({'type': 'job.done', 'payload': job.to_dict()})}\n\n"
                    return
                yield ": keepalive\n\n"
                time.sleep(0.25)

        return StreamingResponse(gen(), media_type="text/event-stream")

    @app.get("/api/events")
    def events(session_id: str | None = None, limit: int = 200) -> dict[str, Any]:
        return {"events": store.list_events(session_id=session_id, limit=limit)}

    @app.get("/api/sessions/{session_id}/events")
    def session_events(session_id: str) -> StreamingResponse:
        def gen():
            seen = 0
            idle = 0
            while True:
                rows = store.list_events(session_id=session_id, limit=800)
                if len(rows) > seen:
                    for row in rows[seen:]:
                        yield f"data: {json.dumps(row, default=str)}\n\n"
                    seen = len(rows)
                    idle = 0
                else:
                    yield ": keepalive\n\n"
                    idle += 1
                if idle > 80:
                    return
                time.sleep(0.4)

        return StreamingResponse(gen(), media_type="text/event-stream")

    @app.get("/api/approvals")
    def list_approvals(session_id: str | None = None) -> dict[str, Any]:
        return {"approvals": approvals.list_pending(session_id)}

    @app.post("/api/approvals/{approval_id}")
    def decide_approval(approval_id: str, body: ApprovalBody) -> dict[str, Any]:
        try:
            return approvals.decide(approval_id, body.decision)
        except KeyError as exc:
            raise HTTPException(404, "approval not found") from exc
        except ValueError as exc:
            raise HTTPException(400, str(exc)) from exc

    @app.get("/api/workspace/files")
    def workspace_files(path: str = ".") -> dict[str, Any]:
        workspace = _ws(runtime)
        sandbox = WorkspaceSandbox(workspace)
        try:
            root = sandbox.resolve(path)
        except SandboxError as exc:
            raise HTTPException(400, friendly_error(exc)) from exc
        entries = []
        if root.is_dir():
            for child in sorted(root.iterdir(), key=lambda p: (not p.is_dir(), p.name.lower())):
                if child.name in {".git", "__pycache__", "node_modules", ".venv"}:
                    continue
                entries.append({"name": child.name, "path": sandbox.relative(child), "type": "dir" if child.is_dir() else "file"})
        parent = ""
        if path not in {"", ".", workspace.name}:
            parent = str(Path(path).parent)
            if parent == ".":
                parent = "."
        return {"path": path, "parent": parent, "entries": entries, "workspace": str(workspace)}

    @app.get("/api/workspace/file")
    def workspace_file(path: str) -> dict[str, Any]:
        workspace = _ws(runtime)
        sandbox = WorkspaceSandbox(workspace)
        try:
            target = sandbox.resolve(path, must_exist=True)
        except SandboxError as exc:
            raise HTTPException(400, friendly_error(exc)) from exc
        if not target.is_file():
            raise HTTPException(400, "not a file")
        try:
            text = target.read_text(encoding="utf-8")
        except UnicodeDecodeError as exc:
            raise HTTPException(400, friendly_error("binary file")) from exc
        if len(text) > 200_000:
            text = text[:200_000] + "\n…[truncated]…"
        return {"path": path, "content": text}

    @app.get("/api/workspace/diff")
    def workspace_diff(path: str = "") -> dict[str, Any]:
        workspace = _ws(runtime)
        import subprocess

        args = ["git", "diff", "--", path] if path else ["git", "diff"]
        proc = subprocess.run(args, cwd=workspace, capture_output=True, text=True, check=False)
        staged = subprocess.run(
            ["git", "diff", "--cached", "--", path] if path else ["git", "diff", "--cached"],
            cwd=workspace,
            capture_output=True,
            text=True,
            check=False,
        )
        return {
            "path": path,
            "diff": proc.stdout,
            "staged": staged.stdout,
            "hunks": _parse_diff_hunks(proc.stdout or staged.stdout),
        }

    @app.get("/api/workspace/git")
    def workspace_git() -> dict[str, Any]:
        workspace = _ws(runtime)
        import subprocess

        def git(*args: str) -> str:
            proc = subprocess.run(["git", *args], cwd=workspace, capture_output=True, text=True, check=False)
            return proc.stdout or proc.stderr

        porcelain = git("status", "--porcelain=v1", "-b")
        return {
            "status": git("status", "-sb"),
            "porcelain": porcelain,
            "log": git("log", "-12", "--oneline", "--decorate"),
            "diff": git("diff", "--stat"),
            "files": _parse_porcelain(porcelain),
            "repo": (workspace / ".git").exists(),
        }

    @app.post("/api/workspace/git/add")
    def git_add(body: GitCommitBody) -> dict[str, Any]:
        workspace = _ws(runtime)
        import subprocess

        paths_ = body.paths or ["."]
        proc = subprocess.run(["git", "add", "--", *paths_], cwd=workspace, capture_output=True, text=True, check=False)
        if proc.returncode != 0:
            raise HTTPException(400, friendly_error(proc.stderr or "git add failed"))
        return {"ok": True, "stdout": proc.stdout}

    @app.post("/api/workspace/git/commit")
    def git_commit(body: GitCommitBody) -> dict[str, Any]:
        workspace = _ws(runtime)
        import subprocess

        if body.paths:
            subprocess.run(["git", "add", "--", *body.paths], cwd=workspace, capture_output=True, text=True, check=False)
        proc = subprocess.run(["git", "commit", "-m", body.message], cwd=workspace, capture_output=True, text=True, check=False)
        if proc.returncode != 0:
            raise HTTPException(400, friendly_error(proc.stderr or "git commit failed"))
        return {"ok": True, "stdout": proc.stdout}

    @app.get("/api/workspace/status")
    def workspace_status() -> dict[str, Any]:
        workspace = _ws(runtime)
        cfg = load_config(workspace)
        return {
            "workspace": str(workspace),
            "model": cfg.model.model_dump(),
            "permissions": cfg.permissions.model_dump(),
            "session": store.list_sessions(limit=1),
            "onboarding": cfg.onboarding.model_dump(),
            "usage": (store.list_sessions(limit=1) or [{}])[0].get("usage_json"),
        }

    @app.get("/api/workspace/instructions")
    def get_instructions() -> dict[str, Any]:
        workspace = _ws(runtime)
        path = workspace / ".shadow" / "instructions.md"
        return {"path": ".shadow/instructions.md", "content": path.read_text(encoding="utf-8") if path.is_file() else "", "exists": path.is_file()}

    @app.put("/api/workspace/instructions")
    def put_instructions(body: SkillsBody) -> dict[str, Any]:
        workspace = _ws(runtime)
        path = workspace / ".shadow" / "instructions.md"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(body.content, encoding="utf-8")
        return {"ok": True, "path": ".shadow/instructions.md"}

    @app.get("/api/workspace/skills")
    def get_skills() -> dict[str, Any]:
        workspace = _ws(runtime)
        skill_dir = workspace / ".shadow" / "skills"
        items = []
        if skill_dir.is_dir():
            for path in sorted(skill_dir.glob("*.md")):
                items.append({"name": path.stem, "path": f".shadow/skills/{path.name}", "content": path.read_text(encoding="utf-8")})
        return {"skills": items}

    @app.put("/api/workspace/skills")
    def put_skill(body: SkillsBody) -> dict[str, Any]:
        name = (body.name or "skill").strip().replace(" ", "-")
        if not name or "/" in name or name.startswith("."):
            raise HTTPException(400, "invalid skill name")
        workspace = _ws(runtime)
        path = workspace / ".shadow" / "skills" / f"{name}.md"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(body.content, encoding="utf-8")
        return {"ok": True, "name": name, "path": f".shadow/skills/{name}.md"}

    @app.post("/api/workspace/attach")
    def attach_file(body: AttachBody) -> dict[str, Any]:
        workspace = _ws(runtime)
        sandbox = WorkspaceSandbox(workspace)
        rel = body.path.strip()
        if not rel:
            name = body.filename.strip() or f"note-{int(time.time())}.md"
            rel = f".shadow/attachments/{name}"
        try:
            target = sandbox.resolve(rel)
        except SandboxError as exc:
            raise HTTPException(400, friendly_error(exc)) from exc
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(body.text, encoding="utf-8")
        return {"ok": True, "path": sandbox.relative(target)}

    @app.post("/api/workspace/exec")
    def workspace_exec(body: ExecBody) -> dict[str, Any]:
        """Run one shell command in the workspace (used by click-to-rerun in the UI)."""
        workspace = _ws(runtime)
        cfg = load_config(workspace)
        if cfg.permissions.level == PermissionLevel.READ_ONLY:
            raise HTTPException(403, friendly_error("exec is disabled in read-only mode"))
        from shadow_agent.models.types import ToolCall
        from shadow_agent.permissions import PermissionGate
        from shadow_agent.tools.terminal import exec_command

        gate = PermissionGate(
            cfg.permissions.level,
            require_approval_for_dangerous=cfg.permissions.require_approval_for_dangerous,
            network=cfg.permissions.network,
            allow_root=cfg.permissions.allow_root,
        )
        sandbox = WorkspaceSandbox(workspace)
        call = ToolCall(id="ui-exec", tool_name="exec", arguments={"command": body.command, "timeout": body.timeout})
        result = exec_command(sandbox, gate, call, default_timeout=min(body.timeout, 300))
        if not result.success and result.metadata.get("needs_approval"):
            raise HTTPException(403, friendly_error(result.error or "command needs approval; run it as an agent task instead"))
        try:
            payload = json.loads(result.output) if result.output else {}
        except json.JSONDecodeError:
            payload = {"stdout": result.output}
        store.add_event("tool.completed", {"tool": "exec", "success": result.success, "output_preview": (result.output or "")[:500], "source": "ui-rerun"})
        return {
            "ok": result.success,
            "command": body.command,
            "stdout": payload.get("stdout", ""),
            "stderr": payload.get("stderr", ""),
            "exit_code": payload.get("exit_code", -1),
            "error": result.error,
        }

    @app.post("/api/workspace/diff/hunk")
    def diff_hunk(body: HunkBody) -> dict[str, Any]:
        """Accept (stage) or reject (revert) one diff hunk via git apply."""
        import subprocess

        workspace = _ws(runtime)
        sandbox = WorkspaceSandbox(workspace)
        try:
            target = sandbox.resolve(body.path, must_exist=True)
        except SandboxError as exc:
            raise HTTPException(400, friendly_error(exc)) from exc
        if not target.is_file():
            raise HTTPException(400, "not a file")
        patch = _build_hunk_patch(body.path, body.hunk)
        if not patch:
            raise HTTPException(400, "could not rebuild hunk patch")
        args = ["git", "apply", "--recount", "--unidiff-zero", "-"]
        if body.action == "accept":
            args.insert(2, "--cached")
        elif body.action == "reject":
            args.insert(2, "-R")
        else:
            raise HTTPException(400, "action must be accept or reject")
        proc = subprocess.run(args, cwd=workspace, input=patch, capture_output=True, text=True, check=False)
        if proc.returncode != 0:
            raise HTTPException(400, friendly_error((proc.stderr or "git apply failed").strip()))
        return {"ok": True, "action": body.action, "path": body.path}

    @app.get("/api/doctor")
    def doctor() -> dict[str, Any]:
        cfg = load_config(runtime.get("workspace"))
        return doctor_report(cfg, runtime.get("workspace"))

    @app.get("/api/commands")
    def list_commands() -> dict[str, Any]:
        """List all slash commands (builtins + project .shadow/commands/*.md)."""
        workspace = _ws(runtime)
        reg = CommandRegistry(workspace)
        cmds = [
            {
                "name": c.name,
                "description": c.description,
                "arg_spec": c.arg_spec,
                "alias": c.alias,
                "source": c.source,
            }
            for c in reg.list()
        ]
        return {"commands": cmds}

    @app.post("/api/commands/run")
    def run_command(body: CommandRunBody) -> dict[str, Any]:
        """Run a builtin slash command and return its structured result.

        Custom (project) commands are NOT run here — they expand to a prompt
        and should be sent to /api/jobs as a task. This endpoint only handles
        builtins, returning a Codex-style card the UI can render.
        """
        workspace = _ws(runtime)
        reg = CommandRegistry(workspace)
        cmd = reg.get(body.name)
        if cmd is None:
            raise HTTPException(404, friendly_http(404, f"unknown command: /{body.name}"))
        if not reg.is_builtin(body.name):
            raise HTTPException(400, "custom commands must be run as tasks via /api/jobs")
        cfg = load_config(workspace)
        ctx = CommandContext(
            workspace=workspace,
            config=cfg,
            store=store,
            registry=registry,
            commands=reg,
            approvals=approvals,
            session_id=body.session_id or "",
            extra={"plan": "", "todos": []},
        )
        result = dispatch_command(body.args, ctx, body.name)
        return result.to_dict()

    @app.get("/api/checkpoints")
    def checkpoints() -> dict[str, Any]:
        pointer = last_checkpoint(_ws(runtime))
        return {"last": pointer}

    @app.post("/api/checkpoints/undo")
    def undo_checkpoint() -> dict[str, Any]:
        result = restore_last(_ws(runtime))
        if not result.get("ok"):
            raise HTTPException(400, result.get("error") or "undo failed")
        return result

    if UI_DIST.is_dir():
        app.mount("/assets", StaticFiles(directory=UI_DIST / "assets"), name="assets")

        @app.get("/icon.svg")
        def icon() -> FileResponse:
            return FileResponse(UI_DIST / "icon.svg")

        @app.get("/")
        def index() -> FileResponse:
            return FileResponse(UI_DIST / "index.html")

    return app


def _public_config(cfg: AppConfig) -> dict[str, Any]:
    data = cfg.model_dump(mode="json")
    data["secrets"] = {
        "api_key_env": cfg.model.api_key_env,
        "key_present": has_secret(cfg.model.api_key_env) if cfg.model.provider != "mock" else True,
    }
    return data


def _ws(runtime: dict[str, Any]) -> Path:
    workspace = runtime.get("workspace")
    if workspace is None:
        return Path.cwd()
    return Path(workspace)


def _parse_porcelain(text: str) -> list[dict[str, str]]:
    files: list[dict[str, str]] = []
    for line in text.splitlines():
        if line.startswith("##") or len(line) < 4:
            continue
        files.append({"index": line[0], "work": line[1], "path": line[3:], "label": line[:2].strip() or "M"})
    return files


def _build_hunk_patch(path: str, hunk: dict[str, Any]) -> str:
    """Rebuild an apply-able unified diff for a single parsed hunk."""
    header = str(hunk.get("header") or "")
    lines = hunk.get("lines") or []
    if not header.startswith("@@") or not lines:
        return ""
    body = []
    for line in lines:
        kind = line.get("kind")
        text = str(line.get("text") or "")
        prefix = "+" if kind == "add" else "-" if kind == "del" else " "
        body.append(prefix + text)
    return f"--- a/{path}\n+++ b/{path}\n{header}\n" + "\n".join(body) + "\n"


def _parse_diff_hunks(diff: str) -> list[dict[str, Any]]:
    hunks: list[dict[str, Any]] = []
    current: dict[str, Any] | None = None
    for line in diff.splitlines():
        if line.startswith("@@"):
            if current:
                hunks.append(current)
            current = {"header": line, "lines": []}
        elif current is not None:
            kind = "ctx"
            if line.startswith("+") and not line.startswith("+++"):
                kind = "add"
            elif line.startswith("-") and not line.startswith("---"):
                kind = "del"
            current["lines"].append({"kind": kind, "text": line[1:] if line[:1] in "+- " else line})
    if current:
        hunks.append(current)
    return hunks


def serve(host: str = "127.0.0.1", port: int = 7430, workspace: Path | None = None) -> None:
    import uvicorn

    load_secrets()
    app = create_app(default_workspace=workspace)
    uvicorn.run(app, host=host, port=port, log_level="info")
