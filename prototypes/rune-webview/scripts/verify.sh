#!/bin/sh
# Launch the spike in a real window, capture it at each step, and quit.
#
# Capture method: `screencapture -l <CGWindowID>` returns an empty buffer for a Metal-backed
# window in this environment, so this captures the whole screen and crops to the window frame
# reported by `scripts/window-frame.swift` (CGWindowListCopyWindowInfo, not accessibility-gated).
# `scripts/crop.py` does the crop and fails the run if the result is blank.
#
# The resize, the toggle, and the remote load are driven by the app's own
# `--spike-at SECONDS:ACTION` script rather than by osascript: this machine denies osascript
# assistive access, so nothing outside the process can resize its window or press a key.
# `--spike-at` runs through `AppHandle::update` on the UI thread and takes the same path a user's
# drag, a "w" press, and a "u" press take.
#
# `--spike-on-top` is required for the resize steps to be meaningful. Rune only relayouts a
# resize when the window is actually painting (crates/rune/src/window/lifecycle.rs:225); an
# occluded window gets its NSWindow frame jumped with no relayout, so `on_layout` never fires and
# the child keeps its old bounds. Keeping the window above other apps keeps it painting.
#
# Usage: scripts/verify.sh [remote-url]
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
out="$here/verification"
bin="$here/target/debug/rune-webview"
url=${1:-https://news.ycombinator.com/item?id=1}
mkdir -p "$out"

now() { python3 -c 'import time; print(time.time())'; }
sleep_until() {
    python3 - "$1" "$start" <<'PY'
import sys, time
target, start = float(sys.argv[1]), float(sys.argv[2])
time.sleep(max(0.0, target - (time.time() - start)))
PY
}

frame() { swift "$here/scripts/window-frame.swift" rune-webview 2>/dev/null; }

shot() {
    name=$1
    shift
    extras=$*
    bounds=$(frame) || { echo "FAIL  $name: no rune-webview window on screen"; return 1; }
    # shellcheck disable=SC2086
    set -- $bounds
    full="/tmp/rune-webview-full.png"
    screencapture -x "$full"
    # shellcheck disable=SC2086
    python3 "$here/scripts/crop.py" "$full" "$1" "$2" "$3" "$4" "$out/$name.png" --scale 2 $extras
}

start=$(now)
"$bin" \
    --spike-on-top \
    --spike-at 5:resize:1280x900 \
    --spike-at 9:toggle \
    --spike-at 12:toggle \
    --spike-at 15:resize:760x560 \
    --spike-at 18:remote \
    "$url" \
    >"$out/run.log" 2>&1 &
pid=$!
trap 'kill "$pid" 2>/dev/null || true; rm -f /tmp/rune-webview-full.png' EXIT

# Wait for the window to exist before the first capture.
i=0
while [ "$i" -lt 60 ]; do
    if frame >/dev/null; then break; fi
    sleep 0.5
    i=$((i + 1))
done

sleep_until 3;  shot 01-initial
sleep_until 7;  shot 02-resized-larger
sleep_until 11; shot 03-toggled-hidden
sleep_until 14; shot 04-toggled-shown
sleep_until 17; shot 05-resized-smaller
sleep_until 23; shot 06-remote-hn --expect-orange

kill "$pid" 2>/dev/null || true
wait "$pid" 2>/dev/null || true
echo "done  app log: $out/run.log"
