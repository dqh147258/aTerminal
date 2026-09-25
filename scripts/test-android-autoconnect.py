#!/usr/bin/env python3
"""Test an attached Android using disposable server/account and real Desktop PTYs.
Requires freshly built desktop/server binaries and debug/instrumentation APKs.
"""
import argparse
import fcntl
import json
import os
from pathlib import Path
import pty
import secrets
import socket
import struct
import subprocess
import tempfile
import termios
import threading
import time
import urllib.request

ROOT = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--serial', required=True)
parser.add_argument('--output', type=Path, required=True)
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=True)
package = 'dev.aiterminal.app'


def adb(*command, **kwargs):
    return subprocess.run(['adb', '-s', args.serial, *map(str, command)], check=True, **kwargs)


def wait_for(action):
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        try:
            value = action()
            if value:
                return value
        except (OSError, ValueError, subprocess.CalledProcessError):
            pass
        time.sleep(.2)
    raise RuntimeError('Fixture did not become ready')


def drain(fd):
    try:
        while os.read(fd, 65536):
            pass
    except OSError:
        pass


with tempfile.TemporaryDirectory(prefix='aiterminal-autoconnect-') as directory:
    work = Path(directory)
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        port = sock.getsockname()[1]
    url = f'http://127.0.0.1:{port}'
    token, password = secrets.token_urlsafe(32), secrets.token_urlsafe(24)
    username = 'autoconnect_' + secrets.token_hex(4)
    env = dict(os.environ, AI_TERMINAL_BIND=f'127.0.0.1:{port}', AI_TERMINAL_DB=str(work / 'server.db'),
               AI_TERMINAL_ADMIN_TOKEN=token, AI_TERMINAL_CREDENTIAL_STORE='file', XDG_CONFIG_HOME=str(work / 'config'))
    cli = [str(ROOT / 'target/debug/ai-terminal'), '--state-dir', str(work / 'agent')]
    terminals, masters = [], []
    server = None
    reversed_port = False
    try:
        with (args.output / 'server.log').open('wb') as log:
            server = subprocess.Popen([str(ROOT / 'target/debug/ai-terminal-server')], env=env, stdout=log, stderr=log)
        wait_for(lambda: urllib.request.urlopen(url + '/healthz', timeout=1).status == 200)
        request = urllib.request.Request(url + '/v2/admin/users', data=json.dumps({'username': username, 'password': password}).encode(),
            headers={'Authorization': 'Bearer ' + token, 'X-Admin-Request': '1', 'Content-Type': 'application/json'}, method='POST')
        with urllib.request.urlopen(request, timeout=10) as response:
            assert response.status in (200, 201)
        subprocess.run(cli + ['auth', 'login', '--server', url, '--username', username, '--name', 'AutoConnect Desktop', '--password-stdin'],
                       env=env, input=password + '\n', text=True, capture_output=True, check=True)
        wait_for(lambda: any(d['online'] for d in json.loads(subprocess.check_output(cli + ['devices', 'list'], env=env))))
        sessions = []
        for name, marker in [('older', 'OLDER_TERMINAL_OK'), ('newer', 'NEWEST_TERMINAL_OK')]:
            cwd = work / name
            cwd.mkdir()
            master, slave = pty.openpty()
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 24, 80, 0, 0))
            process = subprocess.Popen(cli + ['--cwd', str(cwd), '--', '/bin/sh', '-c', f"printf '{marker}\\n'; exec /bin/sh"],
                                       stdin=slave, stdout=slave, stderr=slave, env=dict(env, TERM='xterm-256color'), start_new_session=True)
            os.close(slave)
            terminals.append(process)
            masters.append(master)
            threading.Thread(target=drain, args=(master,), daemon=True).start()
            def find_session():
                lines = subprocess.check_output(cli + ['--list'], env=env, text=True).splitlines()
                return next((line.split('\t')[0] for line in lines if line.endswith(str(cwd))), None)
            sessions.append(wait_for(find_session))
        print('Fixture ready: one Desktop, two live PTYs', flush=True)
        adb('install', '-r', ROOT / 'apps/android/app/build/outputs/apk/debug/app-debug.apk', capture_output=True)
        adb('install', '-r', ROOT / 'apps/android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk', capture_output=True)
        adb('shell', 'am', 'force-stop', package, capture_output=True)
        adb('shell', 'run-as', package, 'mkdir', '-p', 'files')
        fixture = dict(server=url, username=username, password=password, oldest=sessions[0], newest=sessions[1])
        adb('shell', f"run-as {package} sh -c 'cat > files/autoconnect-fixture.json'", input=json.dumps(fixture).encode(), capture_output=True)
        adb('shell', 'run-as', package, 'rm', '-f', 'files/autoconnect-results.json', capture_output=True)
        adb('reverse', f'tcp:{port}', f'tcp:{port}', capture_output=True)
        reversed_port = True
        adb('shell', 'input', 'keyevent', 'KEYCODE_WAKEUP')
        adb('shell', 'wm', 'dismiss-keyguard')
        with (args.output / 'instrumentation.log').open('wb') as log:
            adb('shell', 'am', 'instrument', '-w', '-r', '-e', 'class', 'dev.aiterminal.app.AutoConnectTest',
                package + '.test/androidx.test.runner.AndroidJUnitRunner', stdout=log, stderr=subprocess.STDOUT, timeout=180)
        transcript = (args.output / 'instrumentation.log').read_text()
        for name in ['latest', 'devices', 'reconnected']:
            result = subprocess.run(['adb', '-s', args.serial, 'exec-out', 'run-as', package, 'cat', f'files/autoconnect-{name}.png'], capture_output=True)
            if result.returncode == 0:
                (args.output / f'{name}.png').write_bytes(result.stdout)
        assert 'OK (1 test)' in transcript, transcript
        report = adb('exec-out', 'run-as', package, 'cat', 'files/autoconnect-results.json', capture_output=True).stdout
        (args.output / 'results.json').write_bytes(report)
        assert json.loads(report)['passed']
        print('PASS:', args.output / 'results.json', flush=True)
    finally:
        subprocess.run(['adb', '-s', args.serial, 'shell', 'am', 'force-stop', package], capture_output=True)
        subprocess.run(['adb', '-s', args.serial, 'shell', 'run-as', package, 'rm', '-f', 'files/autoconnect-fixture.json'], capture_output=True)
        if reversed_port:
            subprocess.run(['adb', '-s', args.serial, 'reverse', '--remove', f'tcp:{port}'], capture_output=True)
        for process in terminals:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
        for master in masters:
            os.close(master)
        subprocess.run(cli + ['--agent-stop'], env=env, capture_output=True, timeout=10)
        if server is not None:
            server.terminate()
            server.wait(timeout=10)
