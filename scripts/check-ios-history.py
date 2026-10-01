#!/usr/bin/env python3
"""Check the iOS history pipeline against the actual Swift sources and Rust AgentCache."""
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent
out = ROOT / 'build/ios-history-checks'
out.mkdir(parents=True, exist_ok=True)
library = ROOT / 'target/debug/deps/libai_terminal_mobile.rlib'
if not library.exists():
    subprocess.run(['cargo', 'build', '-p', 'ai-terminal-mobile', '--lib'], cwd=ROOT, check=True)
bridge = out / 'agent-cache-bridge'
subprocess.run(['rustc', '--edition', '2024', str(ROOT / 'apps/ios/Tests/AgentCacheBridge.rs'),
                '-L', 'dependency=' + str(ROOT / 'target/debug/deps'), '--extern', 'ai_terminal_mobile=' + str(library),
                '-o', str(bridge)], cwd=ROOT, check=True)
sources = [ROOT / 'apps/ios/aTerminal' / name for name in ['AgentHistoryCache.swift', 'AgentCacheSearch.swift', 'ChatStore.swift']]
binary = out / 'agent-history-checks'
subprocess.run(['xcrun', 'swiftc', '-parse-as-library', '-module-cache-path', str(out / 'swift-cache'),
                *map(str, sources), str(ROOT / 'apps/ios/Tests/AgentHistoryChecks.swift'), '-o', str(binary)], cwd=ROOT, check=True)
with tempfile.TemporaryDirectory(prefix='fixture-', dir=out) as fixture:
    subprocess.run([str(binary), str(bridge), fixture], cwd=ROOT, check=True, timeout=60)
(out / 'verification.json').write_text(json.dumps({'status': 'passed', 'scope': 'isolated host fixtures; no simulator or network',
    'sources': {str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest() for path in sources}}, indent=2) + '\n')
