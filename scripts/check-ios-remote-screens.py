#!/usr/bin/env python3
"""Run actual iOS screen parsing/polling model on macOS, with in-memory RPC responses.

No generated FFI substitutes, app install, account data or real Desktop capture.
"""
from pathlib import Path
import json
import subprocess

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / 'build/ios-remote-screen-checks'
OUT.mkdir(parents=True, exist_ok=True)
binary = OUT / 'checks'
production = ROOT / 'apps/ios/aTerminal/RemoteScreenModel.swift'
checks = ROOT / 'apps/ios/Tests/RemoteScreenChecks.swift'
subprocess.run(['xcrun', 'swiftc', '-parse-as-library', '-module-cache-path', str(OUT / 'swift-cache'),
                str(production), str(checks), '-o', str(binary)], check=True, cwd=ROOT)
subprocess.run([str(binary)], check=True, timeout=30, cwd=ROOT)

project = ROOT / 'apps/ios/aTerminal.xcodeproj/project.pbxproj'
objects = json.loads(subprocess.check_output(['plutil', '-convert', 'json', '-o', '-', str(project)]))['objects']
for name in ['RemoteScreenModel.swift', 'RemoteScreensPanel.swift', 'RemoteScreenFixture.swift']:
    file_id = next(key for key, value in objects.items() if value.get('isa') == 'PBXFileReference' and value.get('path') == name)
    build_id = next(key for key, value in objects.items() if value.get('isa') == 'PBXBuildFile' and value.get('fileRef') == file_id)
    assert any(build_id in value.get('files', []) for value in objects.values() if value.get('isa') == 'PBXSourcesBuildPhase'), f'{name} missing build source'
    assert any(file_id in value.get('children', []) for value in objects.values() if value.get('isa') == 'PBXGroup'), f'{name} missing source group'
print('PASS: Xcode source wiring; NOT RUN: app build, Simulator UI execution or Desktop capture')
