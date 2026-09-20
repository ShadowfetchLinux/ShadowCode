"""Behavioral regressions for the 0.19 workspace and durable task runtime."""
from __future__ import annotations

import json
import subprocess
import threading
import time
from pathlib import Path

import pytest
from fastapi.testclient import TestClient
from shadow_agent.api.server import create_app
from shadow_agent.approvals import ApprovalHub
from shadow_agent.config import AppConfig, apply_config_patch
from shadow_agent.events import EventBus
from shadow_agent.runtime import Job, JobManager
from shadow_agent.store import Store

@pytest.fixture
def client(isolated, workspace, monkeypatch):
    monkeypatch.setattr('shadow_agent.api.server.detect_providers', lambda: [])
    return TestClient(create_app(store=Store(), default_workspace=workspace, detect=False))

def wait_for_job(client, job):
    for _ in range(250):
        result = client.get(f"/api/jobs/{job['id']}").json()
        if result['status'] not in {'queued', 'running', 'cancelling'}:
            return result
        time.sleep(.02)
    pytest.fail('job did not finish')

def test_stream_starts_at_job_boundary_and_honors_cursor(client, workspace):
    sid = client.post('/api/sessions', json={'workspace': str(workspace)}).json()['id']
    store = Store()
    for i in range(850):
        store.add_event('old.event', {'n': i}, session_id=sid)
    job = client.post('/api/jobs', json={'task': 'Explain this workspace', 'session_id': sid}).json()
    wait_for_job(client, job)
    response = client.get(f"/api/jobs/{job['id']}/events")
    events = [json.loads(line[6:]) for line in response.text.splitlines() if line.startswith('data: ')]
    assert not any(e['type'] == 'old.event' for e in events)
    assert events[-1]['type'] == 'job.done'
    ids = [e['id'] for e in events if 'id' in e]
    assert ids == sorted(set(ids)) and min(ids) > 850
    resumed = client.get(f"/api/jobs/{job['id']}/events", headers={'Last-Event-ID': str(ids[-1])})
    assert resumed.text.count('data: ') == 1
    assert client.get('/api/jobs/missing/events').status_code == 404

def test_cursor_pages_do_not_stall_after_800(isolated):
    store = Store()
    sid = store.create_session('/workspace')
    for i in range(1205):
        store.add_event('chunk', {'n': i}, session_id=sid)
    cursor = 0
    seen = []
    while rows := store.events_after(sid, cursor):
        seen.extend(e['payload']['n'] for e in rows)
        cursor = rows[-1]['id']
    assert seen == list(range(1205))

def test_activate_session_switches_file_and_git_workspace(client, workspace, tmp_path):
    other = tmp_path / 'other'; other.mkdir(); (other / 'only-here.txt').write_text('other')
    sid = client.post('/api/sessions', json={'workspace': str(other), 'title': 'other'}).json()['id']
    client.post('/api/sessions', json={'workspace': str(workspace)})
    assert client.post(f'/api/sessions/{sid}/activate').json()['workspace'] == str(other)
    assert client.get('/api/workspace/file', params={'path': 'only-here.txt'}).json()['content'] == 'other'
    assert client.get('/api/workspace/status').json()['workspace'] == str(other)

def test_job_rejects_empty_unknown_and_wrong_workspace(client, workspace, tmp_path):
    assert client.post('/api/jobs', json={'task': '   '}).status_code == 400
    assert client.post('/api/jobs', json={'task': 'hi', 'session_id': 'missing'}).status_code == 400
    other = tmp_path / 'other'; other.mkdir()
    sid = client.post('/api/sessions', json={'workspace': str(other)}).json()['id']
    assert client.post('/api/jobs', json={'task': 'hi', 'session_id': sid, 'workspace': str(workspace)}).status_code == 400

@pytest.mark.parametrize('headers', [{'Origin': 'https://malicious.example'}, {'Origin': 'null'}, {'Sec-Fetch-Site': 'cross-site'}])
def test_browser_boundary_blocks_cross_origin_read_and_write(client, headers):
    assert client.get('/api/config', headers=headers).status_code == 403
    assert client.post('/api/workspace/exec', json={'command': 'echo nope'}, headers=headers).status_code == 403

def test_host_validation_and_same_origin_api(client):
    assert client.get('/api/health', headers={'Host': 'evil.example'}).status_code == 400
    assert client.get('/api/health', headers={'Origin': 'http://testserver'}).status_code == 200
    assert client.get('/api/health').headers['cache-control'] == 'no-store'

def test_read_only_blocks_direct_workspace_mutations(client):
    apply_config_patch({'permissions': {'level': 'read_only'}})
    for method, route, payload in [
        ('put', '/api/workspace/instructions', {'content': 'no'}),
        ('post', '/api/workspace/git/add', {'message': '', 'paths': ['.']}),
        ('post', '/api/workspace/attach', {'filename': 'x', 'text': 'no'}),
        ('post', '/api/checkpoints/undo', {}),
    ]:
        assert getattr(client, method)(route, json=payload).status_code == 403

def test_restarted_job_is_interrupted_and_completed_records_survive(isolated, workspace):
    store = Store()
    sid = store.create_session(str(workspace))
    job = Job('crashed', workspace, 'unfinished', sid)
    job.status = 'running'; store.save_desktop_job(job.to_dict())
    done = Job('done', workspace, 'finished', sid); done.status = 'completed'; done.summary = 'real result'; store.save_desktop_job(done.to_dict())
    manager = JobManager(store, EventBus(), ApprovalHub())
    assert manager.get('crashed').status == 'interrupted'
    assert manager.get('done').summary == 'real result'
    assert manager.list_active() == []

def test_early_cancel_is_observed_and_workspace_concurrency_rejected(isolated, workspace, monkeypatch):
    from shadow_agent.agent.loop import AgentRunner
    entered, release = threading.Event(), threading.Event()
    original = AgentRunner.__init__
    def delayed(self, *args, **kwargs):
        entered.set(); release.wait(3); original(self, *args, **kwargs)
    monkeypatch.setattr(AgentRunner, '__init__', delayed)
    store = Store(); manager = JobManager(store, EventBus(), ApprovalHub())
    job = manager.start(workspace, 'Explain this workspace')
    assert entered.wait(2)
    with pytest.raises(ValueError, match='already running'):
        manager.start(workspace, 'another task')
    manager.cancel(job.id)
    assert job.status == 'cancelling'
    release.set()
    for _ in range(200):
        if job.finished_at: break
        time.sleep(.02)
    assert job.status == 'cancelled'
    assert not any(e['type'] == 'model.request' for e in store.list_events(session_id=job.session_id))

def test_followup_context_and_custom_session_title(isolated, workspace):
    from shadow_agent.agent.loop import AgentRunner
    from shadow_agent.models.adapters.mock import MockProvider
    from shadow_agent.models.types import ChatResponse
    class Capture(MockProvider):
        def chat(self, request):
            self.messages = request.messages
            return ChatResponse(text='The project is named Nightfall.', finish=True)
    store = Store(); sid = store.create_session(str(workspace), 'mock', title='Keep this name')
    store.add_event('agent.started', {'task': 'The project is named Nightfall.'}, session_id=sid, task_id='prior')
    store.add_event('agent.completed', {'summary': 'I will use Nightfall.'}, session_id=sid, task_id='prior')
    model = Capture()
    result = AgentRunner(workspace, store=store, session_id=sid, model=model).run('What is the project name?')
    assert result.success
    assert any(m.role == 'user' and 'named Nightfall' in m.content for m in model.messages)
    assert store.get_session(sid)['title'] == 'Keep this name'

def test_finished_plan_is_in_event_history(client):
    job = client.post('/api/jobs', json={'task': 'Create a Python hello-world project and run it'}).json()
    assert wait_for_job(client, job)['status'] == 'completed'
    events = client.get(f"/api/sessions/{job['session_id']}").json()['events']
    final = [e for e in events if e['type'] == 'agent.completed'][-1]
    assert all(s['status'] == 'done' for s in final['payload']['plan']['steps'])

def test_git_handles_untracked_staged_spaces_and_stale_hunks(client, workspace):
    def git(*args):
        return subprocess.run(['git', *args], cwd=workspace, capture_output=True, text=True, check=True).stdout
    git('init', '-q'); git('config', 'user.name', 'test'); git('config', 'user.email', 'test@example.invalid')
    (workspace / 'space name.txt').write_text('one\ntwo\n'); git('add', '.'); git('commit', '-qm', 'initial')
    (workspace / 'space name.txt').write_text('one\nchanged\n')
    (workspace / 'brand new.txt').write_text('new content\n')
    files = client.get('/api/workspace/git').json()['files']
    assert {f['path'] for f in files} == {'space name.txt', 'brand new.txt'}
    new = client.get('/api/workspace/diff', params={'path': 'brand new.txt'}).json()
    assert new['untracked'] and '+new content' in new['diff']
    d = client.get('/api/workspace/diff', params={'path': 'space name.txt'}).json()
    body = {'path': 'space name.txt', 'hunk': d['hunks'][0], 'action': 'accept'}
    assert client.post('/api/workspace/diff/hunk', json=body).status_code == 200
    assert client.post('/api/workspace/diff/hunk', json=body).status_code == 409
    staged = client.get('/api/workspace/diff', params={'path': 'space name.txt'}).json()
    assert not staged['hunks'] and staged['staged_hunks']


def test_instruction_and_skill_symlinks_cannot_escape_workspace(client, workspace, tmp_path):
    outside = tmp_path / 'outside'; outside.mkdir()
    (outside / 'instructions.md').write_text('private')
    (workspace / '.shadow').symlink_to(outside, target_is_directory=True)
    assert client.get('/api/workspace/instructions').status_code == 400
    assert client.put('/api/workspace/instructions', json={'content': 'changed'}).status_code == 400
    assert client.put('/api/workspace/skills', json={'name': 'test', 'content': 'changed'}).status_code == 400
    assert (outside / 'instructions.md').read_text() == 'private'


def test_background_commands_obey_network_and_dangerous_command_policy(client):
    assert client.post('/api/background', json={'command': 'curl https://example.invalid'}).status_code == 403
    assert client.post('/api/background', json={'command': 'sudo true'}).status_code == 403


def test_latest_interrupted_job_is_available_for_recovery(isolated, workspace):
    store = Store(); sid = store.create_session(str(workspace))
    job = Job('interrupted-ui', workspace, 'unfinished task', sid)
    job.status = 'running'; store.save_desktop_job(job.to_dict())
    client = TestClient(create_app(store=store, default_workspace=workspace, detect=False))
    assert client.get('/api/jobs/current', params={'session_id': sid}).json()['job'] is None
    latest = client.get('/api/jobs/current', params={'session_id': sid, 'include_finished': True}).json()['job']
    assert latest['status'] == 'interrupted'
    assert client.delete(f'/api/sessions/{sid}').status_code == 200
    assert client.get('/api/jobs/interrupted-ui').status_code == 404
    assert not store.desktop_jobs()
