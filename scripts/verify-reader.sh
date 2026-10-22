#!/bin/sh
# Recorded scenarios use local HTTP only. Live provider smoke remains separate.
set -eu
cd "$(dirname "$0")/.."
state=$(mktemp -d "${TMPDIR:-/tmp}/hn-reader-checks.XXXXXX")
trap 'rm -rf "$state"' EXIT HUP INT TERM
export XDG_CONFIG_HOME="$state/config" XDG_DATA_HOME="$state/data" XDG_STATE_HOME="$state/state"
unset OPENROUTER_API_KEY ANTHROPIC_API_KEY
cargo fmt --check
HTTP_PROXY=http://127.0.0.1:9 HTTPS_PROXY=http://127.0.0.1:9 ALL_PROXY=http://127.0.0.1:9 NO_PROXY=127.0.0.1,localhost cargo test
uv run --with pyte python scripts/replay_reader.py "$@"
