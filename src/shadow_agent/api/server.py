"""Agent API used by both the CLI and the desktop UI."""

from __future__ import annotations

import json
import threading
from pathlib import Path
from typing import Any

from fastapi import FastAPI, HTTPException
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import FileResponse, StreamingResponse
from fastapi.staticfiles import StaticFiles
from pydantic import BaseModel, Field

from shadow_agent import paths
from shadow_agent.agent.loop import AgentRunner
from shadow_agent.config import AppConfig, ensure_user_config, load_config, save_config, set_config_value
from shadow_agent.events import EventBus
from shadow_agent.models.registry import ModelRegistry
from shadow_agent.store import Store
from shadow_agent.tools.sandbox import SandboxError, WorkspaceSandbox

UI_DIST = Path(__file__).resolve().parents[3] / "ui" / "dist"


class RunBody(BaseModel):
    task: str
    workspace: str | None = None
    session_id: str | None = None


class SessionBody(BaseModel):
    workspace: str


class ConfigPatch(BaseModel):
    values: dict[str, Any] = Field(default_factory=dict)


def create_app(store: Store | None = None, default_workspace: Path | None = None) -> FastAPI:
    store = store or Store()
    bus = EventBus()
    registry = ModelRegistry()
    for info in registry.list_models():
        store.upsert_model(info.id, info.name, info.provider, info.endpoint, info.context_limit, info.metadata)

    app = FastAPI(title="Shadow Agent", version="0.1.0")
    app.add_middleware(
        CORSMiddleware,
        allow_origins=["*"],
        allow_methods=["*"],
        allow_headers=["*"],
    )
    runtime: dict[str, Any] = {"store": store, "bus": bus, "workspace": default_workspace}

    @app.get("/api/health")
    def health() -> dict[str, Any]:
        return {"ok": True, "name": "shadow-agent", "workspace": str(runtime.get("workspace") or "")}

    @app.get("/api/config")
    def get_config() -> dict[str, Any]:
        cfg = load_config(runtime.get("workspace"))
        return cfg.model_dump(mode="json")

    @app.put("/api/config")
    def put_config(body: ConfigPatch) -> dict[str, Any]:
        cfg = ensure_user_config()
        raw = cfg.model_dump(mode="json")
        raw.update(body.values)
        cfg = AppConfig.model_validate(raw)
        save_config(cfg)
        return cfg.model_dump(mode="json")

    @app.post("/api/config/set")
    def config_set(key: str, value: str) -> dict[str, Any]:
        return set_config_value(key, value).model_dump(mode="json")

    @app.get("/api/models")
    def models() -> dict[str, Any]:
        return {"models": [m.model_dump() for m in registry.list_models()]}

    @app.get("/api/projects")
    def projects() -> dict[str, Any]:
        return {"projects": store.list_projects()}

    @app.post("/api/sessions")
    def create_session(body: SessionBody) -> dict[str, Any]:
        workspace = Path(body.workspace).expanduser().resolve()
        if not workspace.is_dir():
            raise HTTPException(400, "workspace not found")
        runtime["workspace"] = workspace
        sid = store.create_session(str(workspace))
        store.touch_project(workspace)
        return {"id": sid, "workspace": str(workspace)}

    @app.get("/api/sessions")
    def sessions() -> dict[str, Any]:
        return {"sessions": store.list_sessions()}

    @app.get("/api/sessions/{session_id}")
    def session(session_id: str) -> dict[str, Any]:
        row = store.get_session(session_id)
        if not row:
            raise HTTPException(404, "session not found")
        row["tasks"] = store.list_tasks(session_id)
        row["events"] = store.list_events(session_id=session_id, limit=200)
        return row

    @app.post("/api/sessions/{session_id}/run")
    def run_session(session_id: str, body: RunBody) -> dict[str, Any]:
        row = store.get_session(session_id)
        if not row:
            raise HTTPException(404, "session not found")
        workspace = Path(body.workspace or row["workspace"])
        runtime["workspace"] = workspace
        runner = AgentRunner(workspace, store=store, events=bus, session_id=session_id)
        result = runner.run(body.task)
        return result.model_dump(mode="json")

    @app.post("/api/run")
    def run_direct(body: RunBody) -> dict[str, Any]:
        workspace = Path(body.workspace or runtime.get("workspace") or Path.cwd()).resolve()
        if not workspace.is_dir():
            raise HTTPException(400, "workspace not found")
        runtime["workspace"] = workspace
        runner = AgentRunner(workspace, store=store, events=bus, session_id=body.session_id)
        result = runner.run(body.task)
        return result.model_dump(mode="json")

    @app.get("/api/events")
    def events(session_id: str | None = None, limit: int = 200) -> dict[str, Any]:
        return {"events": store.list_events(session_id=session_id, limit=limit)}

    @app.get("/api/sessions/{session_id}/events")
    def session_events(session_id: str) -> StreamingResponse:
        def gen():
            seen = 0
            while True:
                rows = store.list_events(session_id=session_id, limit=500)
                if len(rows) > seen:
                    for row in rows[seen:]:
                        yield f"data: {json.dumps(row, default=str)}\n\n"
                    seen = len(rows)
                else:
                    yield ": keepalive\n\n"
                import time

                time.sleep(0.4)

        return StreamingResponse(gen(), media_type="text/event-stream")

    @app.get("/api/workspace/files")
    def workspace_files(path: str = ".") -> dict[str, Any]:
        workspace = _ws(runtime)
        sandbox = WorkspaceSandbox(workspace)
        try:
            root = sandbox.resolve(path)
        except SandboxError as exc:
            raise HTTPException(400, str(exc)) from exc
        entries = []
        if root.is_dir():
            for child in sorted(root.iterdir()):
                if child.name in {".git", "__pycache__", "node_modules"}:
                    continue
                entries.append({"name": child.name, "path": sandbox.relative(child), "type": "dir" if child.is_dir() else "file"})
        return {"path": path, "entries": entries, "workspace": str(workspace)}

    @app.get("/api/workspace/file")
    def workspace_file(path: str) -> dict[str, Any]:
        workspace = _ws(runtime)
        sandbox = WorkspaceSandbox(workspace)
        try:
            target = sandbox.resolve(path, must_exist=True)
        except SandboxError as exc:
            raise HTTPException(400, str(exc)) from exc
        if not target.is_file():
            raise HTTPException(400, "not a file")
        try:
            text = target.read_text(encoding="utf-8")
        except UnicodeDecodeError as exc:
            raise HTTPException(400, "binary file") from exc
        if len(text) > 200_000:
            text = text[:200_000] + "\n…[truncated]…"
        return {"path": path, "content": text}

    @app.get("/api/workspace/git")
    def workspace_git() -> dict[str, Any]:
        workspace = _ws(runtime)
        import subprocess

        def git(*args: str) -> str:
            proc = subprocess.run(["git", *args], cwd=workspace, capture_output=True, text=True, check=False)
            return proc.stdout or proc.stderr

        return {
            "status": git("status", "-sb"),
            "log": git("log", "-8", "--oneline"),
            "diff": git("diff", "--stat"),
        }

    @app.get("/api/workspace/status")
    def workspace_status() -> dict[str, Any]:
        workspace = _ws(runtime)
        cfg = load_config(workspace)
        return {
            "workspace": str(workspace),
            "model": cfg.model.model_dump(),
            "permissions": cfg.permissions.model_dump(),
            "session": store.list_sessions(limit=1),
        }

    if UI_DIST.is_dir():
        app.mount("/assets", StaticFiles(directory=UI_DIST / "assets"), name="assets")

        @app.get("/icon.svg")
        def icon() -> FileResponse:
            return FileResponse(UI_DIST / "icon.svg")

        @app.get("/")
        def index() -> FileResponse:
            return FileResponse(UI_DIST / "index.html")

    return app


def _ws(runtime: dict[str, Any]) -> Path:
    workspace = runtime.get("workspace")
    if workspace is None:
        return Path.cwd()
    return Path(workspace)


def serve(host: str = "127.0.0.1", port: int = 7430, workspace: Path | None = None) -> None:
    import uvicorn

    app = create_app(default_workspace=workspace)
    uvicorn.run(app, host=host, port=port, log_level="info")
