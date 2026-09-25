#!/usr/bin/env python3
"""Build one or more project artifacts without changing the active services."""

import argparse
import os
from pathlib import Path
import platform
import shlex
import subprocess


ROOT = Path(__file__).resolve().parent.parent
ORDER = ("desktop", "android", "ios", "server")
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("platforms", nargs="+", help="desktop, android, ios, server, or all; separate with spaces or commas")
parser.add_argument("--toolchain", default="stable")
parser.add_argument("--ndk", type=Path, help="Android NDK r28+; autodetected when omitted")
parser.add_argument("--dry-run", action="store_true", help="print the build steps without running them")
args = parser.parse_args()

requested = {name for value in args.platforms for name in value.split(",")}
if "all" in requested:
    requested.remove("all")
    requested.update(ORDER)
unknown = requested.difference(ORDER)
if unknown or not requested:
    parser.error("unknown platform: " + ", ".join(sorted(unknown or requested)))


def run(command):
    print("+", shlex.join(map(str, command)), flush=True)
    if not args.dry_run:
        subprocess.run(list(map(str, command)), cwd=ROOT, check=True)


def android_ndk():
    if args.ndk:
        candidates = [args.ndk]
    else:
        direct = [Path(value) for name in ("ANDROID_NDK_HOME", "ANDROID_NDK_ROOT") if (value := os.environ.get(name))]
        sdk = Path(os.environ.get("ANDROID_HOME") or os.environ.get("ANDROID_SDK_ROOT") or Path.home() / "Library/Android/sdk")
        candidates = direct + sorted((sdk / "ndk").glob("*"), reverse=True)
    for candidate in candidates:
        version_file = candidate / "source.properties"
        if (candidate / "toolchains/llvm/prebuilt").is_dir() and version_file.is_file():
            revision = next((line.partition("=")[2].strip() for line in version_file.read_text().splitlines()
                             if line.startswith("Pkg.Revision")), "0")
            if int(revision.split(".")[0]) >= 28:
                return candidate.resolve()
    parser.error("Android NDK r28+ was not found; pass --ndk /path/to/ndk")


if "desktop" in requested:
    run(["cargo", "+" + args.toolchain, "build", "--locked", "-p", "ai-terminal", "--bin", "aTerminal"])

if "android" in requested:
    ndk = android_ndk()
    for abi in ("arm64-v8a", "x86_64", "x86"):
        run(["python3", "scripts/build-mobile.py", "android", "--toolchain", args.toolchain,
             "--webrtc", "--android-abi", abi, "--ndk", ndk])

if "ios" in requested:
    if platform.system() != "Darwin":
        parser.error("iOS artifacts require macOS and Xcode")
    run(["python3", "scripts/build-mobile.py", "ios", "--toolchain", args.toolchain, "--webrtc"])
    run(["python3", "scripts/build-mobile.py", "ios", "--toolchain", args.toolchain, "--webrtc", "--simulator"])

if requested.intersection(("android", "ios")):
    run(["python3", "scripts/prepare-bindings.py", "--toolchain", args.toolchain])

if "android" in requested:
    run(["./apps/android/gradlew", "-p", "apps/android", ":app:assembleDebug",
         ":app:assembleDebugAndroidTest", ":app:lintDebug", "--offline"])
    print("Android APK: apps/android/app/build/outputs/apk/debug/app-debug.apk")

if "ios" in requested:
    simulator_target = "aarch64-apple-ios-sim" if platform.machine() == "arm64" else "x86_64-apple-ios"
    architecture = "arm64" if platform.machine() == "arm64" else "x86_64"
    run(["python3", "scripts/package-ios.py", "--simulator-target", simulator_target, "--replace"])
    run(["xcodebuild", "-project", "apps/ios/aTerminal.xcodeproj", "-scheme", "aTerminal",
         "-sdk", "iphonesimulator", "-configuration", "Debug", "-derivedDataPath", "build/xcode",
         "ARCHS=" + architecture, "CODE_SIGN_IDENTITY=-", "build"])
    print("iOS App: build/xcode/Build/Products/Debug-iphonesimulator/aTerminal.app")

if "server" in requested:
    run(["docker", "compose", "-p", "ai-terminal-dev", "-f", "deploy/compose.local.yaml", "build", "server"])
    print("Server image: ai-terminal-server:development")

print("Build selection complete:", ", ".join(name for name in ORDER if name in requested))
