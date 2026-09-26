#!/usr/bin/env python3
"""Stop this project's local trial without removing certificates, pairings, or Docker volumes."""
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
config = json.loads((ROOT/'deploy/ios-lan.json').read_text())
state = ROOT/config['state_dir']
errors = []

try:
    devices = json.loads(subprocess.check_output(['xcrun','simctl','list','devices','available','-j'], text=True))
    device = next((d for group in devices['devices'].values() for d in group if d['udid']==config['simulator_id']), None)
    if device and device['state'] != 'Shutdown':
        subprocess.run(['xcrun','simctl','terminate',config['simulator_id'],config['bundle_id']], capture_output=True)
        subprocess.run(['xcrun','simctl','shutdown',config['simulator_id']], check=True)
    remaining = json.loads(subprocess.check_output(['xcrun','simctl','list','devices','available','-j'], text=True))
    if not any(d['state'] != 'Shutdown' for group in remaining['devices'].values() for d in group):
        subprocess.run(['osascript','-e','if application "Simulator" is running then tell application "Simulator" to quit'], check=True)
    print('Project iOS simulator stopped; other simulators were left alone.')
except (OSError, subprocess.SubprocessError, ValueError) as error:
    errors.append('Simulator: '+str(error))

try:
    if ((state/'runtime/endpoint.json').exists() or (state/'endpoint.json').exists()):
        subprocess.run([str(ROOT/'target/debug/aTerminal'),'--state-dir',str(state),'--agent-stop'], cwd=ROOT, check=True, timeout=15)
    # Shell processes cannot survive a stopped Agent. Keep pairing, but do not let
    # the next run attempt to attach to a session ID from the previous process.
    if (state/'demo.json').exists():
        (state/'demo.json').replace(state/'demo.last.json')
    print('Trial Agent stopped; previous session metadata archived, pairing retained.')
except (OSError, subprocess.SubprocessError) as error:
    errors.append('Agent: '+str(error))

try:
    subprocess.run(['docker','compose','-p','ai-terminal-dev','-f','deploy/compose.lan.yaml',
                    '-f','deploy/compose.test-network.yaml','stop'], cwd=ROOT, check=True, timeout=60)
    print('Project containers stopped; data volumes and configuration retained.')
except (OSError, subprocess.SubprocessError) as error:
    errors.append('Docker: '+str(error))

if errors:
    print('\n'.join(errors), file=sys.stderr)
    raise SystemExit(1)
