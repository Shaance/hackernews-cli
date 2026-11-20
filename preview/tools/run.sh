#!/bin/bash
set -euo pipefail

node=/opt/hn-preview/node/bin/node
state=/var/lib/hn-publisher

"$node" /opt/hn-preview/tools/rollout.mjs --reconcile-writer
rel=$(readlink -f "$state/releases/current")
snapshot=$state/candidate.json

"$rel/hn" --capture-snapshot "$snapshot"
"$rel/hn" --prepare-snapshot "$snapshot"
HN_BIN="$rel/hn" "$node" "$rel/publish.mjs" "$snapshot"
