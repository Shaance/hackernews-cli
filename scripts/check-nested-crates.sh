#!/bin/sh
# Resolve every crate in the repo (root + nested workspaces) against the frozen
# root manifest, without a full build. Catches dependency-pin conflicts between
# the root Cargo.toml and any nested crate (e.g. desktop/ vs Rune's transitive
# requirements) at plan/CI time instead of mid-workstream at build time.
#
# Usage: scripts/check-nested-crates.sh
# Exit: 0 if all crates resolve with --locked, 1 otherwise (errors printed).
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

# Root package plus every nested crate that declares its own [workspace].
crates="."
for manifest in $(find . -name Cargo.toml \
        -not -path './target/*' -not -path './*/target/*' \
        -not -path './preview/*' -not -path './.git/*' | sort); do
    dir=$(dirname "$manifest")
    [ "$dir" = "." ] && continue
    if grep -q '^\[workspace\]' "$manifest"; then
        crates="$crates $dir"
    fi
done

fail=0
for crate in $crates; do
    if (cd "$crate" && cargo metadata --locked --format-version 1 >/dev/null 2>/tmp/check-nested-crates.err); then
        echo "PASS  $crate"
    else
        echo "FAIL  $crate"
        sed 's/^/      /' /tmp/check-nested-crates.err | head -8
        fail=1
    fi
done
rm -f /tmp/check-nested-crates.err
if [ "$fail" -ne 0 ]; then
    echo "Dependency resolution failed. If a nested crate needs a root Cargo.toml pin relaxed,"
    echo "declare it in the workstream brief and land it on main BEFORE cutting the worktree."
    exit 1
fi
echo "All crates resolve against the frozen root manifest."
