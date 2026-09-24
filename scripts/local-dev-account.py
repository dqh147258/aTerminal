#!/usr/bin/env python3
"""Create or show the reusable account for the project-local LAN deployment."""

import argparse
import json
import os
from pathlib import Path
import secrets
import ssl
import urllib.parse
import urllib.request


ROOT = Path(__file__).resolve().parent.parent
SERVER = "https://192.168.0.36:7200"
USERNAME = "aiterminal_local_test"
PRIVATE = ROOT / ".local" / "local-dev"
ACCOUNT = PRIVATE / "account.json"
ADMIN_TOKEN = ROOT / "deploy" / "secrets" / "admin-token"
CA = ROOT / "deploy" / "secrets" / "lan-ca.crt"


def admin_request(method, path, payload=None):
    token = ADMIN_TOKEN.read_text().strip()
    body = None if payload is None else json.dumps(payload).encode()
    request = urllib.request.Request(
        SERVER + path,
        data=body,
        headers={
            "Authorization": "Bearer " + token,
            "X-Admin-Request": "1",
            "Content-Type": "application/json",
        },
        method=method,
    )
    with urllib.request.urlopen(request, context=ssl.create_default_context(cafile=str(CA)), timeout=10) as response:
        return json.load(response)


def save_account(account):
    PRIVATE.mkdir(mode=0o700, parents=True, exist_ok=True)
    PRIVATE.chmod(0o700)
    temporary = PRIVATE / "account.json.tmp"
    descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    try:
        with os.fdopen(descriptor, "w") as output:
            json.dump(account, output)
            output.write("\n")
        temporary.replace(ACCOUNT)
    finally:
        temporary.unlink(missing_ok=True)


def ensure():
    saved = json.loads(ACCOUNT.read_text()) if ACCOUNT.exists() else None
    if saved is not None and saved.get("username") != USERNAME:
        raise SystemExit("Private account file has a different username; refusing to replace it")
    query = urllib.parse.urlencode({"q": USERNAME, "limit": 100})
    users = admin_request("GET", "/v2/admin/users?" + query)["items"]
    matches = [user for user in users if user["username"] == USERNAME]
    if len(matches) > 1:
        raise SystemExit("Duplicate local test usernames; inspect Admin before continuing")
    if matches:
        if saved is None or saved.get("user_id") != matches[0]["id"]:
            raise SystemExit("Test account exists but local credentials do not match; reset it in Admin")
        print(f"Ready: {USERNAME} on {SERVER}; password: {ACCOUNT} (run `show` to display it)")
        return
    password = saved["password"] if saved is not None else secrets.token_urlsafe(24)
    user = admin_request("POST", "/v2/admin/users", {"username": USERNAME, "password": password})
    save_account({"server": SERVER, "username": USERNAME, "password": password, "user_id": user["id"]})
    print(f"Created: {USERNAME} on {SERVER}; password: {ACCOUNT} (run `show` to display it)")


def show():
    if not ACCOUNT.exists():
        raise SystemExit("No local test credentials; start the server and run `ensure` first")
    account = json.loads(ACCOUNT.read_text())
    print("Server:", account["server"])
    print("Username:", account["username"])
    print("Password:", account["password"])


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("ensure", "show"))
    args = parser.parse_args()
    try:
        (ensure if args.action == "ensure" else show)()
    except (OSError, ValueError, KeyError) as error:
        raise SystemExit(f"Local test account unavailable: {error}") from error
