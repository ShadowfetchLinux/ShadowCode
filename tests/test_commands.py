"""Tests for the 0.6.0 Codex-style slash-command system."""

from __future__ import annotations

import subprocess
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from shadow_agent.api.server import create_app
from shadow_agent.commands import CommandContext, CommandRegistry, dispatch as dispatch_command
from shadow_agent.commands.handlers import HANDLERS
from shadow_agent.config import load_config
from shadow_agent.store import Store


# --- registry ---------------------------------------------------------------


def test_registry_lists_all_new_builtins(workspace: Path):
    reg = CommandRegistry(workspace)
    names = {c.name for c in reg.list()}
    for n in [
        "shadowcode", "help", "status", "model", "models", "plan", "compact",
        "context", "diff", "review", "test", "run", "git", "commit", "undo",
        "agents", "mcp", "memory", "settings", "hyperpod-nccl",
    ]:
        assert n in names, f"missing builtin: {n}"


def test_every_builtin_has_a_handler(workspace: Path):
    reg = CommandRegistry(workspace)
    for cmd in reg.list():
        if cmd.source != "builtin":
            continue
        assert cmd.name in HANDLERS, f"no handler for /{cmd.name}"


def test_parse_splits_name_and_args(workspace: Path):
    reg = CommandRegistry(workspace)
    cmd, name, args = reg.parse("/model qwen3:14b")
    assert cmd is not None and name == "model" and args == "qwen3:14b"
    cmd, name, args = reg.parse("/status")
    assert cmd is not None and name == "status" and args == ""
    cmd, name, args = reg.parse("just a task")
    assert cmd is None and args == "just a task"


# --- handler context helper ------------------------------------------------


def make_ctx(workspace: Path, **overrides) -> CommandContext:
    store = Store()
    sid = store.create_session(str(workspace), "mock", title="t")
    return CommandContext(
        workspace=workspace,
        store=store,
        registry=overrides.pop("registry", None),
        commands=CommandRegistry(workspace),
        session_id=sid,
        extra={"plan": "", "todos": []},
        **overrides,
    )


# --- brand / orientation ----------------------------------------------------


def test_shadowcode_card(workspace: Path):
    result = dispatch_command("", make_ctx(workspace), "shadowcode")
    assert result.kind == "card" and result.headline.startswith("ShadowCode")
    assert str(workspace) in result.body
    assert "/help" in result.body


def test_help_lists_commands(workspace: Path):
    result = dispatch_command("", make_ctx(workspace), "help")
    assert result.kind == "list"
    labels = " ".join(item["label"] for item in result.items)
    assert "/shadowcode" in labels and "/hyperpod-nccl" in labels
    assert "keybindings" in result.body


def test_status_card(workspace: Path):
    result = dispatch_command("", make_ctx(workspace), "status")
    assert result.kind == "list"
    labels = " ".join(item["label"] for item in result.items)
    assert "version" in labels and "provider" in labels and "model" in labels


# --- model / context --------------------------------------------------------


def test_model_no_arg_shows_active(workspace: Path, isolated):
    result = dispatch_command("", make_ctx(workspace), "model")
    assert result.kind == "card" and "Active model" in result.headline
    assert "mock" in result.body


def test_model_with_arg_switches_default(workspace: Path, isolated):
    c = make_ctx(workspace)
    result = dispatch_command("qwen3:14b", c, "model")
    assert result.kind == "card" and "Switched" in result.headline
    assert load_config(workspace).model.default == "qwen3:14b"


def test_models_lists_builtins(workspace: Path):
    result = dispatch_command("", make_ctx(workspace), "models")
    assert result.kind == "list"
    labels = " ".join(item["label"] for item in result.items)
    assert "mock" in labels and "ollama" in labels


def test_plan_with_arg_replaces(workspace: Path):
    c = make_ctx(workspace)
    result = dispatch_command("Ship 0.6.0", c, "plan")
    assert result.kind == "card" and "Plan updated" in result.headline
    assert c.extra["plan"] == "Ship 0.6.0"


def test_plan_no_arg_shows_current(workspace: Path):
    c = make_ctx(workspace)
    c.extra["plan"] = "Step 1: inspect"
    result = dispatch_command("", c, "plan")
    assert result.kind == "card" and "Step 1: inspect" in result.body


def test_compact_invokes_callback(workspace: Path):
    called = []
    c = make_ctx(workspace, compactor=lambda: called.append(True))
    result = dispatch_command("", c, "compact")
    assert result.kind == "card" and "Compacted" in result.headline
    assert called == [True]


def test_context_meter(workspace: Path):
    result = dispatch_command("", make_ctx(workspace), "context")
    assert result.kind == "list"
    labels = " ".join(item["label"] for item in result.items)
    assert "context limit" in labels and "compact ratio" in labels


# --- workspace / git --------------------------------------------------------


def _git(workspace: Path, *args: str) -> None:
    subprocess.run(["git", *args], cwd=workspace, check=True, capture_output=True)


def test_diff_returns_diff_card(workspace: Path):
    _git(workspace, "init", "-q")
    _git(workspace, "config", "user.email", "t@t")
    _git(workspace, "config", "user.name", "t")
    (workspace / "f.txt").write_text("hi\n", encoding="utf-8")
    _git(workspace, "add", ".")
    result = dispatch_command("", make_ctx(workspace), "diff")
    assert result.kind == "diff"
    assert "hi" in result.diff

def test_diff_no_changes(workspace: Path):
    _git(workspace, "init", "-q")
    _git(workspace, "config", "user.email", "t@t")
    _git(workspace, "config", "user.name", "t")
    (workspace / "f.txt").write_text("hi\n", encoding="utf-8")
    _git(workspace, "add", ".")
    _git(workspace, "commit", "-qm", "init")
    result = dispatch_command("", make_ctx(workspace), "diff")
    assert result.kind == "card" and "No changes" in result.headline


def test_diff_not_a_repo(workspace: Path):
    result = dispatch_command("", make_ctx(workspace), "diff")
    assert result.kind == "card" and "Not a git repo" in result.headline


def test_review_clean(workspace: Path):
    _git(workspace, "init", "-q")
    _git(workspace, "config", "user.email", "t@t")
    _git(workspace, "config", "user.name", "t")
    result = dispatch_command("", make_ctx(workspace), "review")
    assert result.kind == "card" and "Nothing to review" in result.headline


def test_review_with_changes_stashes_prompt(workspace: Path):
    _git(workspace, "init", "-q")
    _git(workspace, "config", "user.email", "t@t")
    _git(workspace, "config", "user.name", "t")
    (workspace / "app.py").write_text("print('hi')\n", encoding="utf-8")
    c = make_ctx(workspace)
    result = dispatch_command("", c, "review")
    assert result.kind == "list" and "Self-review" in result.headline
    assert "review_prompt" in c.extra and "app.py" in c.extra["review_prompt"]


def test_test_runs_command(workspace: Path):
    (workspace / "test_x.py").write_text("def test_x():\n    assert 1\n", encoding="utf-8")
    result = dispatch_command(
        "python3 -m pytest -q test_x.py --rootdir=. -p no:cacheprovider",
        make_ctx(workspace),
        "test",
    )
    assert result.kind == "card" and "test ·" in result.headline
    assert result.metadata.get("exit_code") == 0


def test_run_streams_output(workspace: Path):
    result = dispatch_command("echo hello-shadow", make_ctx(workspace), "run")
    assert result.kind == "card" and "hello-shadow" in result.body
    assert result.metadata.get("exit_code") == 0


def test_git_status_card(workspace: Path):
    _git(workspace, "init", "-q")
    _git(workspace, "config", "user.email", "t@t")
    _git(workspace, "config", "user.name", "t")
    (workspace / "f.txt").write_text("hi\n", encoding="utf-8")
    result = dispatch_command("", make_ctx(workspace), "git")
    assert result.kind == "card" and "git status" in result.headline
    assert "f.txt" in result.body


def test_commit_creates_commit(workspace: Path):
    _git(workspace, "init", "-q")
    _git(workspace, "config", "user.email", "t@t")
    _git(workspace, "config", "user.name", "t")
    (workspace / "f.txt").write_text("hi\n", encoding="utf-8")
    result = dispatch_command("add a file", make_ctx(workspace), "commit")
    assert result.kind == "card" and "Committed" in result.headline
    log = subprocess.run(["git", "log", "--oneline"], cwd=workspace, capture_output=True, text=True).stdout
    assert "add a file" in log


def test_commit_nothing_to_commit(workspace: Path):
    _git(workspace, "init", "-q")
    _git(workspace, "config", "user.email", "t@t")
    _git(workspace, "config", "user.name", "t")
    result = dispatch_command("", make_ctx(workspace), "commit")
    assert result.kind == "card" and "Nothing to commit" in result.headline


def test_undo_no_checkpoint(workspace: Path):
    result = dispatch_command("", make_ctx(workspace), "undo")
    assert result.kind == "card" and "Nothing to undo" in result.headline


def test_undo_restores_checkpoint(workspace: Path, isolated):
    from shadow_agent.checkpoints import CheckpointStore

    (workspace / "keep.py").write_text("old\n", encoding="utf-8")
    store = CheckpointStore(workspace, "task1")
    store.record_call("write_file", {"path": "keep.py"})
    (workspace / "keep.py").write_text("new\n", encoding="utf-8")
    result = dispatch_command("", make_ctx(workspace), "undo")
    assert result.kind == "card" and "Undid" in result.headline
    assert (workspace / "keep.py").read_text(encoding="utf-8") == "old\n"


# --- subagents / mcp / memory / settings -----------------------------------


def test_agents_lists_roles(workspace: Path):
    result = dispatch_command("", make_ctx(workspace), "agents")
    assert result.kind == "list"
    labels = " ".join(item["label"] for item in result.items)
    for role in ("architect", "coder", "reviewer", "tester", "researcher", "security"):
        assert role in labels


def test_agents_with_role_stashes_dispatch(workspace: Path):
    c = make_ctx(workspace)
    result = dispatch_command("reviewer", c, "agents")
    assert result.kind == "card" and "Dispatch to reviewer" in result.headline
    assert c.extra.get("subagent_role") == "reviewer"


def test_mcp_no_servers(workspace: Path):
    result = dispatch_command("", make_ctx(workspace), "mcp")
    assert result.kind == "card" and "No MCP servers" in result.headline


def test_mcp_lists_configured_servers(workspace: Path, isolated):
    from shadow_agent.config import save_config, AppConfig, MCPServerConfig

    cfg = AppConfig()
    cfg.mcp.servers.append(MCPServerConfig(name="linear", url="https://mcp.linear.app/sse"))
    save_config(cfg)
    result = dispatch_command("", make_ctx(workspace), "mcp")
    assert result.kind == "list" and "linear" in " ".join(i["label"] for i in result.items)


def test_memory_append_and_show(workspace: Path):
    c = make_ctx(workspace)
    dispatch_command("Uses pytest.", c, "memory")
    result = dispatch_command("", c, "memory")
    assert result.kind == "card" and "Uses pytest." in result.body


def test_settings_set_writes_config(workspace: Path, isolated):
    result = dispatch_command("model.default ollama", make_ctx(workspace), "settings")
    assert result.kind == "card" and "Set model.default = ollama" in result.headline
    assert load_config().model.default == "ollama"


def test_settings_no_args_prints_config(workspace: Path):
    result = dispatch_command("", make_ctx(workspace), "settings")
    assert result.kind == "card" and "Config" in result.headline


# --- hyperpod-nccl: read-only and safe --------------------------------------


def test_hyperpod_nccl_without_cluster_prints_usage(workspace: Path):
    result = dispatch_command("", make_ctx(workspace), "hyperpod-nccl")
    assert result.kind == "card" and "HyperPod NCCL diagnostic" in result.headline
    assert "read-only" in result.body
    assert "fails" not in result.metadata


def test_hyperpod_nccl_missing_prereqs_is_safe(workspace: Path, monkeypatch):
    monkeypatch.setattr("shadow_agent.commands.handlers.shutil.which", lambda name: None)
    result = dispatch_command("my-cluster us-east-1", make_ctx(workspace), "hyperpod-nccl")
    assert result.kind == "card" and "prerequisites" in result.headline.lower()
    assert "aws CLI" in result.body and "kubectl" in result.body
    assert "fails" not in result.metadata


def test_hyperpod_nccl_never_runs_destructive_commands(workspace: Path, monkeypatch):
    monkeypatch.setattr("shadow_agent.commands.handlers.shutil.which", lambda name: "/usr/bin/" + name)
    runs: list[list[str]] = []

    def fake_run(cmd, **kwargs):
        runs.append(list(cmd))
        return subprocess.CompletedProcess(cmd, 0, "Issues Found: 0\n[PASS] all checks\n", "")

    monkeypatch.setattr("shadow_agent.commands.handlers.subprocess.run", fake_run)
    monkeypatch.setattr("shadow_agent.commands.handlers._resolve_nccl_script", lambda: Path("/tmp/nccl-diagnose.sh"))
    result = dispatch_command("my-cluster us-east-1", make_ctx(workspace), "hyperpod-nccl")
    assert result.kind == "card"
    assert runs, "expected the diagnostic to run"
    assert runs[0][0] == "bash"
    assert "--cluster" in runs[0] and "my-cluster" in runs[0]
    assert "--region" in runs[0] and "us-east-1" in runs[0]
    joined = " ".join(runs[0])
    for bad in ("sudo", "rm -rf", "mkfs", "git push --force", "shutdown"):
        assert bad not in joined


# --- API endpoints ----------------------------------------------------------


@pytest.fixture
def client(isolated, workspace, monkeypatch):
    monkeypatch.setattr("shadow_agent.api.server.detect_providers", lambda: [])
    store = Store()
    app = create_app(store=store, default_workspace=workspace, detect=False)
    return TestClient(app)


def test_api_commands_lists_builtins(client):
    res = client.get("/api/commands")
    assert res.status_code == 200
    names = {c["name"] for c in res.json()["commands"]}
    for n in ["shadowcode", "help", "status", "model", "hyperpod-nccl"]:
        assert n in names


def test_api_commands_run_shadowcode(client):
    res = client.post("/api/commands/run", json={"name": "shadowcode", "args": ""})
    assert res.status_code == 200
    body = res.json()
    assert body["kind"] == "card" and "ShadowCode" in body["headline"]


def test_api_commands_run_status(client):
    res = client.post("/api/commands/run", json={"name": "status", "args": ""})
    assert res.status_code == 200
    body = res.json()
    assert body["kind"] == "list"
    labels = " ".join(item["label"] for item in body["items"])
    assert "version" in labels and "provider" in labels


def test_api_commands_run_unknown_404(client):
    res = client.post("/api/commands/run", json={"name": "no-such", "args": ""})
    assert res.status_code == 404


def test_api_commands_run_settings_set(client, isolated):
    res = client.post("/api/commands/run", json={"name": "settings", "args": "model.default ollama"})
    assert res.status_code == 200
    assert load_config().model.default == "ollama"


def test_api_commands_run_hyperpod_nccl_safe(client, monkeypatch):
    monkeypatch.setattr("shadow_agent.commands.handlers.shutil.which", lambda name: None)
    res = client.post("/api/commands/run", json={"name": "hyperpod-nccl", "args": ""})
    assert res.status_code == 200
    body = res.json()
    assert body["kind"] == "card"
    # No destructive metadata; only a usage/prereqs card.
    assert body.get("metadata", {}).get("fails") in (None, [])
