"""Tests for the 0.3.0 flagship upgrade: discovery, model UX, retry, trust, hunks, notify, doctor."""

from __future__ import annotations

import time
from pathlib import Path

import httpx
import pytest
from fastapi.testclient import TestClient

from shadow_agent.agent.loop import AgentRunner, _provider_retryable
from shadow_agent.api.server import _build_hunk_patch, _parse_diff_hunks, create_app
from shadow_agent.config import AppConfig, load_config, save_config
from shadow_agent.events import EventBus
from shadow_agent.models.discovery import _parse_ollama, detected_model_entries, detect_providers
from shadow_agent.models.interface import ModelProvider
from shadow_agent.models.registry import ModelRegistry, probe_provider
from shadow_agent.models.types import Capabilities, ChatRequest, ChatResponse, Message, ToolCall, ToolSpec
from shadow_agent.notify import notify_done
from shadow_agent.store import Store


# --- discovery -------------------------------------------------------------

OLLAMA_TAGS = {
    "models": [
        {
            "name": "qwen3:14b",
            "model": "qwen3:14b",
            "size": 9276198565,
            "details": {"parameter_size": "14.8B", "quantization_level": "Q4_K_M", "context_length": 40960},
            "capabilities": ["completion", "tools", "thinking"],
        },
        {
            "name": "gpt-oss:20b",
            "model": "gpt-oss:20b",
            "size": 13793441244,
            "details": {"parameter_size": "20.9B", "quantization_level": "MXFP4", "context_length": 131072},
            "capabilities": ["completion", "tools", "thinking"],
        },
    ]
}


def test_parse_ollama_tags_capabilities():
    models = _parse_ollama(OLLAMA_TAGS)
    assert [m.id for m in models] == ["qwen3:14b", "gpt-oss:20b"]
    qwen = models[0]
    assert qwen.capabilities["tools"] and qwen.capabilities["thinking"]
    assert qwen.context_limit == 40960
    assert "14.8B" in qwen.detail and "GiB" in qwen.detail


def test_detect_providers_offline(monkeypatch):
    monkeypatch.setattr("shadow_agent.models.discovery._port_open", lambda url: False)
    found = detect_providers()
    assert len(found) == 4
    assert all(not item.running for item in found)
    assert {item.provider for item in found} == {"ollama", "local", "llamacpp", "vllm"}


def test_detected_model_entries_flatten():
    # build from a fake payload instead of touching the network
    from shadow_agent.models.discovery import DetectedProvider

    fake = DetectedProvider(provider="ollama", label="Ollama", endpoint="http://127.0.0.1:11434/v1", running=True)
    fake.models = _parse_ollama(OLLAMA_TAGS)
    entries = detected_model_entries([fake])
    assert len(entries) == 2
    assert entries[0]["id"] == "qwen3:14b"
    assert entries[0]["provider"] == "ollama"
    assert entries[0]["metadata"]["detected"] is True
    assert entries[0]["metadata"]["capabilities"]["tools"] is True
    assert entries[0]["context_limit"] == 40960


def test_registry_registers_detected_and_overrides(isolated, monkeypatch):
    from shadow_agent.models.discovery import DetectedProvider

    fake = DetectedProvider(provider="ollama", label="Ollama", endpoint="http://127.0.0.1:11434/v1", running=True)
    fake.models = _parse_ollama(OLLAMA_TAGS)
    monkeypatch.setattr("shadow_agent.models.registry.detect_providers", lambda timeout=1.2: [fake])
    registry = ModelRegistry(detect=True)
    info = registry.get("qwen3:14b")
    assert info is not None and info.provider == "ollama"

    cfg = AppConfig()  # default mock
    provider = registry.create(cfg, model_id="qwen3:14b")
    assert isinstance(provider, ModelProvider)
    assert getattr(provider, "model") == "qwen3:14b"
    assert "11434" in getattr(provider, "endpoint")
    # context limit comes from the detected model, not the mock default
    assert provider.get_context_limit() == 40960


def test_probe_provider_mock_ok():
    result = probe_provider("mock", name="mock-coder", endpoint="", api_key_env="OPENAI_API_KEY")
    assert result["ok"]
    assert result["latency_ms"] >= 0


# --- native Ollama adapter ---------------------------------------------------


def test_ollama_native_payload_and_parse():
    from shadow_agent.models.adapters.ollama import OllamaProvider

    provider = OllamaProvider(model="qwen3:14b", endpoint="http://127.0.0.1:11434/v1")
    assert provider.endpoint == "http://127.0.0.1:11434"  # /v1 stripped for native API
    payload = provider._payload(
        ChatRequest(
            messages=[
                Message(role="user", content="hi"),
                Message(role="assistant", content="", tool_calls=[ToolCall(id="1", tool_name="list_files", arguments={"path": "."})]),
                Message(role="tool", name="list_files", tool_call_id="1", content="{}"),
            ],
            tools=[ToolSpec(name="list_files", description="list", parameters={"type": "object"})],
        ),
        stream=False,
    )
    assert payload["think"] is False
    assert payload["stream"] is False
    assert payload["tools"][0]["function"]["name"] == "list_files"
    # assistant tool calls carry dict arguments; tool results carry the tool name
    assert payload["messages"][1]["tool_calls"][0]["function"]["arguments"] == {"path": "."}
    assert payload["messages"][2] == {"role": "tool", "content": "{}", "name": "list_files"}

    parsed = provider._parse(
        {
            "message": {"role": "assistant", "content": "", "tool_calls": [{"function": {"name": "list_files", "arguments": {"path": "."}}}]},
            "done": True,
            "done_reason": "stop",
            "prompt_eval_count": 146,
            "eval_count": 19,
        }
    )
    assert parsed.tool_calls[0].tool_name == "list_files"
    assert parsed.tool_calls[0].arguments == {"path": "."}
    assert not parsed.finish  # tool calls pending
    assert parsed.usage == {"prompt_tokens": 146, "completion_tokens": 19, "total_tokens": 165}

    done = provider._parse({"message": {"role": "assistant", "content": "All done."}, "done": True, "done_reason": "stop"})
    assert done.finish and done.text == "All done."


# --- retry with backoff ----------------------------------------------------


class FlakyProvider(ModelProvider):
    name = "flaky"

    def __init__(self, failures: int, exc_factory) -> None:
        self.failures = failures
        self.exc_factory = exc_factory
        self.calls = 0

    def get_capabilities(self) -> Capabilities:
        return Capabilities()

    def get_context_limit(self) -> int:
        return 32000

    def generate(self, prompt: str, **kwargs: object) -> str:
        return self.chat(ChatRequest(messages=[])).text

    def stream(self, request: ChatRequest):
        yield "x"

    def chat(self, request: ChatRequest) -> ChatResponse:
        self.calls += 1
        if self.calls <= self.failures:
            raise self.exc_factory()
        return ChatResponse(text="done.", finish=True, usage={"total_tokens": 3})


def test_provider_retryable_classification():
    assert _provider_retryable(httpx.ConnectError("refused"))
    assert _provider_retryable(httpx.ReadTimeout("slow"))
    assert not _provider_retryable(ValueError("nope"))


def test_loop_retries_transient_provider_errors(isolated, workspace, store: Store, bus: EventBus, monkeypatch):
    monkeypatch.setattr(time, "sleep", lambda s: None)
    cfg = AppConfig()
    cfg.agent.model_retries = 3
    cfg.agent.retry_backoff_sec = 0.0
    flaky = FlakyProvider(2, lambda: httpx.ConnectError("connection refused"))
    runner = AgentRunner(workspace, config=cfg, store=store, events=bus, model=flaky)
    result = runner.run("summarize this empty project")
    assert flaky.calls == 3
    retries = [e for e in bus.history() if e["type"] == "model.retry"]
    assert len(retries) == 2
    assert retries[0]["payload"]["attempt"] == 1
    assert result.success


def test_loop_gives_up_after_retries(isolated, workspace, store: Store, bus: EventBus, monkeypatch):
    monkeypatch.setattr(time, "sleep", lambda s: None)
    cfg = AppConfig()
    cfg.agent.model_retries = 2
    cfg.agent.retry_backoff_sec = 0.0
    flaky = FlakyProvider(99, lambda: httpx.ConnectError("down"))
    runner = AgentRunner(workspace, config=cfg, store=store, events=bus, model=flaky)
    result = runner.run("summarize this empty project")
    assert flaky.calls == 2
    assert not result.success
    assert any(e["type"] == "model.error" for e in bus.history())


def test_loop_does_not_retry_permanent_errors(isolated, workspace, store: Store, bus: EventBus):
    cfg = AppConfig()
    cfg.agent.model_retries = 3
    cfg.agent.retry_backoff_sec = 0.0
    flaky = FlakyProvider(5, lambda: ValueError("bad request"))
    runner = AgentRunner(workspace, config=cfg, store=store, events=bus, model=flaky)
    result = runner.run("summarize this empty project")
    assert flaky.calls == 1
    assert not result.success


# --- per-task model override + routing -------------------------------------


def test_runner_model_override_uses_registry_entry(isolated, workspace, store: Store, bus: EventBus, monkeypatch):
    from shadow_agent.models.discovery import DetectedProvider

    fake = DetectedProvider(provider="ollama", label="Ollama", endpoint="http://127.0.0.1:11434/v1", running=True)
    fake.models = _parse_ollama(OLLAMA_TAGS)
    monkeypatch.setattr("shadow_agent.models.registry.detect_providers", lambda timeout=1.2: [fake])
    cfg = AppConfig()
    runner = AgentRunner(workspace, config=cfg, store=store, events=bus, model_override="qwen3:14b")
    assert getattr(runner.model, "model", "") == "qwen3:14b"


def test_runner_routing_hint(isolated, workspace, store: Store, bus: EventBus):
    cfg = AppConfig()
    cfg.routing.enabled = True
    cfg.routing.planner = "mock"
    runner = AgentRunner(workspace, config=cfg, store=store, events=bus, purpose="planner")
    assert runner.model.name == "mock"


# --- API: models test/select/detect, trust, exec, doctor --------------------


@pytest.fixture
def client(isolated, workspace, monkeypatch):
    monkeypatch.setattr("shadow_agent.api.server.detect_providers", lambda: [])
    store = Store()
    app = create_app(store=store, default_workspace=workspace, detect=False)
    return TestClient(app)


def test_api_models_test_mock(client):
    res = client.post("/api/models/test", json={"provider": "mock", "name": "mock-coder"})
    assert res.status_code == 200
    assert res.json()["ok"]


def test_api_models_select_and_detect(client):
    res = client.post("/api/models/select", json={"id": "ollama"})
    assert res.status_code == 200
    assert res.json()["model"]["provider"] == "ollama"
    assert res.json()["model"]["default"] == "ollama"
    res = client.post("/api/models/select", json={"id": "mock"})
    assert res.status_code == 200
    det = client.get("/api/providers/detect")
    assert det.status_code == 200
    assert det.json()["providers"] == []


def test_api_models_select_unknown_404(client):
    res = client.post("/api/models/select", json={"id": "no-such-model"})
    assert res.status_code == 404


def test_workspace_trust_flow(isolated, workspace, monkeypatch, tmp_path):
    monkeypatch.setattr("shadow_agent.api.server.detect_providers", lambda: [])
    store = Store()
    app = create_app(store=store, default_workspace=workspace, detect=False)
    client = TestClient(app)
    other = tmp_path / "other-project"
    other.mkdir()

    first = client.post("/api/projects", json={"path": str(other)})
    assert first.status_code == 200
    assert first.json()["needs_trust"] is True

    trusted = client.post("/api/projects/trust", json={"path": str(other)})
    assert trusted.status_code == 200
    assert str(other) in trusted.json()["trusted"]

    second = client.post("/api/projects", json={"path": str(other)})
    assert second.json().get("needs_trust") is False
    assert load_config().trusted_workspaces == [str(other)]


def test_workspace_exec_endpoint(client, workspace):
    res = client.post("/api/workspace/exec", json={"command": "echo shadow-rerun"})
    assert res.status_code == 200
    assert res.json()["ok"]
    assert "shadow-rerun" in res.json()["stdout"]
    assert res.json()["exit_code"] == 0


def test_workspace_exec_blocked_in_read_only(isolated, workspace, monkeypatch):
    monkeypatch.setattr("shadow_agent.api.server.detect_providers", lambda: [])
    cfg = AppConfig()
    cfg.permissions.level = cfg.permissions.level.READ_ONLY
    save_config(cfg)
    store = Store()
    app = create_app(store=store, default_workspace=workspace, detect=False)
    client = TestClient(app)
    res = client.post("/api/workspace/exec", json={"command": "echo nope"})
    assert res.status_code == 403


def test_doctor_endpoint_and_report(client, isolated):
    from shadow_agent.health import doctor_report

    res = client.get("/api/doctor")
    assert res.status_code == 200
    body = res.json()
    ids = {c["id"] for c in body["checks"]}
    assert {"python", "config", "wrapper", "desktop-entry", "icon", "provider"} <= ids
    # isolated HOME has no install → wrapper check fails with a fix suggestion
    wrapper = next(c for c in body["checks"] if c["id"] == "wrapper")
    assert not wrapper["ok"]
    assert wrapper["fix"]
    assert body["suggestions"]
    report = doctor_report(load_config(), None)
    assert report["checks"]


# --- hunk accept/reject -----------------------------------------------------


def _git(workspace: Path, *args: str) -> str:
    import subprocess

    proc = subprocess.run(["git", *args], cwd=workspace, capture_output=True, text=True, check=False)
    assert proc.returncode == 0, proc.stderr
    return proc.stdout


def test_hunk_reject_and_accept(isolated, workspace, monkeypatch):
    monkeypatch.setattr("shadow_agent.api.server.detect_providers", lambda: [])
    _git(workspace, "init", "-q")
    _git(workspace, "config", "user.email", "t@t")
    _git(workspace, "config", "user.name", "t")
    (workspace / "app.py").write_text("line1\nline2\nline3\nline4\nline5\n", encoding="utf-8")
    _git(workspace, "add", ".")
    _git(workspace, "commit", "-qm", "init")
    (workspace / "app.py").write_text("line1\nline2 changed\nline3\nline4\nline5\n", encoding="utf-8")

    import subprocess

    diff = subprocess.run(["git", "diff"], cwd=workspace, capture_output=True, text=True).stdout
    hunks = _parse_diff_hunks(diff)
    assert len(hunks) == 1
    patch = _build_hunk_patch("app.py", hunks[0])
    assert patch.startswith("--- a/app.py")
    assert "-line2" in patch and "+line2 changed" in patch

    store = Store()
    app = create_app(store=store, default_workspace=workspace, detect=False)
    client = TestClient(app)

    # reject → worktree reverted
    res = client.post("/api/workspace/diff/hunk", json={"path": "app.py", "hunk": hunks[0], "action": "reject"})
    assert res.status_code == 200, res.text
    assert "line2 changed" not in (workspace / "app.py").read_text(encoding="utf-8")

    # re-apply, then accept → staged, worktree matches index
    (workspace / "app.py").write_text("line1\nline2 changed\nline3\nline4\nline5\n", encoding="utf-8")
    res = client.post("/api/workspace/diff/hunk", json={"path": "app.py", "hunk": hunks[0], "action": "accept"})
    assert res.status_code == 200, res.text
    staged = subprocess.run(["git", "diff", "--cached"], cwd=workspace, capture_output=True, text=True).stdout
    assert "line2 changed" in staged


# --- plan status normalization (real models invent statuses) -----------------


def test_plan_normalizes_invented_statuses():
    from shadow_agent.planning.plan import initial_plan, normalize_status

    assert normalize_status("complete") == "done"
    assert normalize_status("Completed") == "done"
    assert normalize_status("working") == "in_progress"
    assert normalize_status("nonsense") == "pending"
    plan = initial_plan("Create a Python hello-world project")
    plan.update(step_id="s1", status="complete")  # type: ignore[arg-type] - real models send this
    assert plan.steps[0].status == "done"
    assert "[x]" in plan.to_markdown()
    plan.update(title="New step from model", status="finished")  # type: ignore[arg-type]
    assert plan.steps[-1].status == "done"


# --- verifier accepts real-world hello-world output --------------------------


def test_verifier_accepts_lowercase_hello_world(workspace):
    import json as _json

    from shadow_agent.models.types import ToolCall, ToolResult
    from shadow_agent.verification.verifier import Verifier

    (workspace / "hello.py").write_text('print("Hello, world!")\n', encoding="utf-8")
    verifier = Verifier(workspace)
    output = _json.dumps({"command": "python3 hello.py", "stdout": "Hello, world!\n", "exit_code": 0})
    call = ToolCall(id="1", tool_name="exec", arguments={"command": "python3 hello.py"})
    verifier.observe(call, ToolResult(id="1", success=True, output=output))
    verdict = verifier.verify("Create a Python hello-world project")
    assert verdict.ok, verdict.reason


# --- desktop notify ----------------------------------------------------------


def test_notify_skips_short_jobs(monkeypatch):
    called = []
    monkeypatch.setattr("shadow_agent.notify.subprocess.Popen", lambda *a, **k: called.append(a) or None)
    assert notify_done("task", success=True, duration_sec=1.0, enabled=True, after_sec=4.0) is False
    assert called == []


def test_notify_sends_for_long_jobs(monkeypatch):
    called = []

    class FakeProc:
        pass

    monkeypatch.delenv("SHADOW_AGENT_NO_NOTIFY", raising=False)  # Popen is mocked; exercise the real path
    monkeypatch.setattr("shadow_agent.notify.shutil.which", lambda name: "/usr/bin/notify-send")
    monkeypatch.setattr("shadow_agent.notify.subprocess.Popen", lambda *a, **k: called.append(a) or FakeProc())
    assert notify_done("build the thing", success=True, duration_sec=30.0, summary="All done", enabled=True, after_sec=4.0) is True
    assert called and "notify-send" in called[0][0][0]


def test_job_manager_notifies(isolated, workspace, monkeypatch):
    sent = []
    monkeypatch.setattr("shadow_agent.runtime.notify_done", lambda *a, **k: sent.append(k) or True)
    monkeypatch.setattr("shadow_agent.api.server.detect_providers", lambda: [])
    store = Store()
    app = create_app(store=store, default_workspace=workspace, detect=False)
    client = TestClient(app)
    job = client.post("/api/jobs", json={"task": "Create a Python hello-world project", "workspace": str(workspace)})
    job_id = job.json()["id"]
    deadline = time.time() + 20
    while time.time() < deadline:
        row = client.get(f"/api/jobs/{job_id}").json()
        if row["status"] in {"completed", "failed", "cancelled"}:
            break
        time.sleep(0.1)
    assert row["status"] == "completed"
    assert sent and sent[0]["success"] is True
