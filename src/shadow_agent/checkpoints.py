"""Snapshot agent file mutations so the last task can be undone."""

from __future__ import annotations

import json
import time
from pathlib import Path
from typing import Any

from shadow_agent import paths
from shadow_agent.tools.sandbox import SandboxError, WorkspaceSandbox

MUTATING_TOOLS = {"write_file", "edit_file", "delete_file", "move_file", "apply_patch"}


class CheckpointStore:
    def __init__(self, workspace: Path, task_id: str) -> None:
        self.workspace = Path(workspace).resolve()
        self.task_id = task_id
        self.root = paths.task_dir(task_id) / "checkpoint"
        self.files = self.root / "files"
        self.manifest_path = self.root / "manifest.json"
        self.root.mkdir(parents=True, exist_ok=True)
        self.files.mkdir(parents=True, exist_ok=True)
        self.entries: list[dict[str, Any]] = _read_json(self.manifest_path, [])

    def record_call(self, tool_name: str, arguments: dict[str, Any]) -> None:
        if tool_name not in MUTATING_TOOLS:
            return
        sandbox = WorkspaceSandbox(self.workspace)
        if tool_name == "move_file":
            self._record_path(sandbox, str(arguments.get("src") or ""))
            return
        rel = str(arguments.get("path") or "")
        if rel:
            self._record_path(sandbox, rel)

    def _record_path(self, sandbox: WorkspaceSandbox, rel: str) -> None:
        if not rel or any(item.get("path") == rel for item in self.entries):
            return
        try:
            path = sandbox.resolve(rel)
        except SandboxError:
            return
        existed = path.is_file()
        entry = {"path": rel, "existed": existed, "ts": time.time()}
        if existed:
            dest = self.files / rel
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_bytes(path.read_bytes())
        self.entries.append(entry)
        self._save()

    def restore(self) -> list[str]:
        restored: list[str] = []
        sandbox = WorkspaceSandbox(self.workspace)
        for entry in reversed(self.entries):
            rel = str(entry.get("path") or "")
            if not rel:
                continue
            try:
                target = sandbox.resolve(rel)
            except SandboxError:
                continue
            backup = self.files / rel
            if entry.get("existed") and backup.is_file():
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(backup.read_bytes())
                restored.append(rel)
            elif not entry.get("existed") and target.is_file():
                target.unlink()
                restored.append(rel)
        self._mark_restored()
        return restored

    def summary(self) -> dict[str, Any]:
        return {
            "task_id": self.task_id,
            "workspace": str(self.workspace),
            "changes": len(self.entries),
            "paths": [item.get("path") for item in self.entries],
            "restored": bool((self.root / "restored").is_file()),
        }

    def _save(self) -> None:
        self.manifest_path.write_text(json.dumps(self.entries, indent=2), encoding="utf-8")
        pointer = {
            "task_id": self.task_id,
            "workspace": str(self.workspace),
            "ts": time.time(),
            "changes": len(self.entries),
        }
        (paths.checkpoints_root() / "last.json").write_text(json.dumps(pointer), encoding="utf-8")

    def _mark_restored(self) -> None:
        (self.root / "restored").write_text(str(time.time()), encoding="utf-8")


def last_checkpoint(workspace: Path | None = None) -> dict[str, Any] | None:
    pointer = paths.checkpoints_root() / "last.json"
    if not pointer.is_file():
        return None
    try:
        data = json.loads(pointer.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return None
    if workspace is not None and Path(data.get("workspace") or "") != Path(workspace).resolve():
        return None
    return data


def restore_last(workspace: Path) -> dict[str, Any]:
    pointer = last_checkpoint(workspace)
    if not pointer:
        return {"ok": False, "restored": [], "error": "No checkpoint exists for this project yet."}
    store = CheckpointStore(workspace, str(pointer["task_id"]))
    if not store.entries:
        return {"ok": False, "restored": [], "error": "The last checkpoint is empty."}
    restored = store.restore()
    return {"ok": True, "restored": restored, "task_id": store.task_id}


def _read_json(path: Path, default: list) -> list:
    if not path.is_file():
        return list(default)
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return list(default)
    return data if isinstance(data, list) else list(default)
