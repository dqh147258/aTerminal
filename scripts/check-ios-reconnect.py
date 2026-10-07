#!/usr/bin/env python3
"""Check reconnect integration; run actual Foundation policy checks unless --source-only.

The source-only mode is useful on Linux but is not a native/SwiftUI test pass.
"""
import argparse
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser()
parser.add_argument('--source-only', action='store_true', help='Only check source wiring; do not claim native validation')
args = parser.parse_args()

def source(name):
    return (ROOT / 'apps/ios/aTerminal' / name).read_text()

def section(text, begin, end):
    return text.split(begin, 1)[1].split(end, 1)[0]

def check(value, message):
    if not value:
        raise AssertionError(message)

model = source('aTerminalApp.swift')
assistant = source('AssistantModel.swift')
auth = source('AgentAuthorization.swift')
view = source('WorkspaceScreen.swift')
surface = source('TerminalView.swift')
settings = source('SettingsDraft.swift')
recovery = section(model, 'private func preserveDisconnectedWorkspace', 'func heartbeat')
check('selected = nil' not in recovery and 'screen = nil' not in recovery and 'history = ""' not in recovery, 'Transient failure destroys workspace content')
check('historyLoading = false' in recovery and 'historyCursor = nil' in recovery and 'historyVersion += 1' in recovery, 'History cursor/loading is retained across channel replacement')
check('releaseHistory' not in recovery, 'A failed channel cursor is released on a replacement channel')
check('accountBusy = false' in recovery and 'busy = false' in recovery, 'Recovery leaves an invalidated account operation busy')
pause = section(model, 'func pause()', 'func loadRecentDirectories')
check('busy = false; accountBusy = false' in pause, 'Cold-start suspension leaves invalidated discovery busy')
check('establishedDevice == deviceID' in model, 'Global-only established workspace is not preserved')
check('controlAfterDisconnect(connected: connected, hasControl: hasControl' in recovery and 'resumeControl = control\n' not in model, 'Recovery reclaims requested rather than actual control')
connect = section(model, 'func connectDevice(', 'func revoke(')
check('WorkspaceRecovery.preservesContext' in connect and 'preparingWorkspace = !preserving' in connect, 'Connection retry still uses fullscreen navigation')
check('self.select(sessionID, control: self.resumeControl, recovering: true)' in connect, 'Recovery does not restore the exact session/control intent')
check('self.sessionExited = sessionID != nil' in connect, 'Missing old session navigates to a different terminal')
for method, end in [('func agent(', 'nonisolated private func observeTransportFailure'), ('func assistant(', '#if DEBUG\n    private func recordTestStatus')]:
    body = section(model, method, end)
    check('let version = generation' in body and 'self.channelState.matches(version)' in body, f'{method} does not fence queued RPC before dispatch')
for method in ['func text(', 'func paste(', 'func key(']:
    check('guard canInput' in model.split(method, 1)[1].split('\n', 1)[0], f'{method} accepts offline input')
check('workspace.reconnect' in view and 'safeAreaInset' in view, 'Missing nonblocking reconnect feedback')
background = section(view, 'if value == .background', 'if value == .active')
check('panel = nil' not in background and 'preservingContext: true' in background, 'Backgrounding discards navigation/agent context')
same_scope = section(assistant, 'if self.scope == scope {', 'saveDraft(); stop();')
check('reset()' not in same_scope and 'restoreDraft()' not in same_scope and 'historyTarget=nil' not in same_scope, 'Reconnect discards agent view/history/draft')
presentation_stop = section(assistant, 'func stop(preservingContext:', 'func switchScope')
check('if !preservingContext {' in presentation_stop and 'presentationEpoch += 1' in presentation_stop, 'Real navigation does not retire the presentation epoch')
check('context: "\\(destination?.key ?? ""):\\(presentationEpoch)"' in assistant, 'A-to-B-to-A navigation can resurrect an old question editor')
check('presentationEpoch' not in same_scope, 'Transport retry retires the live question editor')
check('scopeKey: destination?.key' in assistant and 'if !sameScope { pending = []; rules = []; requestIDs = [:] }' in auth, 'Uncertain approval IDs are lost across transport replacement')
check('link?.invalidate(); link = nil' in surface and 'coordinator.link == nil' in surface, 'Display link cannot restart after failed transport')
check('connectionEpoch() == connection' in settings and 'operationConnection == connection' in settings, 'Settings callbacks/queued work cross transport epochs')
creation = section(model, 'func create(', 'func closeSelected')
creation_sheet = section(view, 'private struct CreateTerminalSheet', 'private func directoryRow')
check('completion(.unconfirmed(terminalError(error)))' in creation, 'Post-dispatch creation failure is offered as safely retryable')
check('completion(.unavailable(' in creation, 'Known pre-dispatch failure is not distinct')
check('uncertainCreation = uncertainCreation || outcome.uncertain' in creation_sheet, 'Creation uncertainty is cleared on reconnect or later response')
check('uncertainCreation = true' in creation_sheet and 'submitting || uncertainCreation || !model.connected' in creation_sheet, 'Connection replacement allows a duplicate non-idempotent create')
check('guard !uncertainCreation else { return }' in creation_sheet, 'Queued duplicate create bypasses the disabled button')
check('uncertainCreation = false' not in creation_sheet.split('var body:', 1)[1], 'Same sheet clears uncertainty without user inspection')
check('label: "取消") { dismiss() }.disabled(submitting)' in creation_sheet, 'Uncertain create traps the user in the sheet')
project = (ROOT / 'apps/ios/aTerminal.xcodeproj/project.pbxproj').read_text()
check(project.count('WorkspaceRecovery.swift') == 5, 'Recovery policy is absent from Xcode source wiring')
print('PASS: iOS reconnect source wiring (not native execution)')
if args.source_only:
    print('NOT RUN: Swift Foundation behavior, iOS build and simulator/device tests')
else:
    compiler = ['xcrun', 'swiftc'] if shutil.which('xcrun') else [shutil.which('swiftc') or 'swiftc']
    if not shutil.which(compiler[0]):
        raise SystemExit('Swift compiler unavailable. Use --source-only for limited integration checks; native verification remains required.')
    out = ROOT / 'build/ios-reconnect-checks'
    out.mkdir(parents=True, exist_ok=True)
    binary = out / 'checks'
    subprocess.run([*compiler, '-parse-as-library', '-module-cache-path', str(out / 'swift-cache'),
                    str(ROOT / 'apps/ios/aTerminal/WorkspaceRecovery.swift'),
                    str(ROOT / 'apps/ios/Tests/WorkspaceRecoveryChecks.swift'), '-o', str(binary)], check=True)
    subprocess.run([str(binary)], check=True, timeout=30)
