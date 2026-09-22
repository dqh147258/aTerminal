#!/usr/bin/env python3
"""Package already-built device/simulator libraries and generated FFI headers."""
from pathlib import Path
import argparse
import shutil
import subprocess
import tempfile

root = Path(__file__).resolve().parent.parent
p = argparse.ArgumentParser()
p.add_argument('--simulator-target', choices=['x86_64-apple-ios','aarch64-apple-ios-sim'], required=True)
p.add_argument('--replace', action='store_true', help='Replace the generated XCFramework after a successful rebuild')
a = p.parse_args()
headers = root/'build/xcframework-headers'
headers.mkdir(parents=True,exist_ok=True)
shutil.copy2(root/'build/bindings/ai_terminal_mobileFFI.h',headers/'ai_terminal_mobileFFI.h')
shutil.copy2(root/'build/bindings/ai_terminal_mobileFFI.modulemap',headers/'module.modulemap')
output = root/'build/AITerminalCore.xcframework'
if output.exists() and not a.replace:
    raise SystemExit('Output already exists; choose a clean packaging directory before rebuilding')
command=['xcodebuild','-create-xcframework']
for target in ['aarch64-apple-ios',a.simulator_target]:
    command+=['-library',str(root/'build/mobile'/target/'libai_terminal_mobile.a'),'-headers',str(headers)]
with tempfile.TemporaryDirectory(prefix='xcframework-',dir=root/'build') as temp:
    staged=Path(temp)/'AITerminalCore.xcframework'
    subprocess.run(command+['-output',str(staged)],check=True)
    if output.exists(): shutil.rmtree(output)
    shutil.move(str(staged),str(output))
shutil.copy2(root/'build/bindings/ai_terminal_mobile.swift',root/'build/ai_terminal_mobile.swift')
print(output)
