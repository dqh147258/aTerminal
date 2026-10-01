#!/usr/bin/env python3
"""Run host checks against the actual Foundation-only Swift production source.

No source copying, application launch, simulator, account, Keychain or network.
Use --source-root for a read-only implementation worktree before Git integration.
"""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-root', type=Path, default=ROOT)
    parser.add_argument('--ui-typecheck', action='store_true', help='Also typecheck XCTest source with the simulator SDK; does not launch a simulator')
    args = parser.parse_args()
    production = args.source_root.resolve() / 'apps/ios/aTerminal/SettingsDraft.swift'
    checks = ROOT / 'apps/ios/Tests/AgentSettingsChecks.swift'
    if not production.is_file():
        parser.error('Production SettingsDraft.swift missing; pass --source-root to the implementation checkout')
    output = ROOT / 'build/ios-parity-checks/settings'
    output.mkdir(parents=True, exist_ok=True)
    binary = output / 'agent-settings-checks'
    report_path = output / 'verification.json'
    source_digest = hashlib.sha256(production.read_bytes()).hexdigest()
    checks_digest = hashlib.sha256(checks.read_bytes()).hexdigest()
    report = {'source': str(production), 'production_sha256': source_digest,
              'checks_sha256': checks_digest, 'status': 'compiling',
              'scope': 'host production logic; in-memory transport and temporary Skill package; no RPC/simulator'}
    report_path.write_text(json.dumps(report, indent=2) + '\n')
    command = ['xcrun', 'swiftc', '-parse-as-library', '-module-cache-path', str(output / 'swift-cache'),
               str(production), str(checks), '-o', str(binary)]
    subprocess.run(command, check=True, cwd=ROOT)
    if hashlib.sha256(production.read_bytes()).hexdigest() != source_digest:
        report['status'] = 'source changed during compilation; rerun after implementation settles'
        report_path.write_text(json.dumps(report, indent=2) + '\n')
        return 2
    # A hung async fixture cannot hang a coordinator indefinitely.
    result = subprocess.run([str(binary)], cwd=ROOT, timeout=30)
    report['exit_code'] = result.returncode
    report['status'] = 'passed' if result.returncode == 0 else 'failed'
    if args.ui_typecheck:
        sdk = Path(subprocess.check_output(['xcrun', '--sdk', 'iphonesimulator', '--show-sdk-path'], text=True).strip())
        developer = sdk.parents[1]
        arch = 'arm64' if platform.machine() == 'arm64' else 'x86_64'
        ui_command = ['xcrun', '--sdk', 'iphonesimulator', 'swiftc', '-typecheck', '-target', arch + '-apple-ios15.0-simulator',
                      '-sdk', str(sdk), '-F', str(developer / 'Library/Frameworks'), '-I', str(developer / 'usr/lib'),
                      '-module-cache-path', str(output / 'xctest-cache'), str(ROOT / 'apps/ios/UITests/WorkspaceUITests.swift')]
        ui_result = subprocess.run(ui_command, cwd=ROOT)
        report['ui_typecheck_exit_code'] = ui_result.returncode
        if ui_result.returncode:
            report['status'] = 'failed'
        else:
            print('PASS: offline WorkspaceUITests simulator-SDK typecheck; no simulator launched')
    if hashlib.sha256(production.read_bytes()).hexdigest() != source_digest:
        report['status'] += '; implementation source changed after compilation'
    report_path.write_text(json.dumps(report, indent=2) + '\n')
    return result.returncode or report.get('ui_typecheck_exit_code', 0)


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired):
        sys.exit('Host checks failed; no app or simulator was launched')
