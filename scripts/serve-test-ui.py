"""Isolated real API for browser tests. Never touches the user's workspace/config."""
import os
import subprocess
import tempfile
from pathlib import Path

root = Path(tempfile.mkdtemp(prefix="shadow-ui-test-"))
for name, folder in [("XDG_CONFIG_HOME", "config"), ("XDG_DATA_HOME", "data"), ("XDG_STATE_HOME", "state")]:
    os.environ[name] = str(root / folder)
os.environ["SHADOW_AGENT_NO_NOTIFY"] = "1"
os.environ["SHADOW_AGENT_DOCTOR_NO_INSTALL"] = "1"
workspace = root / "demo-workspace"
workspace.mkdir()
(workspace / "README.md").write_text("# Demo workspace\n\nA safe workspace for browser checks.\n")
(workspace / "example.py").write_text('print("Hello, ShadowCode")\n')
subprocess.run(["git", "init", "-q", "-b", "main", str(workspace)], check=True)
subprocess.run(["git", "add", "."], cwd=workspace, check=True)
subprocess.run(["git", "-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-qm", "Initial workspace"], cwd=workspace, check=True)
from shadow_agent.config import AppConfig, save_config
cfg = AppConfig()
cfg.onboarding.completed = True
cfg.onboarding.workspace = str(workspace)
cfg.trusted_workspaces = [str(workspace)]
save_config(cfg)
from shadow_agent.api import server
server.detect_providers = lambda: []
server.ModelRegistry.refresh_detected = lambda self: []
from shadow_agent import updater
updater.check_for_update = lambda **kw: {"current": "0.19.0", "latest": "0.19.0", "update_available": False}
from shadow_agent.store import Store
store = Store()
store.touch_project(workspace)
store.create_session(str(workspace), "mock", title="New task")
import uvicorn
uvicorn.run(server.create_app(store=store, default_workspace=workspace, detect=False), host="127.0.0.1", port=17430, log_level="warning")
