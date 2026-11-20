#!/usr/bin/env bash
# Installs or updates the release tooling on one trial host, as root, from a
# checkout of this repository: preview/deploy/install-tools.sh dev|prod
# It never touches releases, snapshots or credentials, and it does not start
# or enable timers; see preview/deploy/README.md for first-time setup.
set -euo pipefail

role=${1:?usage: install-tools.sh dev|prod}
[[ $role == dev || $role == prod ]] || { echo "role must be dev or prod" >&2; exit 2; }
[[ $(id -u) == 0 ]] || { echo "run as root" >&2; exit 2; }
repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
tools=/opt/hn-preview/tools
node=/opt/hn-preview/node/bin

# Pinned helper binaries (cosign v3.1.4, crane v0.22.1, linux amd64).
fetch() { # url sha256 target
  local tmp
  tmp=$(mktemp)
  curl -fsSL --max-time 300 -o "$tmp" "$1"
  echo "$2  $tmp" | sha256sum -c --quiet
  printf '%s\n' "$tmp"
}
install -d -o root -g root -m 0755 "$tools"
if ! echo "424e2c97d0b45fd7a6b608a87f090f35633f4c23ece5f092d48bba11aaf4bbfa  $tools/cosign" | sha256sum -c --quiet 2>/dev/null; then
  f=$(fetch https://github.com/sigstore/cosign/releases/download/v3.1.4/cosign-linux-amd64 \
    424e2c97d0b45fd7a6b608a87f090f35633f4c23ece5f092d48bba11aaf4bbfa)
  install -o root -g root -m 0755 "$f" "$tools/cosign" && rm -f "$f"
fi
if ! echo "8e75c326adb43b053cfd78b9b987663f8f4fe4557f4d3915905643f25c730992  $tools/crane" | sha256sum -c --quiet 2>/dev/null; then
  f=$(fetch https://github.com/google/go-containerregistry/releases/download/v0.22.1/go-containerregistry_Linux_x86_64.tar.gz \
    0ab7a1d6932a213aed964ce97666c3077fe691c8606413674a8b3e0b9ec4cda0)
  tar -xzf "$f" -C "$(dirname "$f")" crane
  install -o root -g root -m 0755 "$(dirname "$f")/crane" "$tools/crane" && rm -f "$f" "$(dirname "$f")/crane"
fi

for file in release.mjs session.mjs update.mjs rollout.mjs; do
  install -o root -g root -m 0644 "$repo/preview/tools/$file" "$tools/$file"
done
install -o root -g root -m 0755 "$repo/preview/tools/run.sh" "$tools/run.sh"
install -o root -g root -m 0644 "$repo/preview/package.json" "$repo/preview/package-lock.json" "$tools/"
(cd "$tools" && PATH="$node:$PATH" npm ci --omit=dev --ignore-scripts --no-audit --no-fund --silent)
bash "$repo/preview/deploy/tools-version.sh" "$repo" > "$tools/VERSION.tmp"
mv "$tools/VERSION.tmp" "$tools/VERSION"

units=/home/hnpreview/.config/systemd/user
for unit in hn-preview.service hn-preview-adopt.service hn-preview-adopt.timer \
  hn-preview-update.service hn-preview-update.timer; do
  install -o hnpreview -g hnpreview -m 0644 "$repo/preview/deploy/$unit" "$units/$unit"
done
install -d -o hnpreview -g hnpreview -m 0700 /home/hnpreview/.local/state/hn-preview
runuser -u hnpreview -- env XDG_RUNTIME_DIR="/run/user/$(id -u hnpreview)" systemctl --user daemon-reload

if [[ $role == dev ]]; then
  for unit in hn-preview-publish.service hn-preview-publish.timer \
    hn-preview-release.service hn-preview-release.timer; do
    install -o root -g root -m 0644 "$repo/preview/deploy/$unit" "/etc/systemd/system/$unit"
  done
  systemctl daemon-reload
fi
echo "Installed tooling $(cat "$tools/VERSION") for $role"
