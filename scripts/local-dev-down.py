#!/usr/bin/env python3
"""Stop this project's LAN stack, preserving accounts, credentials and device identities."""

import argparse
from pathlib import Path
import subprocess
import sys


ROOT = Path(__file__).resolve().parent.parent
STATE = ROOT / ".local/local-dev/agent-next"
CLI = ROOT / "target/debug/aTerminal"
PACKAGE = "com.yxf.aterminal"

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--android-serial", default="127.0.0.1:62001")
parser.add_argument("--skip-android", action="store_true")
args = parser.parse_args()

errors = []


def stop(command, timeout):
    try:
        return subprocess.run(command, cwd=ROOT, capture_output=True, text=True, timeout=timeout)
    except (OSError, subprocess.TimeoutExpired) as error:
        errors.append(str(error))
        return None


if not args.skip_android:
    result = stop(["adb", "-s", args.android_serial, "shell", "am", "force-stop", PACKAGE], 20)
    if result is not None and result.returncode:
        errors.append("Android: " + result.stderr.strip())
    elif result is not None:
        print("Android app stopped; its saved login and device identity remain.")

if (STATE / "endpoint.json").exists():
    if not CLI.is_file():
        errors.append("Desktop: CLI binary is missing; could not stop the Agent")
    else:
        result = stop([str(CLI), "--state-dir", str(STATE), "--agent-stop"], 20)
        if result is not None and result.returncode and "no running Agent" not in result.stderr:
            errors.append("Desktop: " + result.stderr.strip())
        elif result is not None:
            print("Desktop Agent and its Shell processes stopped; saved login remains.")
else:
    print("Desktop Agent is already stopped.")

result = stop(["docker", "compose", "-p", "ai-terminal-dev", "-f", "deploy/compose.lan.yaml",
               "-f", "deploy/compose.test-network.yaml", "stop"], 90)
if result is not None and result.returncode:
    errors.append("Server: " + result.stderr.strip())
elif result is not None:
    print("Server and LAN TLS stopped; database, certificates and test account remain.")

if errors:
    print("\n".join(errors), file=sys.stderr)
    raise SystemExit(1)
