#!/usr/bin/env bash
# Stage a macOS .app bundle for hn-desktop. A spike, not a release pipeline.
#
# It follows Rune's own `scripts/mac-app` layout — Contents/MacOS/<exe>, Contents/Resources/,
# and an Info.plist with the keys LaunchServices needs — but stops at staging: no codesign, no
# install into ~/Applications, no notarization, no atomic exchange. The bundle is written to
# desktop/dist/HN.app and left there for a human to open.
#
# Usage:  bash desktop/scripts/package-macos.sh
#   HN_SKIP_BUILD=1   use the binary already in target/release
#   HN_APP_NAME=...   the bundle's name (default HN)
set -euo pipefail

cd "$(dirname "$0")/.."

NAME=${HN_APP_NAME:-HN}
EXE=hn-desktop
BUNDLE_ID=dev.hn.desktop
VERSION=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
BIN=target/release/$EXE
APP=dist/$NAME.app

if [ "$(uname -s)" != Darwin ]; then
  echo "package-macos.sh: a .app bundle is macOS-only (this is $(uname -s))" >&2
  exit 2
fi

if [ "${HN_SKIP_BUILD:-0}" != 1 ]; then
  echo "== build"
  cargo build --release --locked --bin "$EXE"
fi
[ -x "$BIN" ] || { echo "no release binary at $BIN; run without HN_SKIP_BUILD=1" >&2; exit 2; }

echo "== stage $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/$EXE"
chmod +x "$APP/Contents/MacOS/$EXE"

# The keys LaunchServices reads. `CFBundleIconFile` is omitted: there is no icon yet, and a
# plist naming a missing file is worse than one that names none.
cat >"$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>$NAME</string>
  <key>CFBundleDisplayName</key><string>$NAME</string>
  <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
  <key>CFBundleExecutable</key><string>$EXE</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

plutil -lint "$APP/Contents/Info.plist" >/dev/null

echo "== staged"
printf '  %s\n' "$PWD/$APP"
printf '  binary  %s\n' "$(du -h "$APP/Contents/MacOS/$EXE" | cut -f1)"
printf '  bundle  %s\n' "$(du -sh "$APP" | cut -f1)"
echo
echo "Open it with:  open $APP"
echo "Unsigned and unnotarized: it runs from this machine, and Gatekeeper will refuse it"
echo "anywhere else until it is signed with a Developer ID and notarized."
