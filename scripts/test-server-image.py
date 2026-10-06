#!/usr/bin/env python3
"""Smoke-test a locally loaded server image without exposing a public port."""
import argparse
import json
import pathlib
import secrets
import sqlite3
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import uuid


def docker(*args, check=True):
    return subprocess.run(['docker', *args], check=check, text=True, capture_output=True, timeout=60)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('image')
    args = parser.parse_args()
    config = json.loads(docker('image', 'inspect', args.image).stdout)[0]['Config']
    assert config['User'] == '10001:10001', 'Image must run as non-root'
    assert config.get('Healthcheck'), 'Image must include its health check'
    assert not any('TOKEN=' in e or 'TOKEN_FILE=' in e for e in config.get('Env', []))
    suffix = uuid.uuid4().hex
    name, volume = 'aterminal-smoke-' + suffix, 'aterminal-smoke-data-' + suffix
    with tempfile.TemporaryDirectory() as directory:
        token = pathlib.Path(directory) / 'admin-token'
        token.write_text(secrets.token_hex(32))
        token.chmod(0o444)
        try:
            missing = docker('run', '--rm', '--network', 'none', args.image, check=False)
            assert missing.returncode != 0, 'Missing admin credentials must fail closed'
            docker('volume', 'create', volume)
            docker('run', '-d', '--name', name, '--read-only', '--cap-drop=ALL',
                   '--security-opt=no-new-privileges:true', '--pids-limit=64', '--memory=128m',
                   '-p', '127.0.0.1::8787', '-v', volume + ':/data',
                   '--mount', f'type=bind,src={token},dst=/run/secrets/admin_token,readonly',
                   '-e', 'AI_TERMINAL_ADMIN_TOKEN_FILE=/run/secrets/admin_token', args.image)
            port = docker('port', name, '8787/tcp').stdout.strip().rsplit(':', 1)[1]
            url = 'http://127.0.0.1:' + port
            for attempt in range(60):
                if docker('exec', name, '/usr/local/bin/ai-terminal-server', '--healthcheck', check=False).returncode == 0:
                    break
                time.sleep(1)
            else:
                raise AssertionError('Server did not become healthy')
            with urllib.request.urlopen(url + '/healthz', timeout=3) as response:
                assert response.status == 200 and response.read() == b'ok'
            try:
                urllib.request.urlopen(urllib.request.Request(url + '/v1/pairs', method='POST'), timeout=3)
            except urllib.error.HTTPError as error:
                assert error.code == 401, f'Unexpected unauthenticated status: {error.code}'
            else:
                raise AssertionError('Admin endpoint allowed unauthenticated access')
            request = urllib.request.Request(url + '/v1/pairs', method='POST',
                                             headers={'Authorization': 'Bearer ' + token.read_text()})
            with urllib.request.urlopen(request, timeout=3) as response:
                room = json.load(response)['room']
            docker('restart', name)
            for attempt in range(60):
                if docker('exec', name, '/usr/local/bin/ai-terminal-server', '--healthcheck', check=False).returncode == 0:
                    break
                time.sleep(1)
            else:
                raise AssertionError('Server did not recover after restart')
            docker('stop', name)
            snapshot = pathlib.Path(directory) / 'snapshot'
            snapshot.mkdir()
            docker('cp', name + ':/data/.', str(snapshot))
            with sqlite3.connect(snapshot / 'terminal.sqlite3') as database:
                assert database.execute('SELECT room FROM pairs WHERE room=?', (room,)).fetchone() == (room,), 'Pair record did not persist through restart'
            print('PASS: non-root, fail-closed credentials, hardened startup, health, authorization, persisted database and restart')
        finally:
            docker('rm', '-f', name, check=False)
            docker('volume', 'rm', volume, check=False)


if __name__ == '__main__':
    main()
