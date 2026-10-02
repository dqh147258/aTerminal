#!/usr/bin/env python3
"""Use an existing disposable AUTH_REVIEW service and verify native iOS UI plus PTY effects.

The coordinator owns service startup/time slots. This script never starts/stops
Desktop or models and only installs/provisions the named task simulator.
"""
import argparse
import json
import shutil
from pathlib import Path
import plistlib
import platform
import re
import subprocess
import uuid

ROOT = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--fixture-dir', type=Path, required=True)
parser.add_argument('--simulator', required=True)
parser.add_argument('--derived-data', type=Path, default=ROOT / 'build/authorization-derived')
parser.add_argument('--output', type=Path, required=True)
args = parser.parse_args()
fixture = args.fixture_dir.resolve()
config = json.loads((fixture / 'account-fixture.json').read_text())
assert config.get('authorization_test') is True and re.fullmatch(r'http://127\.0\.0\.1:[0-9]+', config['server'])
assert re.fullmatch(r'[a-f0-9]{16}', config['session'])
uuid.UUID(args.simulator)
# Preserve existing device state by requiring this task's dedicated profile.
devices = json.loads(subprocess.check_output(['xcrun', 'simctl', 'list', 'devices', '--json']))
device = next(d for group in devices['devices'].values() for d in group if d['udid'] == args.simulator)
assert device['name'].startswith('aTerminal authorization'), 'Use a dedicated task simulator'
args.output.mkdir(parents=True, exist_ok=True)
app = args.derived_data / 'Build/Products/Debug-iphonesimulator/aTerminal.app'
assert app.is_dir(), 'Run build-for-testing first'
if device['state'] != 'Booted':
    subprocess.run(['xcrun', 'simctl', 'boot', args.simulator], check=True)
subprocess.run(['xcrun', 'simctl', 'bootstatus', args.simulator, '-b'], check=True)
subprocess.run(['xcrun', 'simctl', 'install', args.simulator, str(app)], check=True)
container = Path(subprocess.check_output(['xcrun', 'simctl', 'get_app_container', args.simulator, 'com.yxf.aterminal', 'data'], text=True).strip())
login = container / 'Documents/local-login-fixture.json'
login.parent.mkdir(parents=True, exist_ok=True)
login.write_text(json.dumps({key: config[key] for key in ['server', 'username', 'password']}))
login.chmod(0o600)
prefix = 'ios-auth-' + uuid.uuid4().hex[:12]
# Inject only nonsecret context into the XCTest runner. Use an adjacent generated
# xctestrun so __TESTROOT__ remains the normal build Products directory.
products = args.derived_data / 'Build/Products'
source = max(products.glob("*-" + platform.machine() + ".xctestrun"), key=lambda p: p.stat().st_mtime)
run_file = products / 'authorization-rpc.xctestrun'
run = plistlib.loads(source.read_bytes())
run['aTerminalUITests'].setdefault('EnvironmentVariables', {})['AI_TERMINAL_IOS_AUTHORIZATION_CONTEXT'] = json.dumps({'session': config['session'], 'prefix': prefix, 'stateDirectory': str(fixture)})
run_file.write_bytes(plistlib.dumps(run))
report = {'prefix': prefix, 'session': config['session'], 'scope': 'native iOS simulator, encrypted RPC, deterministic local model and independent PTY/native files', 'passed': False}
try:
    with (args.output / 'xctest.log').open('wb') as log:
        result = subprocess.run(['xcodebuild', '-xctestrun', str(run_file), '-destination', 'platform=iOS Simulator,id=' + args.simulator,
                                 '-resultBundlePath', str(args.output / 'rpc.xcresult'), '-jobs', '2', '-parallel-testing-enabled', 'NO', '-only-testing:aTerminalUITests/AuthorizationRpcUITests',
                                 'test-without-building'], stdout=log, stderr=subprocess.STDOUT, timeout=600)
    report['xcode_exit_code'] = result.returncode
    assert result.returncode == 0, 'Native RPC XCTest failed; inspect xctest.log'
    expected = {'once': 'once\n', 'always': 'always\nalways\nalways\n', 'full': 'full\nfull\n', 'long': 'long-native-detail ' * 350 + '\n'}
    observed = {}
    for suffix, text in expected.items():
        path = fixture / (prefix + '-' + suffix + '.log')
        observed[suffix] = path.read_text()
        assert observed[suffix] == text, 'PTY marker differs: ' + suffix
    for suffix in ['deny', 'full-off']:
        assert not (fixture / (prefix + '-' + suffix + '.log')).exists(), 'Denied action reached PTY: ' + suffix
    report['pty_and_native_markers'] = observed
    report['passed'] = True
finally:
    observations = fixture / 'authorization-model-observations.jsonl'
    if observations.exists():
        shutil.copyfile(observations, args.output / observations.name)
    login.unlink(missing_ok=True)
    run_file.unlink(missing_ok=True)
    (args.output / 'results.json').write_text(json.dumps(report, indent=2) + '\n')
print('PASS: native iOS authorization, encrypted RPC and independently verified PTY effects')
