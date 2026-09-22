#!/usr/bin/env python3
import argparse
import os
from pathlib import Path
import shutil
import subprocess

ROOT=Path(__file__).resolve().parent.parent
p=argparse.ArgumentParser()
p.add_argument('--toolchain',default='1.94.1')
a=p.parse_args()
env=os.environ.copy();env['RUSTUP_TOOLCHAIN']=a.toolchain
subprocess.run(['cargo','build','--locked','-p','ai-terminal-mobile','-p','ai-terminal-bindgen'],cwd=ROOT,env=env,check=True)
suffix='dylib' if os.uname().sysname=='Darwin' else 'so'
subprocess.run([str(ROOT/'target/debug/ai-terminal-bindgen'),'generate','--library',str(ROOT/f'target/debug/libai_terminal_mobile.{suffix}'),
                '--language','kotlin','--language','swift','--out-dir',str(ROOT/'build/bindings'),'--no-format','--metadata-no-deps'],cwd=ROOT,env=env,check=True)
(ROOT/'build/fixtures').mkdir(parents=True,exist_ok=True)
subprocess.run(['cargo','run','--locked','-p','ai-terminal-engine','--example','fixture','--','build/fixtures/screen.pb'],cwd=ROOT,env=env,check=True)
for target,abi in [('aarch64-linux-android','arm64-v8a'),('x86_64-linux-android','x86_64')]:
    source=ROOT/'build/mobile'/target/'libai_terminal_mobile.so'
    if source.exists():
        dest=ROOT/'build/android-jni'/abi;dest.mkdir(parents=True,exist_ok=True)
        for library in source.parent.glob('*.so'): shutil.copy2(library,dest/library.name)
