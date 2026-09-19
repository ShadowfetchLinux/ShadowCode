from __future__ import annotations

import subprocess

from shadow_agent.models.types import ToolCall
from shadow_agent.tools.git import git_add, git_commit, git_log, git_status
from shadow_agent.tools.sandbox import WorkspaceSandbox


def _git(workspace, *args: str) -> None:
    subprocess.run(["git", *args], cwd=workspace, check=True, capture_output=True)


def test_git_status_add_commit_log(workspace):
    _git(workspace, "init")
    _git(workspace, "config", "user.email", "shadow@example.test")
    _git(workspace, "config", "user.name", "Shadow Agent")
    (workspace / "note.txt").write_text("hi\n", encoding="utf-8")
    box = WorkspaceSandbox(workspace)
    status = git_status(box, ToolCall(id="1", tool_name="git_status", arguments={}))
    assert status.success
    assert "note.txt" in status.output
    assert git_add(box, ToolCall(id="2", tool_name="git_add", arguments={"paths": ["note.txt"]})).success
    commit = git_commit(box, ToolCall(id="3", tool_name="git_commit", arguments={"message": "add note"}))
    assert commit.success
    log = git_log(box, ToolCall(id="4", tool_name="git_log", arguments={}))
    assert "add note" in log.output
