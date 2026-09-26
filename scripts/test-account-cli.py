#!/usr/bin/env python3
"""Exercise the CLI with an isolated account_demo fixture; never print the password/transcript."""
import argparse
import json
import os
from pathlib import Path
import pty
import select
import signal
import subprocess
import tempfile
import termios
import time

parser = argparse.ArgumentParser()
parser.add_argument('binary', type=Path)
parser.add_argument('fixture', type=Path)
parser.add_argument('--os-vault', action='store_true')
args = parser.parse_args()
config = json.loads(args.fixture.read_text())
binary = str(args.binary.resolve())
with tempfile.TemporaryDirectory(prefix='aiterminal-auth-cli-') as directory:
    env = os.environ.copy()
    if args.os_vault:
        env.pop('AI_TERMINAL_CREDENTIAL_STORE', None)
    else:
        env['AI_TERMINAL_CREDENTIAL_STORE'] = 'file'
    env['XDG_CONFIG_HOME'] = directory + '/config'
    child, master = pty.fork()
    if child == 0:
        os.execve(binary, [binary, '--state-dir', directory, 'auth', 'login', '--server',
                          config['server'], '--username', config['username'], '--name', 'CLI Test'], env)
    received = b''
    sent = False
    status = None
    deadline = time.monotonic() + 35
    def run(*command):
        return subprocess.check_output([binary, '--state-dir', directory, *command], env=env, text=True)
    try:
        while time.monotonic() < deadline:
            if select.select([master], [], [], 0.1)[0]:
                try:
                    received += os.read(master, 4096)
                except OSError:
                    pass
            if b'Password:' in received and not sent and not termios.tcgetattr(master)[3] & termios.ECHO:
                os.write(master, (config['password'] + '\n').encode())
                sent = True
            done, child_status = os.waitpid(child, os.WNOHANG)
            if done:
                status = child_status
                break
        assert status == 0 and b'Logged in as ' in received, 'CLI login failed (transcript withheld)'
        assert config['password'].encode() not in received, 'Password echoed'
        assert config['username'] in run('auth', 'status')
        assert 'Local Desktop' in run('devices', 'list')
        assert 'Logged out' in run('auth', 'logout')
        assert 'Not logged in' in run('auth', 'status')
        print('PASS: hidden password login, status, devices, logout; credentials not echoed')
    finally:
        if status is None:
            os.kill(child, signal.SIGKILL)
            os.waitpid(child, 0)
        os.close(master)
        if ((Path(directory) / 'runtime/endpoint.json').exists() or (Path(directory) / 'endpoint.json').exists()):
            subprocess.run([binary, '--state-dir', directory, '--agent-stop'], env=env, check=True,
                           stdout=subprocess.DEVNULL)
