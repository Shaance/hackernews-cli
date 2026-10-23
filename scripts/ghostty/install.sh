#!/bin/sh
# Install the optional companion without retaining a checkout or download dependency.
set -eu
if [ "$#" -ne 1 ]; then
    echo "Usage: $0 /path/to/terminal-browser-v0.13.4" >&2
    exit 2
fi
if [ "$(uname -s)" != Darwin ]; then
    echo "The hn browser companion currently supports macOS with Ghostty." >&2
    exit 1
fi
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)
source_dir=$(CDPATH= cd -- "$1" && pwd -P)
install_dir="${XDG_DATA_HOME:-$HOME/.local/share}/hn/terminal-browser-v0.13.4"
launcher="$HOME/.local/bin/terminal-browser"
original=71cb9486199e4af98c3192ce8a44496d63832006e314bbd2bee70f4542505ac8
patched=de03483f54653266fcd6553c03d16e71f982209c7266e1a631d91229fc8d8393

if [ "$(cat "$source_dir/VERSION")" != v0.13.4 ]; then
    echo "Expected terminal-browser v0.13.4; source left unchanged." >&2
    exit 1
fi
source_hash=$(shasum -a 256 "$source_dir/cli/dist/main.js" | cut -d ' ' -f 1)
if [ "$source_hash" != "$original" ] && [ "$source_hash" != "$patched" ]; then
    echo "Unrecognized terminal-browser bundle; source left unchanged." >&2
    exit 1
fi

mkdir -p "$(dirname -- "$install_dir")" "$(dirname -- "$launcher")"
work=$(mktemp -d "$(dirname -- "$install_dir")/.terminal-browser.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM
# Quote the fixed destination so spaces, apostrophes and shell metacharacters work.
quoted=$(printf '%s' "$install_dir/bin/terminal-browser" | sed "s/'/'\\\\''/g")
printf '#!/bin/sh\nexec '\''%s'\'' "$@"\n' "$quoted" > "$work/launcher"
chmod 755 "$work/launcher"
if [ -e "$launcher" ] || [ -L "$launcher" ]; then
    if [ -L "$launcher" ] && { [ "$launcher" -ef "$source_dir/bin/terminal-browser" ] ||
        [ "$launcher" -ef "$install_dir/bin/terminal-browser" ]; }; then
        : # Replace a link to the supplied distribution with the independent launcher.
    elif ! cmp -s "$work/launcher" "$launcher"; then
        echo "$launcher already exists. Move it aside before installing this companion." >&2
        exit 1
    fi
fi

if [ -e "$install_dir" ] || [ -L "$install_dir" ]; then
    if [ -L "$install_dir" ] ||
        [ "$(cat "$install_dir/VERSION")" != v0.13.4 ] ||
        [ "$(shasum -a 256 "$install_dir/cli/dist/main.js" | cut -d ' ' -f 1)" != "$patched" ]; then
        echo "$install_dir already exists with different contents; left unchanged." >&2
        exit 1
    fi
else
    ditto "$source_dir" "$work/distribution"
    if [ "$source_hash" = "$original" ]; then
        patch -d "$work/distribution" -p1 < "$script_dir/terminal-browser-v0.13.4.patch"
    fi
    if [ "$(shasum -a 256 "$work/distribution/cli/dist/main.js" | cut -d ' ' -f 1)" != "$patched" ]; then
        echo "Companion patch verification failed." >&2
        exit 1
    fi
    mv "$work/distribution" "$install_dir"
fi
DO_NOT_TRACK=1 "$install_dir/bin/terminal-browser" --version
mv -f "$work/launcher" "$launcher"
printf '\nInstalled companion: %s\n' "$install_dir"
printf 'Launcher: %s\n' "$launcher"
case ":$PATH:" in
    *":$HOME/.local/bin:"*) ;;
    *) printf '\nAdd this to your shell configuration, then open a new Ghostty shell:\nexport PATH="$HOME/.local/bin:$PATH"\n' ;;
esac
printf '\nUse the hn-article binding in scripts/ghostty/README.md, then run hn in Ghostty.\n'
