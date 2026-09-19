from __future__ import annotations

import time
from pathlib import Path

from fastapi.testclient import TestClient

from shadow_agent.api.server import create_app
from shadow_agent.approvals import ApprovalHub
from shadow_agent.checkpoints import CheckpointStore
from shadow_agent.config import AppConfig, apply_config_patch
from shadow_agent.export import export_session
from shadow_agent.health import collect_health
from shadow_agent.models.types import ToolCall
from shadow_agent.secrets import has_secret, set_secret
from shadow_agent.store import Store
from shadow_agent.tools.filesystem import edit_file
from shadow_agent.tools.patch import apply_patch
from shadow_agent.tools.sandbox import WorkspaceSandbox


def test_secrets_stay_out_of_yaml(isolated):
    set_secret("XAI_API_KEY", "sk-test-not-real")
    assert has_secret("XAI_API_KEY")
    from shadow_agent import paths

    text = paths.config_file().read_text(encoding="utf-8")
    assert "sk-test-not-real" not in text
    assert paths.secrets_file().stat().st_mode & 0o777 == 0o600


def test_health_mock_is_ready(isolated, workspace):
    payload = collect_health(AppConfig(), workspace)
    assert payload["ok"]
    assert payload["provider"]["ok"]
    assert payload["tools"]["python"]["ok"]
    assert payload["version"]


def test_config_deep_merge(isolated):
    cfg = apply_config_patch({"ui": {"theme": "dim"}, "onboarding": {"completed": True}})
    assert cfg.ui.theme == "dim"
    assert cfg.model.default == "mock"
    assert cfg.onboarding.completed


def test_fuzzy_edit_and_apply_patch(workspace):
    box = WorkspaceSandbox(workspace)
    (workspace / "app.py").write_text("def add(a, b):\n    return a - b\n", encoding="utf-8")
    fuzzy = edit_file(
        box,
        ToolCall(id="1", tool_name="edit_file", arguments={"path": "app.py", "old_string": "return a - b  ", "new_string": "return a + b"}),
    )
    assert fuzzy.success
    patched = apply_patch(
        box,
        ToolCall(
            id="2",
            tool_name="apply_patch",
            arguments={
                "patch": "--- a/app.py\n+++ b/app.py\n@@ -1,2 +1,2 @@\n def add(a, b):\n-    return a + b\n+    return a * b\n"
            },
        ),
    )
    assert patched.success, patched.error
    assert "return a * b" in (workspace / "app.py").read_text(encoding="utf-8")


def test_checkpoint_undo(workspace):
    (workspace / "keep.py").write_text("old\n", encoding="utf-8")
    store = CheckpointStore(workspace, "task1")
    store.record_call("write_file", {"path": "keep.py"})
    store.record_call("write_file", {"path": "new.py"})
    (workspace / "keep.py").write_text("new\n", encoding="utf-8")
    (workspace / "new.py").write_text("created\n", encoding="utf-8")
    restored = store.restore()
    assert "keep.py" in restored
    assert (workspace / "keep.py").read_text(encoding="utf-8") == "old\n"
    assert not (workspace / "new.py").exists()


def test_approval_hub_decide():
    hub = ApprovalHub(timeout_sec=2)
    result = {}

    def worker():
        result["decision"] = hub.request({"command": "rm -rf /tmp/x", "session_id": "s"})

    import threading

    thread = threading.Thread(target=worker)
    thread.start()
    time.sleep(0.05)
    pending = hub.list_pending()
    assert pending
    hub.decide(pending[0]["id"], "deny")
    thread.join(timeout=2)
    assert result["decision"] == "deny"


def test_api_onboarding_job_export_undo(isolated, workspace):
    store = Store()
    app = create_app(store=store, default_workspace=workspace)
    client = TestClient(app)
    health = client.get("/api/health")
    assert health.status_code == 200
    assert health.json()["ok"]
    onboard = client.get("/api/onboarding")
    assert onboard.status_code == 200
    assert onboard.json()["completed"] is False
    done = client.post(
        "/api/onboarding",
        json={"workspace": str(workspace), "provider": "mock", "permission_level": "workspace"},
    )
    assert done.status_code == 200
    assert done.json()["ok"]
    job = client.post("/api/jobs", json={"task": "Create a Python hello-world project", "workspace": str(workspace)})
    assert job.status_code == 200
    job_id = job.json()["id"]
    session_id = job.json()["session_id"]
    deadline = time.time() + 20
    status = "running"
    while time.time() < deadline:
        row = client.get(f"/api/jobs/{job_id}").json()
        status = row["status"]
        if status in {"completed", "failed", "cancelled"}:
            break
        time.sleep(0.1)
    assert status == "completed", row
    assert (workspace / "hello.py").is_file()
    files = client.get("/api/workspace/files").json()
    assert any(item["name"] == "hello.py" for item in files["entries"])
    exported = client.get(f"/api/sessions/{session_id}/export")
    assert exported.status_code == 200
    assert "ShadowCode session" in exported.text
    undo = client.post("/api/checkpoints/undo")
    assert undo.status_code == 200
    assert not (workspace / "hello.py").exists() or "hello.py" in undo.json()["restored"]


def test_export_helper(isolated, workspace, store: Store):
    sid = store.create_session(str(workspace), "mock", title="demo")
    store.create_task(sid, "hello")
    body, media = export_session(store, sid, "md")
    assert "demo" in body or "hello" in body
    assert "markdown" in media
