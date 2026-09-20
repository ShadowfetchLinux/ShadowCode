"""Exercise release executables with isolated config, a real HTTP API, and real tools."""
import json
import os
import signal
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path

root = Path(tempfile.mkdtemp(prefix='shadow-package-smoke-'))
workspace = root / 'workspace'; workspace.mkdir()
env = os.environ.copy()
for key, sub in [('XDG_CONFIG_HOME','config'),('XDG_DATA_HOME','data'),('XDG_STATE_HOME','state')]: env[key] = str(root / sub)
env['SHADOW_AGENT_NO_NOTIFY'] = '1'
config = root / 'config/shadow-agent/config.yaml'; config.parent.mkdir(parents=True)
config.write_text('model:\n  provider: mock\n  default: mock\n  name: mock-coder\nonboarding:\n  completed: true\nui:\n  notify: false\n')
command = sys.argv[1:]
open_browser = os.environ.get('SHADOW_TEST_OPEN_BROWSER') == '1'
if open_browser:
    bindir = root / 'bin'; bindir.mkdir()
    browser = bindir / 'brave-browser'
    browser.write_text('#!/bin/sh\nprintf browser-opened > "' + str(root / 'browser-opened') + '"\n')
    browser.chmod(0o755)
    env['PATH'] = str(bindir) + ':' + env['PATH']
port = 17431
base = f'http://127.0.0.1:{port}'
def request(path, payload=None):
    req = urllib.request.Request(base + path, data=json.dumps(payload).encode() if payload is not None else None, headers={'Content-Type':'application/json'})
    with urllib.request.urlopen(req, timeout=10) as response:
        data = response.read()
        return json.loads(data) if 'application/json' in response.headers.get('Content-Type','') else data.decode()
log = (root / 'server.log').open('w')
process = subprocess.Popen(command + ['ui'] + ([] if open_browser else ['--no-browser']) + ['--host','127.0.0.1','--port',str(port),'--project',str(workspace)], env=env, stdout=log, stderr=log, start_new_session=True)
try:
    for _ in range(150):
        try:
            health = request('/api/health'); break
        except Exception:
            if process.poll() is not None: raise RuntimeError((root / 'server.log').read_text())
            time.sleep(.1)
    else: raise RuntimeError('server startup timed out')
    if open_browser:
        for _ in range(30):
            if (root / 'browser-opened').exists(): break
            time.sleep(.1)
        assert (root / 'browser-opened').exists() and process.poll() is None
    assert health['version'] == '0.19.0' and health['app'] == 'ShadowCode'
    html = request('/'); assert '<title>ShadowCode</title>' in html
    import re
    for asset in re.findall(r'(?:src|href)="(/assets/[^\"]+)"', html): assert request(asset)
    job = request('/api/jobs', {'task':'Create a Python hello-world project and run it'})
    for _ in range(200):
        result = request('/api/jobs/' + job['id'])
        if result['status'] not in {'queued','running','cancelling'}: break
        time.sleep(.1)
    assert result['status'] == 'completed', result
    assert (workspace / 'hello.py').is_file()
    assert 'Hello, World!' in result['summary']
    print('PACKAGE SMOKE PASSED:', command[0], health['version'], 'UI assets + real offline tools + verified job')
finally:
    os.killpg(process.pid, signal.SIGTERM)
    process.wait(timeout=10)
    log.close()
