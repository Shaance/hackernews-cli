#!/bin/sh
# Prove the article pane is the story's live page: launch the window, open a real HN story, and
# capture it.
#
# Capture method: `screencapture -l <CGWindowID>` returns an empty buffer for a Metal-backed
# window in this environment, so this captures the whole screen and crops to the window frame
# reported by `scripts/window-frame.swift` (CGWindowListCopyWindowInfo, not accessibility-gated).
# `scripts/crop.py` does the crop and asserts the pane holds a rendered page rather than the
# app's own background.
#
# The story is opened by `--open-story <id>`, not by a key press: this machine denies osascript
# assistive access, so nothing outside the process can press a key. `--on-top` keeps the window
# above other apps so the fullscreen capture is of this window.
#
# The run is hermetic: XDG_CONFIG_HOME, XDG_DATA_HOME and XDG_STATE_HOME point at a temp
# directory, so it never reads the reader's summary key (and so never calls a provider), never
# writes their saved list, and never overwrites their remembered window size.
#
# Usage: scripts/verify-live.sh [story-id]
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
out="$here/verification"
bin="$here/target/debug/hn-desktop"
mkdir -p "$out"

# The story to open: the argument, or the top story on HN right now.
id=${1:-}
if [ -z "$id" ]; then
    id=$(curl -fsS https://hacker-news.firebaseio.com/v0/topstories.json \
        | python3 -c 'import json,sys; print(json.load(sys.stdin)[0])')
fi
echo "story id: $id"

tmp=$(mktemp -d)
export XDG_CONFIG_HOME="$tmp/config" XDG_DATA_HOME="$tmp/data" XDG_STATE_HOME="$tmp/state"
mkdir -p "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_STATE_HOME"

frame() { swift "$here/scripts/window-frame.swift" hn-desktop 2>/dev/null; }

full=/tmp/hn-desktop-full.png
"$bin" --open-story "$id" --on-top >"$out/live-run.log" 2>&1 &
pid=$!
trap 'kill "$pid" 2>/dev/null || true; rm -rf "$tmp" /tmp/hn-desktop-full.png' EXIT

# Wait for the window to exist before the first capture.
i=0
while [ "$i" -lt 60 ]; do
    if frame >/dev/null; then break; fi
    sleep 0.5
    i=$((i + 1))
done
if ! bounds=$(frame); then
    echo "FAIL  no hn-desktop window on screen"
    exit 1
fi

# The page has to load: the child view is created on the first frame after the story opens, and
# the story itself lands after the feed's first fill.
sleep 8

# shellcheck disable=SC2086
set -- $bounds
screencapture -x "$full"
python3 "$here/scripts/crop.py" "$full" "$1" "$2" "$3" "$4" \
    "$out/live-article-pane.png" --scale 2 --pane

kill "$pid" 2>/dev/null || true
wait "$pid" 2>/dev/null || true
echo "done  app log: $out/live-run.log"
