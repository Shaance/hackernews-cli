#!/bin/sh
# Print the Homebrew formula for one release, filled in from the archives' .sha256 files.
# Usage: render-formula.sh VERSION DIR   (DIR holds hn-VERSION-TARGET.tar.gz.sha256)
# The tui-release workflow pushes the output to Shaance/homebrew-tap as Formula/hn.rb.
set -eu
version=$1
dir=$2
script="s/@VERSION@/$version/g"
for target in aarch64-apple-darwin x86_64-apple-darwin aarch64-unknown-linux-gnu x86_64-unknown-linux-gnu; do
    sha=$(cut -d ' ' -f 1 "$dir/hn-$version-$target.tar.gz.sha256")
    script="$script;s/@SHA256_$target@/$sha/g"
done
sed "$script" "$(dirname "$0")/hn.rb"
