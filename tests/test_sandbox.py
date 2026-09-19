from __future__ import annotations

import pytest

from shadow_agent.tools.sandbox import SandboxError, WorkspaceSandbox


def test_resolve_stays_inside(workspace):
    box = WorkspaceSandbox(workspace)
    (workspace / "a.txt").write_text("x", encoding="utf-8")
    assert box.resolve("a.txt") == (workspace / "a.txt").resolve()
    with pytest.raises(SandboxError):
        box.resolve("../secret")
    with pytest.raises(SandboxError):
        box.resolve(workspace.parent / "nope")
