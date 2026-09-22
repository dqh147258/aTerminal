#!/usr/bin/env python3
"""Link the P0 native SwiftUI/UIKit harness against the generated Rust static library."""
import argparse
import os
from pathlib import Path
import plistlib
import shutil
import subprocess

ROOT = Path(__file__).resolve().parent.parent
p = argparse.ArgumentParser()
p.add_argument('--simulator', action='store_true')
a = p.parse_args()
sdk = 'iphonesimulator' if a.simulator else 'iphoneos'
arch = 'x86_64' if a.simulator and os.uname().machine != 'arm64' else 'arm64'
rust_target = ('x86_64-apple-ios' if arch == 'x86_64' else 'aarch64-apple-ios-sim') if a.simulator else 'aarch64-apple-ios'
triple = arch + '-apple-ios15.0' + ('-simulator' if a.simulator else '')
sdkroot = subprocess.check_output(['xcrun','--sdk',sdk,'--show-sdk-path'], text=True).strip()
app = ROOT / 'build' / ('ios-simulator' if a.simulator else 'ios-device') / 'AITerminal.app'
app.mkdir(parents=True, exist_ok=True)
bindings = ROOT / 'build/bindings'
args = ['xcrun','--sdk',sdk,'swiftc','-D','DEBUG','-parse-as-library','-sdk',sdkroot,'-target',triple,
        '-module-cache-path',str(ROOT/'build/swift-module-cache'),
        '-I',str(bindings),'-Xcc','-fmodule-map-file='+str(bindings/'ai_terminal_mobileFFI.modulemap'),
        '-L',str(ROOT/'build/mobile'/rust_target),'-lai_terminal_mobile','-lc++',
        '-framework','UIKit','-framework','SwiftUI','-framework','Foundation','-framework','Security',
        str(bindings/'ai_terminal_mobile.swift'),
        *map(str,sorted((ROOT/'apps/ios/AITerminal').glob('*.swift'))),'-o',str(app/'AITerminal')]
subprocess.run(args,cwd=ROOT,check=True)
shutil.copy2(ROOT/'build/fixtures/screen.pb',app/'screen.pb')
info = {'CFBundleIdentifier':'dev.aiterminal.app','CFBundleName':'AI Terminal','CFBundleExecutable':'AITerminal',
        'CFBundlePackageType':'APPL','CFBundleShortVersionString':'0.1.0','CFBundleVersion':'1',
        'MinimumOSVersion':'15.0','LSRequiresIPhoneOS':True,'UIDeviceFamily':[1,2],
        'UILaunchScreen':{},'UIApplicationSceneManifest':{'UIApplicationSupportsMultipleScenes':False},
        'CFBundleSupportedPlatforms':['iPhoneSimulator' if a.simulator else 'iPhoneOS']}
with (app/'Info.plist').open('wb') as f: plistlib.dump(info,f)
if a.simulator:
    # This is a linker/render probe only. Use the Xcode project for Keychain and
    # app lifecycle tests; simulator entitlements also require linker metadata.
    subprocess.run(['codesign','--force','--sign','-',str(app)],check=True)
print(app)
if not a.simulator: print('Device build is unsigned. No device installation was attempted.')
