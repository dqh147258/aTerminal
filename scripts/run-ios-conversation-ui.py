#!/usr/bin/env python3
"""Run offline native conversation fixtures in a dedicated disposable simulator."""
import json
from pathlib import Path
import platform
import subprocess

ROOT = Path(__file__).resolve().parent.parent


def simctl(*arguments):
    return subprocess.check_output(['xcrun', 'simctl', *arguments], text=True).strip()


def main():
    if platform.system() != 'Darwin':
        raise SystemExit('Native iOS UI tests require macOS and Xcode.')
    runtimes = json.loads(simctl('list', 'runtimes', '--json'))['runtimes']
    available = [item for item in runtimes if item.get('isAvailable') and '.iOS-' in item['identifier']]
    if not available:
        raise SystemExit('No available iOS simulator runtime.')
    runtime = max(available, key=lambda item: tuple(map(int, item['version'].split('.'))))
    parts = list(map(int, runtime['version'].split('.'))) + [0, 0]
    version = (parts[0] << 16) + (parts[1] << 8) + parts[2]
    devices = json.loads(simctl('list', 'devicetypes', '--json'))['devicetypes']
    phones = [item for item in devices if item['name'].startswith('iPhone')
              and item.get('minRuntimeVersion', 0) <= version <= item.get('maxRuntimeVersion', 0xFFFFFFFF)]
    if not phones:
        raise SystemExit('No iPhone simulator supports the installed iOS runtime.')
    phone = next((item for item in phones if item['name'] == 'iPhone 16'), phones[-1])
    device = simctl('create', 'aTerminal conversation CI', phone['identifier'], runtime['identifier'])
    output = ROOT / 'build/reconnect-evidence/ios-conversation.xcresult'
    output.parent.mkdir(parents=True, exist_ok=True)
    tests = [
        'ChatDeliveryUITests',
        'AuthorizationUITests',
        'WorkspaceUITests/testSessionAndGlobalDraftsStaySeparateAndReturnToList',
        'WorkspaceUITests/testClosedAndOfflineHistoryRemainReachableAndReadOnly',
        'WorkspaceUITests/testBackgroundClearsConnectionBusyAndIgnoresOldFailure',
    ]
    try:
        simctl('boot', device)
        subprocess.run(['xcrun', 'simctl', 'bootstatus', device, '-b'], check=True)
        # Select only offline fixtures; live-account/RPC suites require separate setup.
        command = ['xcodebuild', '-project', 'apps/ios/aTerminal.xcodeproj', '-scheme', 'aTerminal',
                   '-sdk', 'iphonesimulator', '-configuration', 'Debug',
                   '-destination', 'platform=iOS Simulator,id=' + device,
                   '-derivedDataPath', 'build/xcode-conversation', '-resultBundlePath', str(output),
                   '-jobs', '2', '-parallel-testing-enabled', 'NO', 'CODE_SIGN_IDENTITY=-',
                   *['-only-testing:aTerminalUITests/' + test for test in tests], 'test']
        return subprocess.run(command, cwd=ROOT, check=False).returncode
    finally:
        # Only the simulator created above is touched; preserve results on failure.
        subprocess.run(['xcrun', 'simctl', 'shutdown', device], check=False)
        subprocess.run(['xcrun', 'simctl', 'delete', device], check=False)


if __name__ == '__main__':
    raise SystemExit(main())
