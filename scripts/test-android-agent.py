#!/usr/bin/env python3
"""Run Agent panels against an isolated encrypted Desktop/PTY fixture on one Android device."""
import argparse
import json
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--serial', required=True)
parser.add_argument('--output', type=Path, required=True)
parser.add_argument('--adb', default=shutil.which('adb') or str(Path.home() / 'Library/Android/sdk/platform-tools/adb'))
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=True)
package = 'com.yxf.aterminal'

def adb(*command, **kwargs):
    return subprocess.run([args.adb, '-s', args.serial, *map(str, command)], check=True, **kwargs)

with tempfile.TemporaryDirectory(prefix='aterminal-agent-ui-') as directory:
    state = Path(directory) / 'desktop'
    fixture = None
    reverse = None
    with (args.output / 'fixture.log').open('wb') as log:
        fixture = subprocess.Popen([str(ROOT/'target/debug/examples/account_demo'), str(state), str(ROOT/'target/debug/aTerminal'), '--agent-test'], stdout=log, stderr=log, start_new_session=True)
    try:
        deadline = time.monotonic() + 30
        while not (state/'account-fixture.json').exists():
            if fixture.poll() is not None:
                raise RuntimeError('Fixture exited; inspect fixture.log')
            if time.monotonic() > deadline:
                raise RuntimeError('Fixture startup timed out')
            time.sleep(.1)
        config = json.loads((state/'account-fixture.json').read_text())
        assert config['server'].startswith('http://127.0.0.1:')
        port = int(config['server'].rsplit(':', 1)[1])
        adb('install', '-r', ROOT/'apps/android/app/build/outputs/apk/debug/app-debug.apk', capture_output=True)
        adb('install', '-r', ROOT/'apps/android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk', capture_output=True)
        adb('shell', 'run-as', package, 'mkdir', '-p', 'files')
        adb('shell', f"run-as {package} sh -c 'cat > files/agent-ui-fixture.json'", input=json.dumps(config).encode(), capture_output=True)
        adb('reverse', f'tcp:{port}', f'tcp:{port}', capture_output=True)
        reverse = port
        adb('shell', 'input', 'keyevent', 'KEYCODE_WAKEUP', capture_output=True)
        adb('shell', 'wm', 'dismiss-keyguard', capture_output=True)
        with (args.output/'instrumentation.log').open('wb') as log:
            adb('shell', 'am', 'instrument', '-w', '-r', '-e', 'class', 'com.yxf.aterminal.AgentReadingUiTest', package+'.test/androidx.test.runner.AndroidJUnitRunner', stdout=log, stderr=subprocess.STDOUT, timeout=180)
        transcript = (args.output/'instrumentation.log').read_text()
        report = adb('exec-out', 'run-as', package, 'cat', 'files/agent-ui-results.json', capture_output=True).stdout
        (args.output/'results.json').write_bytes(report)
        assert 'OK (1 test)' in transcript and json.loads(report).get('passed'), 'Agent UI test failed; inspect instrumentation.log/results.json'
        saved = subprocess.check_output([str(ROOT/'target/debug/aTerminal'), '--state-dir', str(state), 'config', 'terminal-reading', '--json'], text=True)
        reading = json.loads(saved)['result']['terminal_reading']
        assert reading == {'head_lines': 7, 'tail_lines': 31}, reading
        (args.output/'desktop-reading.json').write_text(saved)
        print('PASS: native Agent settings, encrypted RPC, PTY evidence, current path and vision upload:', args.output/'results.json')
    finally:
        for name in ['defaults', 'settings-saved', 'conversation', 'history', 'evidence', 'vision', 'global-wire']:
            picture = subprocess.run([args.adb, '-s', args.serial, 'exec-out', 'run-as', package, 'cat', f'files/agent-ui-{name}.png'], capture_output=True)
            if picture.returncode == 0:
                (args.output/f'{name}.png').write_bytes(picture.stdout)
        subprocess.run([args.adb, '-s', args.serial, 'shell', 'run-as', package, 'rm', '-f', 'files/agent-ui-fixture.json'], capture_output=True)
        if reverse is not None:
            subprocess.run([args.adb, '-s', args.serial, 'reverse', '--remove', f'tcp:{reverse}'], capture_output=True)
        if fixture.poll() is None:
            fixture.send_signal(signal.SIGINT)
            try:
                fixture.wait(timeout=10)
            except subprocess.TimeoutExpired:
                fixture.terminate()
                fixture.wait(timeout=5)
        subprocess.run([str(ROOT/'target/debug/aTerminal'), '--state-dir', str(state), 'daemon', 'stop'], capture_output=True, timeout=10)
