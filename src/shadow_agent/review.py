"""Bounded, literal-path Git reads for the desktop review panel."""
from __future__ import annotations

import difflib
import subprocess
from pathlib import Path
from typing import Any


def git(workspace: Path, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(["git", "--literal-pathspecs", "-c", "core.quotepath=false", *args], cwd=workspace, capture_output=True, text=True, errors="replace", timeout=15, check=False)


def status(workspace: Path) -> dict[str, Any]:
    inside = git(workspace, "rev-parse", "--is-inside-work-tree").returncode == 0
    if not inside:
        return {"repo": False, "files": [], "status": "", "porcelain": "", "log": "", "diff": ""}
    raw = git(workspace, "status", "--porcelain=v1", "-z").stdout.split("\0")
    files = []
    i = 0
    while i < len(raw):
        row = raw[i]
        i += 1
        if len(row) < 4:
            continue
        entry = {"index": row[0], "work": row[1], "path": row[3:], "label": row[:2].strip() or "M"}
        if "R" in row[:2] or "C" in row[:2]:
            entry["original_path"] = raw[i] if i < len(raw) else ""
            i += 1
        files.append(entry)
    return {"repo": True, "files": files, "status": git(workspace, "status", "-sb").stdout, "porcelain": git(workspace, "status", "--porcelain=v1", "-b").stdout, "log": git(workspace, "log", "-12", "--oneline", "--decorate").stdout, "diff": git(workspace, "diff", "--stat").stdout}


def diff(workspace: Path, path: str = "") -> dict[str, Any]:
    from shadow_agent.tools.sandbox import WorkspaceSandbox
    if path:
        WorkspaceSandbox(workspace).resolve(path)
    suffix = ["--", path] if path else []
    unstaged = git(workspace, "diff", "--no-ext-diff", "--no-color", *suffix).stdout
    staged = git(workspace, "diff", "--cached", "--no-ext-diff", "--no-color", *suffix).stdout
    untracked = bool(path) and bool(git(workspace, "ls-files", "--others", "--exclude-standard", "--", path).stdout)
    binary = "Binary files " in unstaged or "Binary files " in staged
    truncated = False
    if untracked:
        target = WorkspaceSandbox(workspace).resolve(path, must_exist=True)
        if target.is_file():
            with target.open("rb") as handle:
                data = handle.read(200001)
            truncated = len(data) > 200000
            try:
                content = data[:200000].decode("utf-8")
                binary = "\0" in content
            except UnicodeDecodeError:
                binary = True
                content = ""
            if not binary:
                unstaged = "".join(difflib.unified_diff([], content.splitlines(keepends=True), fromfile="/dev/null", tofile=f"b/{path}"))
    if len(unstaged) > 500000 or len(staged) > 500000:
        truncated = True
    return {"path": path, "diff": unstaged[:500000], "staged": staged[:500000], "untracked": untracked, "binary": binary, "truncated": truncated}
