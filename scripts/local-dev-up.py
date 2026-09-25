#!/usr/bin/env python3
"""Start the LAN stack and reuse one Desktop/Android test-account identity."""

import argparse
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import time


ROOT = Path(__file__).resolve().parent.parent
CLI = ROOT / "target/debug/aTerminal"
STATE = ROOT / ".local/local-dev/agent-next"
CONFIG = ROOT / ".local/local-dev/config-next"
ACCOUNT = ROOT / ".local/local-dev/account.json"
SERVER = "https://192.168.0.36:7200"
DESKTOP_NAME = "Local Desktop 新版"
PACKAGE = "com.yxf.aterminal"
WRAPPER_MARKER = "# Managed by aTerminal scripts/local-dev-up.py"

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--android-serial", default="127.0.0.1:62001")
parser.add_argument("--skip-android", action="store_true", help="start Server and Desktop only")
parser.add_argument("--build-android", action="store_true", help="rebuild every Android ABI and APK before installing")
args = parser.parse_args()


def run(command, **kwargs):
    kwargs.setdefault("check", True)
    return subprocess.run(list(map(str, command)), cwd=ROOT, **kwargs)


def install_command():
    target = Path.home() / ".cargo/bin/aTerminal"
    target.parent.mkdir(parents=True, exist_ok=True)
    if target.exists() or target.is_symlink():
        if target.is_symlink() or WRAPPER_MARKER not in target.read_text(errors="replace"):
            raise RuntimeError(f"{target} already exists and is not this project's wrapper")
    quoted_cli = shlex.quote(str(CLI))
    content = f"""#!/bin/sh
{WRAPPER_MARKER}
export AI_TERMINAL_CREDENTIAL_STORE=file
export XDG_CONFIG_HOME={shlex.quote(str(CONFIG))}
for argument in "$@"; do
    case "$argument" in
        --state-dir|--state-dir=*) exec {quoted_cli} "$@" ;;
    esac
done
exec {quoted_cli} --state-dir {shlex.quote(str(STATE))} "$@"
"""
    temporary = target.with_name("aTerminal.new")
    descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o755)
    try:
        with os.fdopen(descriptor, "w") as output:
            output.write(content)
        os.chmod(temporary, 0o755)
        temporary.replace(target)
    finally:
        temporary.unlink(missing_ok=True)
    if str(target.parent) not in os.environ.get("PATH", "").split(os.pathsep):
        raise RuntimeError(f"Add {target.parent} to PATH before using aTerminal")
    print("CLI command:", target)


def start_desktop(credentials):
    env = os.environ.copy()
    env["AI_TERMINAL_CREDENTIAL_STORE"] = "file"
    env["XDG_CONFIG_HOME"] = str(CONFIG)
    command = [CLI, "--state-dir", STATE]
    status = run(command + ["auth", "status"], env=env, capture_output=True, text=True).stdout.strip()
    if status == "Not logged in":
        run(command + ["auth", "login", "--server", SERVER, "--username", credentials["username"],
                       "--name", DESKTOP_NAME, "--ca-file", ROOT / "deploy/secrets/lan-ca.crt", "--password-stdin"],
            env=env, input=credentials["password"] + "\n", text=True, capture_output=True)
        status = run(command + ["auth", "status"], env=env, capture_output=True, text=True).stdout.strip()
    if credentials["username"] not in status or SERVER not in status:
        raise RuntimeError(f"Desktop has another account or server: {status}")
    deadline = time.monotonic() + 15
    while True:
        devices = json.loads(run(command + ["devices", "list"], env=env, capture_output=True, text=True).stdout)
        current = [device for device in devices if device["platform"] == "desktop" and device["current"]]
        if len(current) != 1 or current[0]["online"] or time.monotonic() >= deadline:
            break
        time.sleep(0.5)
    if len(current) != 1 or current[0]["name"] != DESKTOP_NAME or not current[0]["online"]:
        raise RuntimeError("Canonical Desktop identity is not online; inspect `aTerminal devices list`")
    others = [device["name"] for device in devices if device["platform"] == "desktop" and device["online"] and not device["current"]]
    if others:
        raise RuntimeError("Other Desktop Agents are online: " + ", ".join(others) + ". Stop them before starting one-device mode.")
    print("Desktop online:", DESKTOP_NAME, "(existing device identity reused)")


def adb(serial, *command, **kwargs):
    return run(["adb", "-s", serial, *command], **kwargs)


def android_apk_is_current():
    output = ROOT / "apps/android/app/build/outputs/apk/debug"
    if not (output / "app-debug.apk").is_file():
        return False
    try:
        metadata = json.loads((output / "output-metadata.json").read_text())
    except (OSError, ValueError):
        return False
    return metadata.get("applicationId") == PACKAGE


def start_android(credentials):
    serial = args.android_serial
    listing = run(["adb", "devices"], capture_output=True, text=True).stdout
    if f"{serial}\tdevice" not in listing:
        raise RuntimeError(f"Android {serial} is not in `device` state; Server and Desktop are already running")
    if adb(serial, "reverse", "--list", capture_output=True, text=True).stdout.strip():
        raise RuntimeError("adb reverse is active; remove it before real LAN testing")
    apk = ROOT / "apps/android/app/build/outputs/apk/debug/app-debug.apk"
    if not apk.is_file():
        raise RuntimeError("Android APK is missing; run `python3 scripts/build-artifacts.py android`")
    adb(serial, "install", "-r", apk, capture_output=True)
    adb(serial, "shell", "run-as", PACKAGE, "mkdir", "-p", "files", capture_output=True)
    adb(serial, "shell", f"run-as {PACKAGE} sh -c 'cat > files/acceptance-ca.pem'",
        input=(ROOT / "deploy/secrets/lan-ca.crt").read_bytes(), capture_output=True)
    adb(serial, "shell", "run-as", PACKAGE, "rm", "-f", "files/local-debug-status.json", capture_output=True)
    fixture = {"server": SERVER, "username": credentials["username"], "password": credentials["password"]}
    try:
        adb(serial, "shell", f"run-as {PACKAGE} sh -c 'cat > files/local-debug-login.json'",
            input=json.dumps(fixture).encode(), capture_output=True)
        adb(serial, "shell", "am", "force-stop", PACKAGE, capture_output=True)
        adb(serial, "shell", "am", "start", "-W", "-n", PACKAGE + "/.MainActivity",
            "--ez", "local_debug_autologin", "true",
            capture_output=True, timeout=20)
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            response = adb(serial, "exec-out", "run-as", PACKAGE, "cat", "files/local-debug-status.json",
                           capture_output=True, check=False)
            if response.returncode == 0 and response.stdout:
                try:
                    status = json.loads(response.stdout)
                except ValueError:
                    status = {}
                if status.get("state") == "ready":
                    print("Android connected:", serial, "→", DESKTOP_NAME)
                    return
                if status.get("state") == "error":
                    raise RuntimeError("Android auto login: " + status.get("message", "unknown error"))
            time.sleep(0.5)
        raise RuntimeError("Android auto login timed out; inspect the app and `adb logcat`")
    finally:
        cleanup = adb(serial, "shell", "run-as", PACKAGE, "rm", "-f", "files/local-debug-login.json",
                      capture_output=True, check=False)
        if cleanup.returncode:
            print("Warning: could not remove Android's temporary local-debug-login.json", file=sys.stderr)


try:
    run(["python3", "scripts/prepare-lan.py"])
    run(["python3", "scripts/local-dev-account.py", "ensure"])
    run(["python3", "scripts/build-artifacts.py", "desktop"])
    if args.build_android or not android_apk_is_current():
        run(["python3", "scripts/build-artifacts.py", "android"])
    install_command()
    credentials = json.loads(ACCOUNT.read_text())
    if credentials.get("server") != SERVER or credentials.get("username") != "aiterminal_local_test":
        raise RuntimeError("Private test-account file does not match this LAN deployment")
    start_desktop(credentials)
    if not args.skip_android:
        start_android(credentials)
    print("Ready: run `aTerminal` in any terminal window; use `aTerminal --list` to see sessions.")
except (OSError, ValueError, KeyError, RuntimeError, subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
    print("Local start incomplete:", error, file=sys.stderr)
    raise SystemExit(1)
