#!/usr/bin/env python3
"""Configure and verify the persistent, authorized LAN trial in an iOS simulator."""
import argparse
import datetime
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import time

ROOT = Path(__file__).resolve().parent.parent
p = argparse.ArgumentParser()
p.add_argument('--build', action='store_true', help='Rebuild desktop, iOS Rust libraries, XCFramework, and Xcode app')
p.add_argument('--toolchain', default='1.94.1')
p.add_argument('--restore', action='store_true', help='Reconnect using the saved Keychain invitation, without importing it')
p.add_argument('--relay-only', action='store_true', help='Force WSS for the simulator verification run')
p.add_argument('--interactive', action='store_true', help='Open the saved session read-only without injecting a test command')
a = p.parse_args()
config = json.loads((ROOT/'deploy/ios-lan.json').read_text())
env = os.environ.copy()
env['AI_TERMINAL_CA_FILE'] = str(ROOT/config['ca_file'])
env['NO_PROXY'] = '127.0.0.1,localhost,192.168.0.36'
env['no_proxy'] = env['NO_PROXY']

def run(command, **kwargs):
    return subprocess.run(command, cwd=ROOT, env=env, check=True, **kwargs)

run(['curl','--fail','--silent','--show-error','--noproxy','*','--cacert',env['AI_TERMINAL_CA_FILE'],config['server']+'/healthz'])
udid = config['simulator_id']
devices = json.loads(subprocess.check_output(['xcrun','simctl','list','devices','available','-j'], text=True))
device = next((d for group in devices['devices'].values() for d in group if d['udid']==udid), None)
if not device: raise SystemExit('Configured simulator is unavailable; edit deploy/ios-lan.json with an available simulator UUID')
if device['state'] != 'Booted': run(['xcrun','simctl','boot',udid])
run(['xcrun','simctl','bootstatus',udid,'-b'])
if a.build:
    run(['cargo','+'+a.toolchain,'build','--locked','-p','ai-terminal','--bin','ai-terminal','--example','mobile_demo'])
    run(['python3','scripts/build-mobile.py','ios','--toolchain',a.toolchain,'--webrtc'])
    run(['python3','scripts/build-mobile.py','ios','--toolchain',a.toolchain,'--webrtc','--simulator'])
    run(['python3','scripts/prepare-bindings.py','--toolchain',a.toolchain])
    target = 'aarch64-apple-ios-sim' if platform.machine()=='arm64' else 'x86_64-apple-ios'
    run(['python3','scripts/package-ios.py','--simulator-target',target,'--replace'])
    run(['xcodebuild','-project','apps/ios/AITerminal.xcodeproj','-scheme','AITerminal',
         '-sdk','iphonesimulator','-destination','platform=iOS Simulator,id='+udid,
         '-configuration','Debug','-derivedDataPath','build/xcode','CODE_SIGNING_ALLOWED=YES','CODE_SIGN_IDENTITY=-','build'])

cli = ROOT/'target/debug/ai-terminal'
demo = ROOT/'target/debug/examples/mobile_demo'
state = ROOT/config['state_dir']
if not cli.exists() or not demo.exists(): raise SystemExit('Build the CLI and mobile_demo first, or pass --build')
run([str(demo),str(state),str(ROOT/config['admin_token_file']),str(cli),config['server']])
manifest = json.loads((state/'demo.json').read_text())
subprocess.run(['xcrun','simctl','terminate',udid,config['bundle_id']], capture_output=True)
run(['xcrun','simctl','install',udid,str(ROOT/config['app'])])
container = Path(subprocess.check_output(['xcrun','simctl','get_app_container',udid,config['bundle_id'],'data'],text=True).strip())
launch_env = env.copy()
if not a.restore:
    invitation = container/'Documents'/'lan-invitation.txt'
    invitation.parent.mkdir(parents=True,exist_ok=True)
    shutil.copy2(state/'invitation.txt',invitation)
    launch_env['SIMCTL_CHILD_AI_TERMINAL_TEST_INVITATION_FILE'] = str(invitation)
marker = 'IOS_LAN_7200_' + datetime.datetime.now().strftime('%Y%m%d_%H%M%S')
launch_env['SIMCTL_CHILD_AI_TERMINAL_TEST_INPUT_MARKER'] = marker
launch_env['SIMCTL_CHILD_AI_TERMINAL_TEST_SESSION_ID'] = manifest['session_id']
status_file = container/'Documents'/'lan-status.json'
status_file.unlink(missing_ok=True)
launch_env['SIMCTL_CHILD_AI_TERMINAL_TEST_STATUS_FILE'] = str(status_file)
launch_env['SIMCTL_CHILD_AI_TERMINAL_TEST_RELAY_ONLY'] = '1' if a.relay_only else '0'
launch=['xcrun','simctl','launch',udid,config['bundle_id'],'--connect-fixture']
if not a.interactive: launch.append('--input-fixture')
subprocess.run(launch,cwd=ROOT,env=launch_env,check=True)
deadline = time.monotonic()+45
verified = False
while time.monotonic()<deadline:
    probe_command=[str(demo),str(state),str(ROOT/config['admin_token_file']),str(cli),config['server']]
    if not a.interactive: probe_command.append(marker)
    probe = subprocess.run(probe_command,
                           cwd=ROOT,env=env,capture_output=True,text=True)
    if probe.returncode==0 and status_file.exists():
        status=json.loads(status_file.read_text())
        output_ok=a.interactive or (status.get('observed') and status.get('controlled'))
        if status.get('marker')==marker and output_ok and status.get('path')==('relay' if a.relay_only else 'direct'):
            print(probe.stdout.strip());verified=True;break
    time.sleep(0.5)
if not verified:
    if status_file.exists(): print(status_file.read_text())
    raise SystemExit('Simulator did not confirm the expected path/output within 45 seconds; inspect the app')
screenshots = ROOT/'build/screenshots'
screenshots.mkdir(exist_ok=True,parents=True)
name = 'ios-lan-' + ('relay' if a.relay_only else 'direct') + ('-restored' if a.restore else '') + ('-interactive' if a.interactive else '') + '.png'
run(['xcrun','simctl','io',udid,'screenshot',str(screenshots/name)])
subprocess.run(['open','-a','Simulator','--args','-CurrentDeviceUDID',udid],check=False)
print('PASS:',config['server'],'session='+manifest['session_id'],'path='+status['path'])
print('Trial remains running. Attach from desktop:')
print(str(cli)+' --state-dir '+str(state)+' --attach '+manifest['session_id'])
