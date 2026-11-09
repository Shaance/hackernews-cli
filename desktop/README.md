# hn-desktop

The native window over `hn_lib`: the same feed, discussion and summary the terminal app shows,
drawn with Rune instead of a terminal. One window, one `SplitView`; opening a story puts the
story's **live page** in the main pane and the summary card plus the discussion in the inspector.

`mockups/desktop/DESIGN.md` is the design of record — window anatomy, the motion spec, the
screens, and the testing strategy. `DESKTOP_IMPLEMENTATION_PLAN.md` at the repo root is the plan
this crate was built to.

## The article pane is the live page

Opening a story hosts a `WKWebView` child view over the article pane and loads the story's URL —
the same scheme-checked URL the terminal app opens (`hn_lib::url_open::safe_story_url`, with the
HN item page as the fallback for a story with no usable link). The page is the real one: its own
typography, images, code blocks, and anything interactive.

```
article pane
├── Rune header   eyebrow · title · byline · "open original ↗"
└── live region   the story's page, in a WKWebView child view
```

The header stays Rune's because the child view is a subview of the window's content view and
draws above everything Rune paints: anything the reader must still be able to press has to sit
outside its rect. The child is pinned to the region below the header and follows it through
`El::on_layout`, so a window resize, a grip drag or `⌘\` moves the page with the pane. It is
hidden — `setHidden`, not torn down — while the settings dialog or the help sheet is open, and
while the feed is showing.

**The readability capture still runs, and is still the point.** Opening a story fetches and
extracts the article exactly as before; that capture is what the summary card and its citations
read. The model never sees the page, it sees the extracted text, and a citation's excerpt is a
span of that text. The capture is a backend, not a surface.

**Fallback.** When no child view can exist, the pane is the extracted Markdown at the 66ch
measure, under a banner that says so:

```
live page unavailable — showing extracted text
```

| Case | Why |
| --- | --- |
| Headless (`--snap`, `--tree`, `--script`) | A `WKWebView` needs an `NSWindow`; a headless run renders offscreen. The scene trees are deterministic because they always take this branch. |
| Not macOS | `wry` is a macOS-only dependency here; there is no child host to build. |
| The child view failed to build | The banner carries the platform's own message beside it. |

Summaries and citations work in all three, because they read the capture rather than the pane.
`O` and `open original ↗` are unchanged in all three: they hand the URL to the system browser.

Two consequences of a native child view, both worth knowing before you use it:

- **The page owns its own keyboard while it has focus.** Clicking into the page gives the
  `WKWebView` first responder and Rune's `on_key` no longer sees those keys. Clicking any Rune
  chrome — the toolbar, the header, the inspector — takes focus back.
- **A window resized while another app is in front keeps stale bounds** until the next layout.
  Rune only relayouts a resize when the window is painting
  (`crates/rune/src/window/lifecycle.rs:225`), and the child follows `on_layout`. This is the
  spike's finding, unchanged.

## Run it

```sh
cargo run --release --manifest-path desktop/Cargo.toml   # the window
cargo install --path desktop                            # `hn-desktop` on PATH
```

The crate is its own workspace root with its own `rust-toolchain.toml` (1.99.1, which Rune
needs; the repo root pins 1.88 for the TUI). `--locked` works: `desktop/Cargo.lock` is committed.

Two launch flags exist for the GUI proof below, because this machine denies osascript assistive
access and nothing outside the process can press a key or raise its window:

| Flag | What it does |
|---|---|
| `--open-story <id>` | opens that story once the feed holds it (and says so if the id is not on the page) |
| `--on-top` | keeps the window above other apps, so a fullscreen capture is of this window |

Keys are the terminal app's, unchanged: `j`/`k` move, `1`–`4` switch tabs, `Enter` opens a story,
`c` opens the discussion, `s` saves, `?` shows the keyboard sheet, `M` (or `⌘,`) opens the summary
settings, `⌘?` shows the sheet, `⌘\` toggles the inspector, `q` quits. The sheet is generated from
`src/keys.rs`, so it cannot drift from what the window binds.

## Where it keeps its files

| What | Path | Owner |
|---|---|---|
| Summary settings (provider, key, profile) | `$XDG_CONFIG_HOME/hn/summary.json` | `hn_lib::reader::Config` — the same file the TUI writes |
| Saved stories | `$XDG_DATA_HOME/hn/saved.json` | `hn_lib::bookmarks` |
| Window size, inspector width, tab, page, selection | `$XDG_STATE_HOME/hn/desktop.json` | `src/layout.rs` |

Each falls back to `~/.config`, `~/.local/share` and `~/.local/state` when the XDG variable is
unset. The layout file is written at most every two seconds while the window moves, and once on
quit; a missing or malformed file is the defaults, never an error. A headless run never reads or
writes it, so a golden cannot depend on the machine's window size.

The API key is a secure field: Rune emits no `text` for it, skips its accessibility value, and
nothing in this crate formats it into a string. `--tree` output for the settings scene contains
the key zero times, which `scripts/check_goldens.sh` asserts.

## Headless testing

Every scene renders offscreen, so the whole UI is testable without a window server:

```sh
cargo run -- --scene feed --size 1180x760 --scale 2 --theme dark \
    --script scripts/focus.json --snap out.png --tree out.json --trace out.trace.jsonl
```

| Flag | What it does |
|---|---|
| `--scene <name>` | which fixture state to draw (`feed`, `feed-wide`, `feed-saved`, `resize`, `story`, `comments`, `thread`, `citation`, `summary-generating`, `settings`, `help`, …) |
| `--size WxH`, `--scale N`, `--theme dark\|light` | the window, the pixel scale, the theme |
| `--script <file>` | a JSON timeline of key and pointer events |
| `--at mark:<name>` / `--at <seconds>` | stop at a point in that timeline instead of the end |
| `--snap`, `--tree`, `--trace` | write the PNG, the element + accessibility tree, and the per-frame motion trace |
| `--frames N` | render N frames and exit (a cold-start measurement) |

`--tree` is the interesting one: it carries every element's rect, role, label, focus flags and
accessibility node, which is what the gate reads instead of the pixels.

A headless run has no `NSWindow`, so the article pane always takes its fallback branch there: the
`story`, `comments`, `thread`, `citation` and `summary-generating` scenes draw the extracted text
under the `article-fallback` banner, never a live page. That is deliberate — it is what makes
those trees deterministic — and it is why the live pane needs the GUI proof below.

```sh
bash desktop/scripts/check_goldens.sh          # the gate
HN_SKIP_BUILD=1 bash desktop/scripts/check_goldens.sh   # reuse target/debug
HN_TREES_ONLY=1 bash desktop/scripts/check_goldens.sh   # skip the PNG comparison (Linux/Windows)
```

The gate renders every frozen scene and checks four independent things: the canonicalised tree
against `verification/expected/`, the PNG within 0.5% of the golden (macOS only), the motion lint
(nothing animates at rest, no spatial track is ever active), and fidelity invariants read from the
tree — column x-positions, rank order, stepper text, the selected row's focus flags. It also
asserts the settings dialog's key field carries no text, that the help sheet's rows are each a
keybind plus a label, and that `HN_REDUCED_MOTION=1` lands the same state as the animated run.

Beyond the stills it drives the overlays: `M` opens the dialog from the story split and `Esc`
closes it with the discussion intact; a press on the Anthropic radio and the Thorough segment
moves both and re-resolves the model line; the sheet's close button dismisses it and the feed
behind comes back to life. Those read the tree after the press, so a handler that renders but
does nothing fails the gate.

`cargo test` covers the parts a golden cannot: the key map against the routing tables, the
settings round-trip through a temp `XDG_CONFIG_HOME`, the layout file, and the feed's arithmetic.

## The GUI proof

A golden cannot show the live page: headless always takes the fallback branch, because a
`WKWebView` needs an `NSWindow`. So the pane's real state is proved by a screenshot of a real
window.

```sh
bash desktop/scripts/verify-live.sh            # the top story on HN right now
bash desktop/scripts/verify-live.sh 50426582   # a specific story
```

It launches the window with `--open-story <id> --on-top`, waits for the page, captures the whole
screen and crops to the window frame from `scripts/window-frame.swift`, then asserts with
`scripts/crop.py --pane` that the article pane holds a rendered page rather than the app's own
background. The result is `verification/live-article-pane.png`; the run's log is
`verification/live-run.log`.

`screencapture -l <CGWindowID>` returns an empty buffer for a Metal-backed window in this
environment, which is why the capture is fullscreen-then-crop. The run is hermetic: it points
`XDG_CONFIG_HOME`, `XDG_DATA_HOME` and `XDG_STATE_HOME` at a temp directory, so it never reads the
reader's summary key (and so never calls a provider), never writes their saved list, and never
overwrites their remembered window size.

## Packaging

`scripts/package-macos.sh` stages a `.app` bundle — `Contents/MacOS/hn-desktop`,
`Contents/Resources/`, and an `Info.plist` with the keys LaunchServices reads — into
`desktop/dist/HN.app`. It follows the layout of Rune's own `scripts/mac-app`, and stops there: no
codesign, no install into `~/Applications`, no notarization, no atomic exchange.

```sh
bash desktop/scripts/package-macos.sh
open desktop/dist/HN.app
```

This is a spike, not a release pipeline. What a real one still needs:

- **Signing.** The bundle is unsigned. It runs from the machine that built it; anywhere else
  Gatekeeper refuses it until it is signed with a Developer ID and notarized.
- **An icon.** `CFBundleIconFile` is omitted rather than naming a file that does not exist.
- **A universal binary.** The staged executable is whatever `cargo build --release` produced —
  this machine's architecture only.
- **A version story.** `CFBundleShortVersionString` comes from `Cargo.toml`, which is `0.1.0`.

### Measured

Release build, `cargo build --release --locked`, macOS arm64:

| | |
|---|---|
| Binary | 32,788,560 bytes (31 MiB) |
| Cold start, exec → first frame | 80–130 ms over sixteen runs |
| GPU init | 11–35 ms |
| First frame, CPU | 3.5–6.4 ms warm; 17 ms on the first run after a build |

The cold-start figure is `--frames 1` on the release binary at 1180×760 @2x: process start,
dyld, GPU init, layout and one rendered frame. The first run after a build takes ~0.7 s because
the 31 MiB binary is not yet in the page cache; every run after that is the range above. A
windowed launch adds AppKit window creation, which the headless path does not include and this
number therefore does not claim.

## Known limitations

- **Rune is pinned to a pre-release commit** (`d751f07`), not a published crate. Building needs
  network access to `github.com/anomalyco/rune`, and an upstream change can break this crate
  without a version bump to warn us.
- **macOS is the only platform with a verified window.** The headless path is cross-platform and
  the gate runs on Linux with `HN_TREES_ONLY=1`, but the window itself has only been driven on
  macOS. Linux needs the usual X11/Wayland and font packages a GPU window implies; nothing here
  has been tested against them.
- **The live pane is macOS-only.** `wry` sits under `[target.'cfg(target_os = "macos")'.dependencies]`,
  so a Linux or Windows build resolves and runs without webkit2gtk or WebView2 — and its article
  pane is the extracted-text fallback. Wiring the other two backends is wry's own work, unspiked
  here. On Linux, `wry` would also pull webkit2gtk, which is a large dependency for a crate that
  had none.
- **The live pane's keyboard belongs to the page while it has focus**, and a window resized while
  occluded keeps stale child bounds until the next layout. Both are in *The article pane is the
  live page* above.
- **No notarization, no signing, no installer.** See Packaging above.
- **No VoiceOver pass has been run.** The accessibility tree is asserted headlessly (roles,
  labels, focus flags, focus order); how it *sounds* has not been checked by a human. The
  checklist below is that pass, written down so it can be run.

## Manual VoiceOver pass

Headless tests prove the tree has roles, labels and focus flags. They cannot prove the result is
usable with a screen reader, because that depends on how VoiceOver reads it, in what order, and
whether the focus ring is visible while it does. Run this on a real window, with VoiceOver on
(`⌘F5`), and record pass/fail per line.

**Feed**

- [ ] The window announces itself and the toolbar's tabs are reachable with `VO←`/`VO→`.
- [ ] Each story row reads as a button, and its label is the title followed by points, author and
      comment count — not the rank number first, and not the raw text runs.
- [ ] `VO-Space` on a row opens the story, the same as `Enter`.
- [ ] The page stepper's arrows read as "Previous page" and "Next page", and pressing them turns
      the page.
- [ ] The footer hints are not announced as controls (they are text, not buttons).

**Discussion**

- [ ] The trail reads as a chain of buttons ending in the comment being read, and the last
      segment is not announced as pressable.
- [ ] A folded thread's label states how many replies it hides.
- [ ] Focus mode's dimmed rows are still announced — dimming is visual, not hidden.
- [ ] Citation chips read as "Citation N" and open the source.

**Settings (`M`)**

- [ ] The dialog is announced as a dialog named "Summary settings", and focus lands inside it.
- [ ] `Tab` cycles the provider radios, the key field, the switch, the profile segments and the
      three buttons, and never escapes to the feed behind.
- [ ] The key field announces itself as a secure field and **never speaks the key**, not on focus,
      not on typing, not on `VO-c`.
- [ ] The provider radios announce which one is selected.
- [ ] `Esc` closes the dialog and focus returns to where it was.

**Help (`?`)**

- [ ] The sheet is announced as a dialog named "Keyboard".
- [ ] Each row reads as its keys then its action ("n, right arrow, space, page down — Next page").
- [ ] `Esc` and `?` both close it.

**Everywhere**

- [ ] The focus ring is visible on every focused control, and it is amber, not AppKit blue.
- [ ] Nothing is reachable by `Tab` that is not visible, and nothing visible that is a control is
      skipped — with one deliberate exception: the inspector's grip is not a Tab stop, as AppKit's
      dividers are not. It exposes a range value and a step instead, so a screen reader can still
      resize the pane.
- [ ] With **Reduce Motion** on in System Settings, opening a story and re-rooting the thread
      still land in the same place, without the glide.
