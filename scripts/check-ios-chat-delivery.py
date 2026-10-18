#!/usr/bin/env python3
"""Exercise production Foundation chat delivery state; source-only is not a native test."""
import argparse
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser()
parser.add_argument('--source-only', action='store_true')
args = parser.parse_args()
source = ROOT / 'apps/ios/aTerminal'
assistant = (source / 'AssistantModel.swift').read_text()
chat = (source / 'ChatPanel.swift').read_text()
question = (source / 'AgentAuthorizationView.swift').read_text()
project = (ROOT / 'apps/ios/aTerminal.xcodeproj/project.pbxproj').read_text()

assert 'AgentReadRetry.run(' in assistant and 'expectedEpoch: connection' in assistant
assert 'try composerStore.saveDelivery(delivery)' in assistant and 'try composerStore.confirm(delivery)' in assistant
assert 'requestID == delivery.requestID' in assistant and 'let delivery = AgentSendSnapshot(' in assistant
assert 'TextEditor(text: $model.draft)' in chat and 'chat.delivery.retry' in chat
assert 'model.draft).accessibilityIdentifier("chat.draft").disabled(model.submitting)' not in chat
assert 'private var canEdit: Bool' in question and '.disabled(!canEdit)' in question
assert 'AgentMessagePresentation.interrupted(kind: eventKind, value: value)' in assistant and 'chat.interrupted' in chat
assert 'AgentMessagePresentation.modelRetryStatus(response["model_retry"])' in assistant and 'model.modelRetryStatus' in chat
assert 'liveReply.receive(response)' in assistant and 'liveReply.reconcile(history:' in assistant and 'chat.live.status' in chat
assert assistant.count('historyGeneration.accept(nextGeneration, scopeKey: target.key)') == 2
assert 'func stop(preservingContext: Bool = false) {\n        saveDraft()' in assistant
assert project.count('AgentChatDelivery.swift') == 5
print('PASS: iOS chat delivery/UI source wiring (not native execution)')
if args.source_only:
    print('NOT RUN: Foundation behavior, iOS build and simulator/device tests')
else:
    compiler = ['xcrun', 'swiftc'] if shutil.which('xcrun') else [shutil.which('swiftc') or 'swiftc']
    if not shutil.which(compiler[0]):
        raise SystemExit('Swift compiler unavailable; --source-only provides limited source checks only.')
    out = ROOT / 'build/ios-chat-delivery-checks'
    out.mkdir(parents=True, exist_ok=True)
    binary = out / 'checks'
    subprocess.run([*compiler, '-parse-as-library', '-module-cache-path', str(out / 'swift-cache'),
                    str(source / 'WorkspaceRecovery.swift'), str(source / 'AgentChatDelivery.swift'),
                    str(ROOT / 'apps/ios/Tests/AgentChatDeliveryChecks.swift'), '-o', str(binary)], check=True)
    subprocess.run([str(binary)], check=True, timeout=30)
