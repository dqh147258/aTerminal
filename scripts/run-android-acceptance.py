#!/usr/bin/env python3
"""Run the isolated Android acceptance fixture; credentials never enter command arguments."""
import argparse
import json
from pathlib import Path
import subprocess
import time

ROOT = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser()
parser.add_argument('--serial', required=True)
parser.add_argument('--fixture', type=Path, required=True)
parser.add_argument('--output', type=Path, required=True)
parser.add_argument('--apk', type=Path, default=ROOT / 'apps/android/app/build/outputs/apk/debug/app-debug.apk')
parser.add_argument('--mode', choices=['full', 'smoke'], default='full')
parser.add_argument('--input-interval-ms', type=int, default=10, choices=range(1, 101))
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=True)
prefix = ['adb', '-s', args.serial]


def adb(*command, **kwargs):
    return subprocess.run(prefix + list(command), check=True, **kwargs)


fixture = args.fixture.read_bytes()
config = json.loads(fixture)
port = config['server'].rsplit(':', 1)[1]
assert config['server'].startswith('http://127.0.0.1:') and port.isdigit()
adb('install', '-r', str(args.apk), capture_output=True)
adb('install', '-r', str(ROOT / 'apps/android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk'), capture_output=True)
adb('shell', 'run-as', 'com.yxf.aterminal', 'mkdir', '-p', 'files')
adb('shell', "run-as com.yxf.aterminal sh -c 'cat > files/device-fixture.json'", input=fixture)
adb('reverse', 'tcp:' + port, 'tcp:' + port, capture_output=True)
try:
    for label, command in [('thermal-before', 'thermalservice'), ('battery-before', 'battery')]:
        with (args.output / (label + '.txt')).open('wb') as output:
            adb('shell', 'dumpsys', command, stdout=output)
    adb('shell', 'input', 'keyevent', 'KEYCODE_WAKEUP')
    adb('shell', 'wm', 'dismiss-keyguard')
    adb('shell', 'am', 'start', '-W', '-n', 'com.yxf.aterminal/.MainActivity', capture_output=True)
    deadline = time.monotonic() + 5
    while True:
        window = adb('shell', 'dumpsys', 'window', capture_output=True, text=True).stdout
        focus = [line for line in window.splitlines() if 'mFocusedApp=' in line]
        if focus and all('com.yxf.aterminal/' in line for line in focus):
            break
        if time.monotonic() > deadline:
            raise RuntimeError('aTerminal not in foreground; input test cancelled')
        time.sleep(0.1)
    with (args.output / 'instrumentation.log').open('wb') as output:
        adb('shell', 'am', 'instrument', '-w', '-r', '-e', 'mode', args.mode,
            '-e', 'nativeBuild', 'release', '-e', 'inputIntervalMs', str(args.input_interval_ms), '-e', 'class',
            'com.yxf.aterminal.DeviceAcceptanceTest',
            'com.yxf.aterminal.test/androidx.test.runner.AndroidJUnitRunner',
            stdout=output, stderr=subprocess.STDOUT, timeout=180)
    report = adb('exec-out', 'run-as', 'com.yxf.aterminal', 'cat',
                 'files/device-results.json', capture_output=True).stdout
    (args.output / 'results.json').write_bytes(report)
    transcript = (args.output / 'instrumentation.log').read_text()
    assert 'OK (1 test)' in transcript and json.loads(report).get('passed'), 'Acceptance failed; inspect instrumentation.log'
    print('PASS: device acceptance; report:', args.output / 'results.json')
finally:
    adb('shell', 'am', 'force-stop', 'com.yxf.aterminal')
    adb('shell', 'run-as', 'com.yxf.aterminal', 'rm', '-f', 'files/device-fixture.json')
    adb('reverse', '--remove', 'tcp:' + port)
