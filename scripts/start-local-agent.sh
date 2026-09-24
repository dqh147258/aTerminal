#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if [ ! -x "$repo_root/target/debug/ai-terminal" ]; then
    echo "Build the Desktop CLI first: cargo +stable build --locked -p ai-terminal --bin ai-terminal" >&2
    exit 1
fi

export AI_TERMINAL_CREDENTIAL_STORE=file
export XDG_CONFIG_HOME="$repo_root/.local/local-dev/config"
exec "$repo_root/target/debug/ai-terminal" --state-dir "$repo_root/.local/local-dev/agent" auth status
