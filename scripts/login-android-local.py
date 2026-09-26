#!/usr/bin/env python3
"""Install the local build and sign in using the existing local account, then leave the App open."""
import argparse
import json
from pathlib import Path
import subprocess
import time
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--serial', required=True)
parser.add_argument('--expect-terminal', action='store_true', help='Require auto-connection to an existing running Terminal')
parser.add_argument('--output', type=Path, default=ROOT / '.local/android-local-login')
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=True)
package = 'com.yxf.aterminal'
credentials = json.loads((ROOT / '.local/local-dev/account.json').read_text())


def adb(*command, **kwargs):
    return subprocess.run(['adb', '-s', args.serial, *map(str, command)], check=True, **kwargs)


adb('install', '-r', ROOT / 'apps/android/app/build/outputs/apk/debug/app-debug.apk', capture_output=True)
adb('install', '-r', ROOT / 'apps/android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk', capture_output=True)
adb('shell', 'am', 'force-stop', package, capture_output=True)
adb('shell', 'run-as', package, 'mkdir', '-p', 'files', capture_output=True)
try:
    adb('shell', f"run-as {package} sh -c 'cat > files/local-login-fixture.json'", input=json.dumps(credentials).encode(), capture_output=True)
    adb('shell', 'input', 'keyevent', 'KEYCODE_WAKEUP', capture_output=True)
    adb('shell', 'wm', 'dismiss-keyguard', capture_output=True)
    reports = []
    for phase in ['login', 'restored']:
        adb('shell', 'am', 'force-stop', package, capture_output=True)
        with (args.output / f'{phase}.log').open('wb') as log:
            adb('shell', 'am', 'instrument', '-w', '-r', '-e', 'class', 'com.yxf.aterminal.LocalLoginTest', '-e', 'phase', phase,
                package + '.test/androidx.test.runner.AndroidJUnitRunner', stdout=log, stderr=subprocess.STDOUT, timeout=120)
        transcript = (args.output / f'{phase}.log').read_text()
        assert 'OK (1 test)' in transcript, transcript
        reports.append(json.loads(adb('exec-out', 'run-as', package, 'cat', f'files/local-login-{phase}.json', capture_output=True).stdout))
    if args.expect_terminal:
        assert all(report.get('connected') and report.get('terminal_selected') for report in reports), 'No Terminal was automatically opened'
    assert reports[0]['pid'] != reports[1]['pid']
    assert reports[0]['device_id'] == reports[1]['device_id']
    screenshot = subprocess.run(['adb', '-s', args.serial, 'exec-out', 'run-as', package, 'cat', 'files/local-login-page.png'], capture_output=True)
    if screenshot.returncode == 0:
        (args.output / 'login-page.png').write_bytes(screenshot.stdout)
finally:
    adb('shell', 'run-as', package, 'rm', '-f', 'files/local-login-fixture.json', capture_output=True)
# No fixture/debug launch extras: the user receives the normal, signed-in App.
adb('shell', 'am', 'force-stop', package, capture_output=True)
adb('shell', 'am', 'start', '-W', '-f', '0x10008000', '-n', package + '/.MainActivity',
    '-a', 'android.intent.action.MAIN', '-c', 'android.intent.category.LAUNCHER', capture_output=True)
deadline = time.monotonic() + 45
while time.monotonic() < deadline:
    adb('shell', 'uiautomator', 'dump', '/data/local/tmp/aterminal-login-window.xml', capture_output=True, timeout=10)
    tree = ET.fromstring(adb('exec-out', 'cat', '/data/local/tmp/aterminal-login-window.xml', capture_output=True).stdout)
    descriptions = {node.get('content-desc') for node in tree.iter('node')}
    if ('终端屏幕，点击输入' if args.expect_terminal else '账号与设备') in descriptions:
        break
    time.sleep(.2)
else:
    raise RuntimeError('Normal App launch did not finish restoring the workspace')
(args.output / 'results.json').write_text(json.dumps(dict(passed=True, phases=reports, normal_launch_ready=True), indent=2))
(args.output / 'signed-in.png').write_bytes(adb('exec-out', 'screencap', '-p', capture_output=True).stdout)
print('Signed in:', credentials['username'], 'at', credentials['server'], 'on', args.serial)
print('Process restart restored the same account/device. Report:', args.output / 'results.json')
