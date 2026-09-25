#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if [ ! -x "$repo_root/target/debug/aTerminal" ]; then
    echo "Build the Desktop CLI first: cargo +stable build --locked -p ai-terminal --bin aTerminal" >&2
    exit 1
fi

export AI_TERMINAL_CREDENTIAL_STORE=file
export XDG_CONFIG_HOME="$repo_root/.local/local-dev/config-next"
exec "$repo_root/target/debug/aTerminal" --state-dir "$repo_root/.local/local-dev/agent-next" auth status
