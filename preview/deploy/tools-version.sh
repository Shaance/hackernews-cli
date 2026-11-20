#!/usr/bin/env bash
set -euo pipefail

cd "${1:-$(dirname "${BASH_SOURCE[0]}")/../..}"
shopt -s nullglob
hasher=(sha256sum)
command -v sha256sum >/dev/null || hasher=(shasum -a 256)

printf '%s\n' preview/tools/*.mjs preview/tools/*.sh preview/deploy/*.service preview/deploy/*.timer |
  LC_ALL=C sort |
  while IFS= read -r file; do
    case "$file" in ''|*-check.mjs) continue ;; esac
    cat -- "$file" || exit 1
  done |
  "${hasher[@]}" | cut -c 1-12
