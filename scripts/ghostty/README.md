# Optional Ghostty companion setup

The maintained prototype recipe targets terminal-browser **v0.13.4** and the
existing Ghostty key-table support. Install terminal-browser through its
[official instructions](https://github.com/zenbu-labs/terminal-browser#installation).
Keep macOS signature/quarantine checks intact. The patch modifies only the
JavaScript CLI bundle, not the signed Electron app.

Point the checkout at an extracted/installed v0.13.4 distribution. Replace
`/path/to/terminal-browser` below with its root, containing `VERSION`, `bin/`,
`cli/`, and `electron/`:

```sh
mkdir -p .tools
ln -s /path/to/terminal-browser .tools/terminal-browser
cat .tools/terminal-browser/VERSION
shasum -a 256 .tools/terminal-browser/cli/dist/main.js
```

Expected version: `v0.13.4`. Original bundle SHA-256:
`71cb9486199e4af98c3192ce8a44496d63832006e314bbd2bee70f4542505ac8`.
If it matches, apply the checked-in patch from the repository root:

```sh
patch --dry-run -d .tools/terminal-browser -p1 < scripts/ghostty/terminal-browser-v0.13.4.patch
patch -d .tools/terminal-browser -p1 < scripts/ghostty/terminal-browser-v0.13.4.patch
```

Already patched SHA-256:
`de03483f54653266fcd6553c03d16e71f982209c7266e1a631d91229fc8d8393`.
If that matches, skip patching. For any other hash/version, stop and inspect the
upstream change; do not force this patch onto a different release.

Add the single binding in `hn-article.conf` to your Ghostty config, then reload
Ghostty configuration once with Cmd+Shift+comma. The binding defines a named
key table; hn activates it only on the companion it creates. Other panes retain
their ordinary Cmd+W action. Running hn never installs this configuration.

Run `./scripts/dev.sh` inside Ghostty. The checked-in patch clears startup text
and closes the exact companion pane after a successful browser exit. It is
experimental; validate native behavior on your Ghostty version:

1. Keep another ordinary pane open. Open a story using `o`.
2. Scroll the article and follow a same-tab link. Return focus to hn and press
   `o` again. The same page and scroll position should remain.
3. Return to the feed and open a different story. The existing companion should
   display it, without an additional pane.
4. Press Cmd+W in the companion. It should close without confirmation or a
   “Process exited” screen. The hn pane and unrelated panes must remain intact.
5. Press `o` again; the companion should reopen. Check the ordinary pane still
   uses its usual close behavior, cancelling any confirmation shown there.

Socket tests prove owned-tab commands, not these native window interactions.
To revert the optional setup, remove the named key-table line, reload config,
and reverse the patch (`patch -R` with the same file). No global browser setting
or close-confirmation preference needs changing.
