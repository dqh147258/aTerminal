#!/usr/bin/env python3
"""Run isolated UI regressions and keep native screenshots even when a test fails."""
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parent.parent
classes = ','.join('com.yxf.aterminal.' + name for name in (
    'AgentAuthorizationUiTest', 'WorkspaceReconnectTest', 'ReconnectUiTest',
    'WorkspaceReviewRegressionTest', 'ConversationStabilityUiTest', 'ToolDetailsUiTest'))
result = subprocess.run([
    str(ROOT / 'apps/android/gradlew'), '-p', str(ROOT / 'apps/android'),
    '-PauthorizationUiFixture=true',
    '-Pandroid.testInstrumentationRunnerArguments.class=' + classes,
    ':app:connectedDebugAndroidTest', '--no-daemon'], cwd=ROOT, check=False)
output = ROOT / 'build/reconnect-evidence'
output.mkdir(parents=True, exist_ok=True)
archive = output / 'android-ui.tar'
package = 'com.yxf.aterminal.authorizationfixture'
listing = subprocess.run(['adb', 'exec-out', 'run-as', package, 'ls', '-1', 'files'],
                         capture_output=True, text=True, cwd=ROOT, check=False)
captures = [name for name in listing.stdout.splitlines()
            if name == 'reconnect-evidence' or re.fullmatch(r'tool-details-[A-Za-z0-9_.-]+\.png', name)]
exported_ok = False
if captures:
    with archive.open('wb') as stream:
        exported = subprocess.run([
            'adb', 'exec-out', 'run-as', package,
            'tar', '-C', 'files', '-cf', '-', *captures],
            stdout=stream, cwd=ROOT, check=False)
    exported_ok = exported.returncode == 0 and archive.stat().st_size > 0
if not exported_ok:
    archive.unlink(missing_ok=True)
    print('Native screenshot export unavailable; keep the instrumentation report as evidence.')
# Never turn a failing native test green because artifact collection succeeded.
raise SystemExit(result.returncode)
