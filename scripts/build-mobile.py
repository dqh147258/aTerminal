#!/usr/bin/env python3
"""Build native Rust artifacts without changing the user's global toolchain or SDK."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
p = argparse.ArgumentParser()
p.add_argument('platform', choices=['android', 'ios'])
p.add_argument('--toolchain', default='1.94.1')
p.add_argument('--ndk', type=Path)
p.add_argument('--webrtc', action='store_true')
p.add_argument('--release', action='store_true', help='Build optimized native code for on-device performance measurements')
p.add_argument('--simulator', action='store_true')
p.add_argument('--android-abi', choices=['arm64-v8a','x86_64','x86'], default='arm64-v8a')
a = p.parse_args()
env = os.environ.copy()
args = ['cargo', '+' + a.toolchain, 'build', '--locked', '-p', 'ai-terminal-mobile']
if a.release:
    args += ['--release']
if a.webrtc:
    args += ['--features', 'webrtc']
if a.platform == 'ios':
    target = 'aarch64-apple-ios' if not a.simulator else ('aarch64-apple-ios-sim' if os.uname().machine == 'arm64' else 'x86_64-apple-ios')
    env['IPHONEOS_DEPLOYMENT_TARGET'] = '15.0'
    sdk = 'iphonesimulator' if a.simulator else 'iphoneos'
    env['SDKROOT'] = subprocess.check_output(['xcrun', '--sdk', sdk, '--show-sdk-path'], text=True).strip()
else:
    if not a.ndk:
        p.error('--ndk must name a complete Android NDK installation (r28 or later)')
    ndk = a.ndk.resolve()
    hosts = list((ndk / 'toolchains/llvm/prebuilt').glob('*'))
    if not hosts:
        p.error('NDK has no LLVM host toolchain; installation may be incomplete')
    llvm = hosts[0]
    target = {'arm64-v8a': 'aarch64-linux-android', 'x86_64': 'x86_64-linux-android', 'x86': 'i686-linux-android'}[a.android_abi]
    target_env = target.replace('-', '_')
    cc = str(llvm / ('bin/' + target + '25-clang'))
    rustflags = '-C link-arg=-Wl,-z,max-page-size=16384'
    if a.android_abi == 'x86':
        resource_dir = Path(subprocess.check_output([str(llvm / 'bin/clang'), '--print-resource-dir'], text=True).strip())
        rustflags += ' -C link-arg=' + str(resource_dir / 'lib/linux/libclang_rt.builtins-i686-android.a')
    env.update({
        'ANDROID_NDK_HOME': str(ndk), 'ANDROID_NDK_ROOT': str(ndk),
        'AI_TERMINAL_ANDROID_ABI': a.android_abi,
        'CARGO_TARGET_' + target_env.upper() + '_LINKER': cc,
        'CC_' + target_env: cc,
        'CXX_' + target_env: cc + '++',
        'AR_' + target_env: str(llvm / 'bin/llvm-ar'),
        'CMAKE_TOOLCHAIN_FILE_' + target_env: str(ROOT / 'scripts/android-arm64.cmake'),
        'CARGO_TARGET_' + target_env.upper() + '_RUSTFLAGS': rustflags,
        'BINDGEN_EXTRA_CLANG_ARGS_' + target_env: '--sysroot=' + str(llvm / 'sysroot'),
        'PATH': str(llvm / 'bin') + os.pathsep + env['PATH'],
    })
args += ['--target', target, '--target-dir', str(ROOT / 'target' / ('mobile-' + a.platform))]
subprocess.run(args, cwd=ROOT, env=env, check=True)
source = ROOT / 'target' / ('mobile-' + a.platform) / target / ('release' if a.release else 'debug')
dest = ROOT / 'build/mobile' / target
dest.mkdir(parents=True, exist_ok=True)
name = 'libai_terminal_mobile.a' if a.platform == 'ios' else 'libai_terminal_mobile.so'
shutil.copy2(source / name, dest / name)
if a.platform == 'android' and a.webrtc:
    shutil.copy2(llvm / ('sysroot/usr/lib/' + target + '/libc++_shared.so'), dest / 'libc++_shared.so')
print(dest / name)
