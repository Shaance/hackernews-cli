#!/bin/sh
# Regenerate the README screenshots and demo GIF with vhs (brew install vhs).
# Uses live Hacker News and an OpenRouter key from the environment or .env.local
# (two summaries, a few cents). Settings and saved stories live in a throwaway directory.
set -eu
cd "$(dirname "$0")/.."
command -v vhs >/dev/null || { echo "vhs is required: brew install vhs" >&2; exit 1; }
command -v ffmpeg >/dev/null || { echo "ffmpeg is required: brew install ffmpeg" >&2; exit 1; }
if [ -z "${OPENROUTER_API_KEY:-}" ] && [ -f .env.local ]; then
    OPENROUTER_API_KEY=$(sed -n 's/^OPENROUTER_API_KEY=//p' .env.local | tr -d "\"'")
fi
[ -n "${OPENROUTER_API_KEY:-}" ] || { echo "Set OPENROUTER_API_KEY or add it to .env.local" >&2; exit 1; }
export OPENROUTER_API_KEY
unset ANTHROPIC_API_KEY
state=$(mktemp -d "${TMPDIR:-/tmp}/hn-readme.XXXXXX")
trap 'rm -rf "$state"' EXIT HUP INT TERM
export XDG_CONFIG_HOME="$state/config" XDG_DATA_HOME="$state/data" XDG_STATE_HOME="$state/state"
cargo build --release
for tape in scripts/readme/dark.tape scripts/readme/light.tape; do
    vhs "$tape"
done
# Cut stills from the recordings; times follow the visible Sleeps in each tape.
still() { ffmpeg -loglevel error -y -ss "$2" -i "$1" -frames:v 1 "docs/screenshots/$3"; }
still docs/screenshots/demo.gif 0.9 feed.png
still docs/screenshots/demo.gif 2.9 reader.png
still docs/screenshots/demo.gif 4.3 citation.png
still docs/screenshots/demo.gif 9.5 help.png
still docs/screenshots/light.gif 1.0 reader-light.png
rm -f docs/screenshots/light.gif
ls -l docs/screenshots
