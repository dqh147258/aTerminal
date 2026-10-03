#!/usr/bin/env python3
"""Run Agent panels against an isolated encrypted Desktop/PTY fixture on one Android device."""
import argparse
import json
from pathlib import Path
import re
import shutil
import signal
import sqlite3
import subprocess
import tempfile
import hashlib
import time

ROOT = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--serial', required=True)
parser.add_argument('--output', type=Path, required=True)
parser.add_argument('--adb', default=shutil.which('adb') or str(Path.home() / 'Library/Android/sdk/platform-tools/adb'))
parser.add_argument('--authorization', action='store_true', help='Run the authorization UI workflow and independently verify PTY marker files')
parser.add_argument('--package', help='Target application ID; authorization defaults to the isolated authorizationfixture package')
parser.add_argument('--aapt', help='aapt executable used to verify APK package IDs before installation')
parser.add_argument('--cli', type=Path, default=ROOT/'target/debug/aTerminal', help='Exact tested Desktop CLI binary')
parser.add_argument('--example', type=Path, default=ROOT/'target/debug/examples/account_demo', help='Matching deterministic fixture binary')
parser.add_argument('--test-timeout', type=int, default=600, help='Authorization instrumentation deadline in seconds')
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=True)
for executable in [args.cli, args.example]:
    assert executable.is_file(), f'Missing fixture binary: {executable}'
(args.output/'runtime-binaries.json').write_text(json.dumps({str(path.resolve()): hashlib.sha256(path.read_bytes()).hexdigest() for path in [args.cli, args.example]}, indent=2))
package = args.package or ('com.yxf.aterminal.authorizationfixture' if args.authorization else 'com.yxf.aterminal')
assert re.fullmatch(r'[A-Za-z][A-Za-z0-9_]*(?:\.[A-Za-z][A-Za-z0-9_]*)+', package), 'Invalid application ID'
if args.authorization:
    assert package != 'com.yxf.aterminal', 'Authorization acceptance requires a separate application ID'
    candidates = sorted((Path.home()/'Library/Android/sdk/build-tools').glob('*/aapt'), reverse=True)
    aapt = args.aapt or shutil.which('aapt') or (str(candidates[0]) if candidates else None)
    assert aapt, 'aapt is required to verify the isolated APK before installation'
    for apk, expected_package in [
        (ROOT/'apps/android/app/build/outputs/apk/debug/app-debug.apk', package),
        (ROOT/'apps/android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk', package+'.test'),
    ]:
        badging = subprocess.check_output([aapt, 'dump', 'badging', str(apk)], text=True)
        declared = re.search(r"^package: name='([^']+)'", badging, re.MULTILINE)
        assert declared and declared.group(1) == expected_package, f'Build APKs with -PauthorizationUiFixture=true before installation: {apk}'

def adb(*command, **kwargs):
    return subprocess.run([args.adb, '-s', args.serial, *map(str, command)], check=True, **kwargs)

with tempfile.TemporaryDirectory(prefix='aterminal-agent-ui-') as directory:
    state = Path(directory) / 'desktop'
    fixture = None
    reverse = None
    previous_ime = None
    with (args.output / 'fixture.log').open('wb') as log:
        fixture = subprocess.Popen([str(args.example), str(state), str(args.cli), '--authorization-test' if args.authorization else '--agent-test'], stdout=log, stderr=log, start_new_session=True)
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
        if args.authorization:
            previous_ime = adb('shell', 'settings', 'get', 'secure', 'show_ime_with_hard_keyboard', capture_output=True).stdout.decode().strip()
            adb('shell', 'settings', 'put', 'secure', 'show_ime_with_hard_keyboard', '1', capture_output=True)
        with (args.output/'instrumentation.log').open('wb') as log:
            test_class = 'com.yxf.aterminal.AgentAuthorizationRpcUiTest' if args.authorization else 'com.yxf.aterminal.AgentReadingUiTest'
            adb('shell', 'am', 'instrument', '-w', '-r', '-e', 'class', test_class, package+'.test/androidx.test.runner.AndroidJUnitRunner', stdout=log, stderr=subprocess.STDOUT, timeout=args.test_timeout if args.authorization else 180)
        transcript = (args.output/'instrumentation.log').read_text()
        report_name = 'authorization-ui-results.json' if args.authorization else 'agent-ui-results.json'
        report = adb('exec-out', 'run-as', package, 'cat', f'files/{report_name}', capture_output=True).stdout
        (args.output/'results.json').write_bytes(report)
        result = json.loads(report)
        assert 'OK (' in transcript and result.get('passed'), 'Agent UI test failed; inspect instrumentation.log/results.json'
        if args.authorization:
            expected = result.get('expected_markers')
            assert isinstance(expected, dict) and expected, 'Authorization UI report must declare independently checkable marker expectations'
            observed = {}
            for name, lines in expected.items():
                marker = (state/name).resolve()
                assert marker.is_relative_to(state.resolve()) and marker != state.resolve(), f'Marker escaped fixture: {name}'
                assert lines is None or isinstance(lines, list) and all(isinstance(line, str) for line in lines), f'Invalid expected marker lines: {name}'
                actual = marker.read_text().splitlines() if marker.exists() else None
                assert actual == lines, f'PTY side effect mismatch for {name}: {actual!r} != {lines!r}'
                observed[name] = actual
            (args.output/'authorization-pty-markers.json').write_text(json.dumps(observed, indent=2))
            print('PASS: native authorization UI, encrypted RPC, and independently observed PTY side effects:', args.output/'results.json')
        else:
            saved = subprocess.check_output([str(args.cli), '--state-dir', str(state), 'config', 'terminal-reading', '--json'], text=True)
            reading = json.loads(saved)['result']['terminal_reading']
            assert reading == {'head_lines': 7, 'tail_lines': 31}, reading
            (args.output/'desktop-reading.json').write_text(saved)
            for name in ['task-result-observations.json', 'task-error-observations.json']:
                shutil.copyfile(state/name, args.output/name)
            task_error = json.loads((state/'task-error-observations.json').read_text())
            # Verify durable completion independently of the daemon's in-memory state.
            with sqlite3.connect(f'file:{state}/data/agent.sqlite3?mode=ro', uri=True) as db:
                persisted = db.execute('SELECT state FROM runs WHERE id=?', [task_error['task_id']]).fetchone()
                pins = db.execute('SELECT COUNT(*) FROM pins WHERE run=?', [task_error['task_id']]).fetchone()[0]
            assert persisted == ('paused',) and pins == 0, (persisted, pins)
            (args.output/'task-durability.json').write_text(json.dumps({'state': persisted[0], 'remaining_child_pins': pins, 'error_truncated': task_error['error_truncated']}, indent=2))
            print('PASS: native Agent settings, encrypted RPC, task results/waits, retention, long errors, PTY evidence and vision upload:', args.output/'results.json')
    finally:
        if previous_ime is not None:
            setting = ['delete', 'secure', 'show_ime_with_hard_keyboard'] if previous_ime == 'null' else ['put', 'secure', 'show_ime_with_hard_keyboard', previous_ime]
            subprocess.run([args.adb, '-s', args.serial, 'shell', 'settings', *setting], capture_output=True)
        if args.authorization:
            declared = {}
            result_file = args.output/'results.json'
            if result_file.exists():
                declared = json.loads(result_file.read_text()).get('expected_markers', {})
            names = set(declared) | {str(path.relative_to(state)) for path in state.rglob('auth-review*') if path.is_file()}
            actual = {}
            for name in sorted(names):
                marker = (state/name).resolve()
                if marker.is_relative_to(state.resolve()) and marker != state.resolve():
                    actual[name] = marker.read_text(errors='replace').splitlines() if marker.is_file() else None
            (args.output/'authorization-actual-markers.json').write_text(json.dumps(actual, indent=2))
        for name in ['task-result-observations.json', 'task-error-observations.json', 'authorization-model-observations.jsonl']:
            if (state/name).exists():
                shutil.copyfile(state/name, args.output/name)
        for name in ['defaults', 'settings-saved', 'conversation', 'history', 'evidence', 'vision', 'global-wire', 'task-results', 'task-error', 'authorization-once', 'authorization-deny', 'authorization-question', 'authorization-full', 'authorization-rules', 'authorization-readonly', 'authorization-timeout']:
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
        subprocess.run([str(args.cli), '--state-dir', str(state), 'daemon', 'stop'], capture_output=True, timeout=10)
