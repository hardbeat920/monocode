import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import urllib.request

qualification = Path.home() / 'monocode-gpui-qualification'
binary = qualification / 'target/debug/monocode-host'
data = Path(tempfile.mkdtemp(prefix='host-smoke-', dir=qualification))
with socket.socket() as listener:
    listener.bind(('127.0.0.1', 0))
    port = listener.getsockname()[1]
args = ['--data-dir', str(data), '--port', str(port)]

def run(command):
    result = subprocess.run([str(binary), *command, *args], text=True, capture_output=True, timeout=30)
    if result.returncode:
        raise RuntimeError(f'Host command {command[0]} exited {result.returncode}: {result.stderr}')
    return result.stdout.strip()

started = False
state_path = data / 'running.json'
try:
    print(subprocess.check_output([str(binary), '--version'], text=True).strip())
    print(run(['start']))
    started = True
    deadline = time.monotonic() + 10
    while not state_path.exists() and time.monotonic() < deadline:
        time.sleep(0.05)
    state = json.loads(state_path.read_text())
    assert state['port'] == port
    request = urllib.request.Request(
        f'http://127.0.0.1:{port}/lifecycle',
        data=b'{"action":"status"}',
        headers={'Authorization': 'Bearer ' + state['secret'], 'Content-Type': 'application/json'},
    )
    with urllib.request.urlopen(request, timeout=5) as response:
        status = json.load(response)
    assert status['pid'] == state['pid']
    assert status['network']['enabled'] is False
    listeners = subprocess.check_output(['ss', '-ltnH'], text=True)
    bindings = [line for line in listeners.splitlines() if f':{port}' in line.split()[3]]
    assert len(bindings) == 1 and bindings[0].split()[3] == f'127.0.0.1:{port}', bindings
    print(run(['status']))
    print(json.dumps({'version': status['version'], 'port': port, 'runningTurns': status['runningTurns'], 'loopbackOnly': True}))
finally:
    if started or state_path.exists():
        print(run(['stop']))
        deadline = time.monotonic() + 10
        while (data / 'running.json').exists() and time.monotonic() < deadline:
            time.sleep(0.05)
        assert not (data / 'running.json').exists(), 'The host did not release its running marker'
        assert run(['status']) == 'Host is stopped'
        while time.monotonic() < deadline:
            listeners = subprocess.check_output(['ss', '-ltnH'], text=True)
            if not any(line.split()[3] == f'127.0.0.1:{port}' for line in listeners.splitlines()):
                break
            time.sleep(0.05)
        else:
            raise AssertionError('The host listener did not stop')
        print('Host is stopped')
