#!/usr/bin/env python3
"""Test the production iOS authorization state machine with isolated in-memory RPC."""
from pathlib import Path
import subprocess
ROOT = Path(__file__).resolve().parent.parent
out = ROOT / 'build/ios-authorization-checks'
out.mkdir(parents=True, exist_ok=True)
binary = out / 'checks'
subprocess.run(['xcrun', 'swiftc', '-parse-as-library', '-module-cache-path', str(out / 'swift-cache'),
                str(ROOT / 'apps/ios/aTerminal/AgentAuthorization.swift'),
                str(ROOT / 'apps/ios/Tests/AgentAuthorizationChecks.swift'), '-o', str(binary)], check=True)
subprocess.run([str(binary)], check=True, timeout=30)
