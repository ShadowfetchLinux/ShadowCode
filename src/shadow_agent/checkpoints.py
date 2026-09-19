"""Snapshot agent file mutations so the last task can be undone.

Also supports *named* checkpoints: before a major change the agent (or user)
calls `checkpoint(name)` to snapshot the current state of touched files plus
the git HEAD; `/rollback <name>` restores it. Named checkpoints live in
`.shadow/checkpoints/<name>/` and are listed by `shadow checkpoints`.
"""

from __future__ import annotations

import json
import subprocess
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


# --- Named checkpoints (ShadowCode 0.8.0) --------------------------------------

NAME_RE = __import__("re").compile(r"^[A-Za-z0-9_.-]{1,64}$")


def named_dir(workspace: Path, name: str) -> Path:
    return Path(workspace).resolve() / ".shadow" / "checkpoints" / name


def create_named_checkpoint(workspace: Path, name: str, paths_to_snapshot: list[str] | None = None) -> dict[str, Any]:
    """Snapshot the current state of `paths_to_snapshot` (or all tracked files)
    plus the git HEAD, under `.shadow/checkpoints/<name>/`.

    Returns a manifest dict. Idempotent: re-creating a checkpoint overwrites it.
    """
    if not NAME_RE.match(name):
        return {"ok": False, "error": f"invalid checkpoint name: {name!r}"}
    root = named_dir(workspace, name)
    files_dir = root / "files"
    files_dir.mkdir(parents=True, exist_ok=True)
    sandbox = WorkspaceSandbox(Path(workspace).resolve())
    # Default to git-tracked files; fall back to a directory walk.
    target_paths: list[str] = []
    if paths_to_snapshot:
        target_paths = paths_to_snapshot
    elif (Path(workspace) / ".git").is_dir():
        proc = subprocess.run(
            ["git", "ls-files"], cwd=workspace, capture_output=True, text=True, check=False
        )
        if proc.returncode == 0:
            target_paths = [line for line in proc.stdout.splitlines() if line]
    if not target_paths:
        # Walk the tree, skipping obvious noise.
        skip = {".git", ".venv", "venv", "__pycache__", "node_modules", ".shadow"}
        for p in Path(workspace).iterdir():
            if p.name in skip or p.name.startswith("."):
                continue
            if p.is_file():
                target_paths.append(p.name)
            elif p.is_dir():
                for f in p.rglob("*"):
                    if f.is_file() and ".venv" not in str(f) and "node_modules" not in str(f):
                        target_paths.append(str(f.relative_to(workspace)))
    manifest: list[dict[str, Any]] = []
    for rel in target_paths:
        try:
            src = sandbox.resolve(rel)
        except SandboxError:
            continue
        if not src.is_file():
            continue
        dest = files_dir / rel
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(src.read_bytes())
        manifest.append({"path": rel, "size": src.stat().st_size, "ts": time.time()})
    git_head = ""
    if (Path(workspace) / ".git").is_dir():
        proc = subprocess.run(
            ["git", "rev-parse", "HEAD"], cwd=workspace, capture_output=True, text=True, check=False
        )
        git_head = proc.stdout.strip()
    (root / "manifest.json").write_text(
        json.dumps({"name": name, "git_head": git_head, "files": manifest, "ts": time.time()}, indent=2),
        encoding="utf-8",
    )
    return {"ok": True, "name": name, "git_head": git_head, "files": len(manifest), "path": str(root)}


def rollback_named(workspace: Path, name: str) -> dict[str, Any]:
    """Restore files from a named checkpoint. Does NOT move git HEAD (the user
    can `git reset` separately if they want); only file contents are restored."""
    if not NAME_RE.match(name):
        return {"ok": False, "error": f"invalid checkpoint name: {name!r}"}
    root = named_dir(workspace, name)
    manifest_path = root / "manifest.json"
    if not manifest_path.is_file():
        return {"ok": False, "error": f"no checkpoint named {name!r}"}
    try:
        data = json.loads(manifest_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return {"ok": False, "error": "checkpoint manifest is corrupt"}
    sandbox = WorkspaceSandbox(Path(workspace).resolve())
    restored: list[str] = []
    for entry in data.get("files", []):
        rel = str(entry.get("path") or "")
        if not rel:
            continue
        src = root / "files" / rel
        if not src.is_file():
            continue
        try:
            dest = sandbox.resolve(rel)
        except SandboxError:
            continue
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(src.read_bytes())
        restored.append(rel)
    return {
        "ok": True,
        "name": name,
        "git_head": data.get("git_head", ""),
        "restored": restored,
        "files": len(restored),
    }


def list_named_checkpoints(workspace: Path) -> list[dict[str, Any]]:
    root = Path(workspace).resolve() / ".shadow" / "checkpoints"
    if not root.is_dir():
        return []
    out: list[dict[str, Any]] = []
    for d in sorted(root.iterdir()):
        if not d.is_dir():
            continue
        manifest = d / "manifest.json"
        if not manifest.is_file():
            continue
        try:
            data = json.loads(manifest.read_text(encoding="utf-8"))
        except json.JSONDecodeError:
            continue
        out.append({
            "name": data.get("name") or d.name,
            "ts": data.get("ts"),
            "git_head": data.get("git_head", ""),
            "files": len(data.get("files", [])),
        })
    return out
