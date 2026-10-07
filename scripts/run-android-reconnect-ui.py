#!/usr/bin/env python3
"""Run isolated UI regressions and keep native screenshots even when a test fails."""
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
classes = ','.join('com.yxf.aterminal.' + name for name in (
    'AgentAuthorizationUiTest', 'WorkspaceReconnectTest', 'ReconnectUiTest'))
result = subprocess.run([
    str(ROOT / 'apps/android/gradlew'), '-p', str(ROOT / 'apps/android'),
    '-PauthorizationUiFixture=true',
    '-Pandroid.testInstrumentationRunnerArguments.class=' + classes,
    ':app:connectedDebugAndroidTest', '--no-daemon'], cwd=ROOT, check=False)
output = ROOT / 'build/reconnect-evidence'
output.mkdir(parents=True, exist_ok=True)
archive = output / 'android-ui.tar'
with archive.open('wb') as stream:
    exported = subprocess.run([
        'adb', 'exec-out', 'run-as', 'com.yxf.aterminal.authorizationfixture',
        'tar', '-C', 'files', '-cf', '-', 'reconnect-evidence'],
        stdout=stream, cwd=ROOT, check=False)
if exported.returncode or archive.stat().st_size == 0:
    archive.unlink(missing_ok=True)
    print('Native screenshot export unavailable; keep the instrumentation report as evidence.')
# Never turn a failing native test green because artifact collection succeeded.
raise SystemExit(result.returncode)
