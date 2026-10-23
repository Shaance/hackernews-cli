# Ghostty article pane setup

This optional companion supports macOS, Ghostty, and terminal-browser **v0.13.4**.
Install `hn` first with `cargo install --path . --locked`.

## Install the browser once

Download and extract the matching macOS distribution from the
[v0.13.4 release](https://github.com/zenbu-labs/terminal-browser/releases/tag/v0.13.4).
An existing v0.13.4 distribution also works. Its root must contain `VERSION`,
`bin/`, `cli/`, and `electron/`.

From the hn checkout, run:

```sh
sh scripts/ghostty/install.sh /path/to/terminal-browser
```

The script copies the complete distribution into
`${XDG_DATA_HOME:-$HOME/.local/share}/hn/terminal-browser-v0.13.4` and installs a
launcher at `~/.local/bin/terminal-browser`. It checks the supported version and
bundle hash, then applies the included startup/exit patch to its own copy.
The source distribution, saved settings, and API keys stay unchanged. After
installation, the browser works independently of the download and hn checkout.

If `~/.local/bin` is not on your PATH, add this line to `~/.zshrc` and open a new
Ghostty shell:

```sh
export PATH="$HOME/.local/bin:$PATH"
```

Check in that shell:

```sh
command -v hn
command -v terminal-browser
terminal-browser --version
```

The last command should print `terminal-browser v0.13.4`. Keep macOS signature
and quarantine checks intact. Only the JavaScript CLI bundle is patched; the
signed Electron app is copied unchanged. Use the supported version until the
pane patch has been checked against a newer release.

Running the installer again reuses a matching installation. It can replace an
old launcher symlink that points directly at the supplied distribution. If
another command already occupies `~/.local/bin/terminal-browser`, the installer
stops and asks you to move that command aside.

## Configure the browser pane's close shortcut

Add the line from [hn-article.conf](hn-article.conf) to your active Ghostty config:

```ini
keybind = hn-article/cmd+w=text:\x11
```

Reload Ghostty configuration once with Cmd+Shift+comma. This defines a named key
table that hn activates only on its browser pane. Other panes retain their
ordinary Cmd+W action. Neither the installer nor hn edits Ghostty configuration.

## Read with the installed app

Run `hn` from any directory inside Ghostty. Start with at least 110 terminal
columns. Press `o` or Enter in the feed to open the article beside its discussion;
`c` opens the discussion without creating a browser pane. `O` always opens the
external browser.

Check the native behavior on your Ghostty version:

1. Keep another ordinary pane open. Open a story using `o`.
2. Scroll the article and follow a same-tab link. Return focus to hn and press
   `o` again. The same page and scroll position should remain.
3. Return to the feed and open a different story. The existing companion should
   display it, without an additional pane.
4. Press Cmd+W in the companion. It should close without confirmation or a
   “Process exited” screen. The hn pane and unrelated panes must remain intact.
5. Press `o` again; the companion should reopen.

## Verify without opening Ghostty

Run the installer check with Python 3 and a real distribution. You can use the
installed copy, so the original download is no longer needed:

```sh
python3 scripts/ghostty/test-install.py "${XDG_DATA_HOME:-$HOME/.local/share}/hn/terminal-browser-v0.13.4"
```

It uses temporary directories to check fresh installation, rerunning setup,
removal of the source directory, settings preservation, and launcher conflicts.
It does not open Ghostty. Socket tests cover owned-tab commands; the steps above
cover native window interactions. The main [verification commands](../../README.md#verify-without-ghostty-or-api-keys)
exercise the reader and installed settings without computer use or provider keys.

## Remove the companion

To remove this companion, remove its `~/.local/bin/terminal-browser` launcher and
the installed distribution, then remove the named key-table line and reload
Ghostty. Summary settings and saved stories are separate and remain available.
