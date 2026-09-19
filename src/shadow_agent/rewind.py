"""Named checkpoints with multi-dimension rewind.

Extends the existing per-task ``CheckpointStore`` (which only restores file
mutations from one task) with named, timestamped checkpoints that can
restore any subset of:

  files        — restore the file tree snapshot
  conversation — restore the session event log to the checkpoint
  agent_state  — restore the plan + todos + stage
  memory       — restore project + task memory
  git          — `git reset --hard <sha>` to the recorded HEAD

A checkpoint is created explicitly with ``/checkpoint <label>`` or
implicitly at task completion. ``/rewind <id>`` opens the multi-dimension
restore selector.
"""

from __future__ import annotations

import json
import shutil
import sqlite3
import time
import uuid
from enum import Enum
from pathlib import Path
from typing import Any

from shadow_agent import paths


class RewindDimension(str, Enum):
    FILES = "files"
    CONVERSATION = "conversation"
    AGENT_STATE = "agent_state"
    MEMORY = "memory"
    GIT = "git"


ALL_DIMENSIONS = tuple(d.value for d in RewindDimension)


class RewindStore:
    """Persists named checkpoints under ``state_dir/checkpoints/named/``.

    Each checkpoint is a directory:

        <id>/
          manifest.json   — id, label, ts, workspace, session_id, task_id, dimensions
          files.tar       — workspace file tree snapshot (best-effort; skips .git)
          conversation.json — copied event rows for the session up to ts
          agent_state.json   — plan + todos + stage at checkpoint time
          memory.json        — project + task memory at checkpoint time
          git_head.txt       — HEAD sha at checkpoint time
    """

    def __init__(self, workspace: Path, state_dir: Path | None = None) -> None:
        self.workspace = Path(workspace).resolve()
        self.root = (state_dir or paths.state_dir() / "checkpoints" / "named")
        self.root.mkdir(parents=True, exist_ok=True)

    # --- create -----------------------------------------------------------

    def create(
        self,
        label: str,
        *,
        session_id: str = "",
        task_id: str = "",
        agent_state: dict[str, Any] | None = None,
        memory: dict[str, str] | None = None,
        store: Any | None = None,
    ) -> dict[str, Any]:
        cid = _next_id(self.root)
        cdir = self.root / cid
        cdir.mkdir(parents=True, exist_ok=True)
        ts = time.time()
        git_head = _git_head(self.workspace)
        file_count = _snapshot_files(self.workspace, cdir / "files.tar")
        if store is not None and session_id:
            _snapshot_conversation(store, session_id, cdir / "conversation.json")
        else:
            (cdir / "conversation.json").write_text("[]", encoding="utf-8")
        if agent_state is not None:
            (cdir / "agent_state.json").write_text(json.dumps(agent_state, default=str), encoding="utf-8")
        else:
            (cdir / "agent_state.json").write_text("{}", encoding="utf-8")
        if memory is not None:
            (cdir / "memory.json").write_text(json.dumps(memory, default=str), encoding="utf-8")
        else:
            (cdir / "memory.json").write_text("{}", encoding="utf-8")
        (cdir / "git_head.txt").write_text(git_head, encoding="utf-8")
        manifest = {
            "id": cid,
            "label": label,
            "ts": ts,
            "workspace": str(self.workspace),
            "session_id": session_id,
            "task_id": task_id,
            "file_count": file_count,
            "git_head": git_head,
            "dimensions": list(ALL_DIMENSIONS),
        }
        (cdir / "manifest.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
        _write_index(self.root)
        return manifest

    # --- list / get -------------------------------------------------------

    def list(self) -> list[dict[str, Any]]:
        out: list[dict[str, Any]] = []
        for cdir in sorted(self.root.iterdir()) if self.root.is_dir() else []:
            mpath = cdir / "manifest.json"
            if not mpath.is_file():
                continue
            try:
                out.append(json.loads(mpath.read_text(encoding="utf-8")))
            except json.JSONDecodeError:
                continue
        return out

    def get(self, cid: str) -> dict[str, Any] | None:
        mpath = self.root / cid / "manifest.json"
        if not mpath.is_file():
            return None
        try:
            return json.loads(mpath.read_text(encoding="utf-8"))
        except json.JSONDecodeError:
            return None

    # --- restore ----------------------------------------------------------

    def restore(
        self,
        cid: str,
        dimensions: list[str] | None = None,
        *,
        store: Any | None = None,
    ) -> dict[str, Any]:
        manifest = self.get(cid)
        if manifest is None:
            return {"ok": False, "error": f"no checkpoint {cid}"}
        dims = [RewindDimension(d) for d in (dimensions or ALL_DIMENSIONS)]
        cdir = self.root / cid
        results: dict[str, Any] = {"ok": True, "id": cid, "restored": {}}
        for dim in dims:
            if dim is RewindDimension.FILES:
                results["restored"]["files"] = _restore_files(self.workspace, cdir / "files.tar")
            elif dim is RewindDimension.CONVERSATION:
                results["restored"]["conversation"] = _restore_conversation(store, manifest.get("session_id", ""), cdir / "conversation.json")
            elif dim is RewindDimension.AGENT_STATE:
                results["restored"]["agent_state"] = _restore_agent_state(cdir / "agent_state.json")
            elif dim is RewindDimension.MEMORY:
                results["restored"]["memory"] = _restore_memory(self.workspace, cdir / "memory.json")
            elif dim is RewindDimension.GIT:
                results["restored"]["git"] = _restore_git(self.workspace, cdir / "git_head.txt")
        return results

    def remove(self, cid: str) -> bool:
        cdir = self.root / cid
        if not cdir.is_dir():
            return False
        shutil.rmtree(cdir, ignore_errors=True)
        _write_index(self.root)
        return True


# --- helpers --------------------------------------------------------------


def _next_id(root: Path) -> str:
    existing: list[int] = []
    for cdir in root.iterdir() if root.is_dir() else []:
        if cdir.is_dir() and cdir.name.isdigit():
            existing.append(int(cdir.name))
    return f"{(max(existing) + 1) if existing else 1:03d}"


def _write_index(root: Path) -> None:
    items: list[dict[str, Any]] = []
    for cdir in sorted(root.iterdir()) if root.is_dir() else []:
        mpath = cdir / "manifest.json"
        if mpath.is_file():
            try:
                items.append(json.loads(mpath.read_text(encoding="utf-8")))
            except json.JSONDecodeError:
                continue
    (root / "index.json").write_text(json.dumps(items, indent=2), encoding="utf-8")


def _git_head(workspace: Path) -> str:
    import subprocess

    proc = subprocess.run(["git", "rev-parse", "HEAD"], cwd=workspace, capture_output=True, text=True, check=False)
    return proc.stdout.strip() or ""


def _snapshot_files(workspace: Path, dest: Path) -> int:
    import tarfile

    count = 0
    with tarfile.open(dest, "w") as tar:
        for path in workspace.rglob("*"):
            if ".git" in path.parts:
                continue
            if "__pycache__" in path.parts or ".venv" in path.parts or "node_modules" in path.parts:
                continue
            if not path.is_file():
                continue
            try:
                tar.add(path, arcname=str(path.relative_to(workspace)))
                count += 1
            except (OSError, ValueError):
                continue
    return count


def _restore_files(workspace: Path, src: Path) -> dict[str, Any]:
    import tarfile

    if not src.is_file():
        return {"ok": False, "error": "no file snapshot"}
    restored = 0
    with tarfile.open(src, "r") as tar:
        for member in tar.getmembers():
            try:
                tar.extract(member, path=workspace, filter="data")  # type: ignore[call-arg]
                restored += 1
            except (OSError, KeyError):
                continue
    return {"ok": True, "restored": restored}


def _snapshot_conversation(store: Any, session_id: str, dest: Path) -> None:
    rows: list[dict[str, Any]] = []
    if store is not None and session_id:
        try:
            rows = store.list_events(session_id=session_id, limit=2000)
        except Exception:  # noqa: BLE001
            rows = []
    dest.write_text(json.dumps(rows, default=str), encoding="utf-8")


def _restore_conversation(store: Any, session_id: str, src: Path) -> dict[str, Any]:
    if store is None:
        return {"ok": False, "error": "no store available"}
    if not src.is_file():
        return {"ok": False, "error": "no conversation snapshot"}
    try:
        rows = json.loads(src.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return {"ok": False, "error": "corrupt conversation snapshot"}
    # Best-effort: truncate events for the session after the checkpoint ts,
    # then re-insert the snapshot rows. We do not delete history; we only
    # re-emit the captured events so the transcript matches.
    if not session_id:
        return {"ok": True, "restored": 0, "note": "no session_id; snapshot retained"}
    # Insert back the captured rows (idempotent: skip if the row id exists).
    inserted = 0
    existing_ids = {row.get("id") for row in store.list_events(session_id=session_id, limit=5000)}
    for row in rows:
        rid = row.get("id")
        if rid is None or rid in existing_ids:
            continue
        try:
            store.add_event(row.get("type", "restored.event"), row.get("payload", {}), session_id=session_id, task_id=row.get("task_id"), ts=row.get("ts"))
            inserted += 1
        except Exception:  # noqa: BLE001
            continue
    return {"ok": True, "restored": inserted}


def _restore_agent_state(src: Path) -> dict[str, Any]:
    if not src.is_file():
        return {"ok": False, "error": "no agent_state snapshot"}
    try:
        return {"ok": True, "state": json.loads(src.read_text(encoding="utf-8"))}
    except json.JSONDecodeError:
        return {"ok": False, "error": "corrupt agent_state snapshot"}


def _restore_memory(workspace: Path, src: Path) -> dict[str, Any]:
    if not src.is_file():
        return {"ok": False, "error": "no memory snapshot"}
    try:
        memory = json.loads(src.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return {"ok": False, "error": "corrupt memory snapshot"}
    restored = 0
    shadow = workspace / ".shadow" / "memory"
    shadow.mkdir(parents=True, exist_ok=True)
    for key, value in memory.items():
        if not isinstance(value, str):
            continue
        (shadow / f"{key}.md").write_text(value, encoding="utf-8")
        restored += 1
    return {"ok": True, "restored": restored}


def _restore_git(workspace: Path, src: Path) -> dict[str, Any]:
    import subprocess

    if not src.is_file():
        return {"ok": False, "error": "no git snapshot"}
    target = src.read_text(encoding="utf-8").strip()
    if not target:
        return {"ok": False, "error": "checkpoint has no git HEAD"}
    if not (workspace / ".git").exists():
        return {"ok": False, "error": "not a git repo"}
    proc = subprocess.run(["git", "reset", "--hard", target], cwd=workspace, capture_output=True, text=True, check=False)
    if proc.returncode != 0:
        return {"ok": False, "error": proc.stderr.strip() or "git reset failed"}
    return {"ok": True, "head": target}
