# hn-desktop

The native window over `hn_lib`: the same feed, discussion and summary the terminal app shows,
drawn with Rune instead of a terminal. One window, one `SplitView`; opening a story puts the
story's **live page** in the main pane and the summary card plus the discussion in the inspector.

`mockups/desktop/DESIGN.md` is the design of record — window anatomy, the motion spec, the
screens, and the testing strategy. `docs/notes/DESKTOP_IMPLEMENTATION_PLAN.md` is the plan
this crate was built to.

## The article pane is the live page

Opening a story hosts a `WKWebView` child view over the article pane and loads the story's URL —
the same scheme-checked URL the terminal app opens (`hn_lib::url_open::safe_story_url`, with the
HN item page as the fallback for a story with no usable link). The page is the real one: its own
typography, images, code blocks, and anything interactive.

```
article pane
├── Rune header   title · metadata
└── live region   the story's page, in a WKWebView child view
```

The header stays Rune's because the child view is a subview of the window's content view and
draws above everything Rune paints: anything the reader must still be able to press has to sit
outside its rect. The child is pinned to the region below the header and follows it through
`El::on_layout`, so a window resize, a grip drag or `⌘\` moves the page with the pane. It is
hidden — `setHidden`, not torn down — while the settings dialog or the help sheet is open, and
while the feed is showing. Reopening that same story retains its page and scroll position.
Opening a different story drops the old child immediately and creates a hidden child for the
new URL. A centered “Loading article…” status stays visible until that child commits its
navigation; it then uses the current pane bounds before appearing. Late callbacks belong to
their original child and cannot expose a replacement. A stalled navigation can keep the status
visible; Open in browser remains available. This handoff does not wait for every subresource
or claim that commit guarantees the first painted pixels.

**The readability capture still runs, and is still the point.** Opening a story fetches and
extracts the article for the article summary. The discussion summary uses captured top-level
comments. Its inline citations open the captured comment and the available reply thread; replies
can be loaded and retried without changing the snapshot used by the summary.

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
In the feed, `o` opens the story; while reading, `o` activates the selected comment or citation.
`Shift+O` and the footer's browser action open the URL in the system browser.

Article extraction and the root discussion load independently. Comments become readable as
soon as they arrive, while the summary continues from that discussion capture. A later summary
result preserves the comment tree, expanded replies, and article already on screen. During the
session, leaving and reopening a story reuses its captures, summaries, citation snapshots and
in-flight work. `r` refreshes comments without regenerating summaries; explicitly regenerating
starts new paid summary requests. On restart, the app reuses saved summary halves and their
citations from the same folder as the TUI. Regenerating a restored half captures fresh sources.

Turn off Automatic summaries to keep loading articles and comments without starting new
automatic summary requests. Submitted summaries can finish; Regenerate summary still works
when automatic generation is off.
Stories opened with it off keep their manual state until you regenerate their summaries.

Two consequences of a native child view, both worth knowing before you use it:

- **The page owns its own keyboard while it has focus.** Clicking into the page gives the
  `WKWebView` first responder and Rune's `on_key` no longer sees those keys. Unmodified `Esc`
  still follows the app's Back action. Clicking Rune chrome takes focus back.
- **A window resized while another app is in front keeps stale bounds** until the next layout.
  Rune only relayouts a resize when the window is painting
  (`crates/rune/src/window/lifecycle.rs:225`), and the child follows `on_layout`. This is the
  spike's finding, unchanged.

## Run it

```sh
cd desktop
cargo run --release      # the window
cargo install --path .   # `hn-desktop` on PATH
```

The crate is its own workspace root with its own `rust-toolchain.toml` (1.99.1, which Rune
needs; the repo root pins 1.88 for the TUI). Run these from inside `desktop/`: rustup picks the
toolchain from the current directory, not from `--manifest-path`, so running them from the repo
root builds with 1.88 and fails. `--locked` works: `desktop/Cargo.lock` is committed.

Two launch flags exist for the GUI proof below, because this machine denies osascript assistive
access and nothing outside the process can press a key or raise its window:

| Flag | What it does |
|---|---|
| `--open-story <id>` | opens that story once the feed holds it (and says so if the id is not on the page) |
| `--on-top` | keeps the window above other apps, so a fullscreen capture is of this window |

Keys follow the terminal app with the desktop feed exceptions below: `j`/`k` move, `1`–`4` switch tabs and return arrow navigation to the feed, `Enter` opens a story,
`c` opens the discussion, `s` saves, `?` shows the keyboard sheet, `M` (or `⌘,`) opens the summary
settings, `⌘?` shows the sheet, `⌘\` toggles the inspector, `⌘=` or `⌘+` zooms in,
`⌘-` zooms out, `⌘0` returns to actual size, and `q` quits. The zoom applies to the whole
window and live article page, and is remembered across launches. `?` also opens the sheet
from the discussion and citation views. The sheet is generated from
`src/keys.rs`, so it cannot drift from what the window binds.

In a story, `Esc` returns to the feed, `Shift+S` shows or hides the summary, `Shift+J`/`Shift+K`
scroll it, and `r` refreshes comments without regenerating either summary. `Tab` selects a
citation and `Enter` or `o` opens its captured comment alongside the supporting summary points.
`j`/`k` or Up/Down select comments in either view; `Enter` or `o` toggles replies, `c` collapses the
current branch or its nearest open parent, and Page Up/Down scrolls. In a source, `Tab` and
`Shift+Tab` switch citations and `Esc` returns to the same discussion position. Focus mode
starts on and `f` toggles it; moving the pointer over a comment selects it while focus is on.
The footer's Back control follows the same path, as does a mouse Back button over the app chrome,
discussion, and native article pane on macOS, including a macOS Back navigation swipe. Passive
shortcut hints dim with pointer use and return with keyboard navigation; the Back and browser
actions remain visible.
`Shift+L` lists links in the post and selected comment, or says when there are none. These
shortcuts work when the app chrome or discussion has focus; the live page retains its own
keyboard while focused.

The desktop feed also accepts `M` for summary settings; the terminal feed does not. Desktop feed navigation uses Left/Right to select the same row in the adjacent column (clamped on a shorter right column). At a column edge they turn the page; in one column they page directly. Forward lands on the first story and backward on the last; known feed ends stay put. Use `n`/`p`, Space, Page Down/Page Up, or the page buttons to change pages. Keyboard selection is instant; pointer selection has a restrained 120 ms color response with fixed text geometry. Completed page changes enter as one group from the navigation direction, 24 pt away and at 0.2 opacity, over about 220 ms. Reduced motion is instant. Discussion arrows retain their existing back/unfold bindings.

Moving the pointer over a feed row selects it; clicking the row opens the story. The separate
Save or Unsave action changes the saved list without opening it.

An empty feed waiting for startup or a tab uses placeholder rows in the same viewport, with its loading label in the footer. On a tab change, newly mounted placeholder rows fade in over about 200 ms. Delayed startup results rise 12 pt and fade in over about 220 ms; data ready before the first paint appears immediately. Tab results enter from the tab's direction using the same 24 pt slide and fade as pages. Placeholder graphics fade out over about 120 ms while readable content arrives. Tabs and controls respond immediately, reduced motion is instant, and refresh/page requests keep the rows already on screen. Empty feeds and load failures show their own status instead of placeholders.

Refresh and uncached page requests show pending status in the footer even when rows remain
visible. Saving reserves the marker's space so titles stay aligned. Summary folding and citation
changes remove outgoing text immediately and fade incoming reading content in over about 150 ms;
reduced motion switches it instantly. Help and settings keep their controls visible in short
windows, with scrolling for body content that does not fit.

Comment folds close their space with a short glide; expanded replies fade in over 150 ms.
Visible parents stay in place during keyboard folds, while offscreen selections are revealed.
Reduced motion makes both changes immediate.

Fully cached feed windows return immediately without source requests. This session cache keeps the last fetched story data; Refresh always reloads the current ranked window, while other visited windows retain their last fetched data. Missing or failed cached entries use the normal fetch path.

## Where it keeps its files

| What | Path | Owner |
|---|---|---|
| Summary settings (provider, key, profile) | `$XDG_CONFIG_HOME/hn/summary.json` | `hn_lib::reader::Config` — the same file the TUI writes |
| Saved stories | `$XDG_DATA_HOME/hn/saved.json` | `hn_lib::bookmarks` |
| Window size, zoom, inspector width, tab, page, selection | `$XDG_STATE_HOME/hn/desktop.json` | `src/layout.rs` |
| Saved article and discussion summaries, citation sources, and provider and model details. Halves older than 30 days are deleted at launch. | `$XDG_STATE_HOME/hn/summaries/<id>.{article,discussion}.json` | `hn_lib::reader::saved`, shared with the TUI |

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
The gate also runs `scripts/check_polish.py` to check transient reading-text overlap, reduced
motion, rapid reversals, short-window dialogs, saved-row geometry, and the compact browser action.

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

`scripts/package-macos.sh` stages a `.app` bundle — `Contents/MacOS/HN`,
`Contents/Resources/`, and an `Info.plist` with the keys LaunchServices reads — into
`desktop/dist/HN.app`. It follows the layout of Rune's own `scripts/mac-app`, and stops there: no
codesign, no install into `~/Applications`, no notarization, no atomic exchange.

```sh
bash desktop/scripts/package-macos.sh
open desktop/dist/HN.app
```

Locally the bundle is unsigned, so it runs only on the machine that built it.

`.github/workflows/desktop-release.yml` is the release pipeline. It runs the tests, stages the
bundle with this script, signs it with a Developer ID (hardened runtime), notarizes and staples
it, and uploads `HN-<ref>-macos-arm64.zip`. Every push to `main` keeps the zip as a workflow
artifact. A `desktop-vX.Y.Z` tag that matches `Cargo.toml`'s version also publishes it as a
GitHub Release. The workflow's header lists the signing and notarization secrets. Without them,
a `main` push uploads an unsigned zip and a tag refuses to release.

The icon is `assets/AppIcon.icns`, built from `assets/AppIcon-1024.png`. `CFBundleVersion` is
`HN_BUILD` (the CI run number) and `CFBundleShortVersionString` is `Cargo.toml`'s version. Builds
are Apple Silicon only.

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

The release measurements above were recorded at the earlier `d751f07` pin; they have not been remeasured for the current upgrade.

## Known limitations

- **Rune is pinned to a pre-release commit** (`78efd4a7`), not a published crate. Building needs
  network access to `github.com/anomalyco/rune`, and an upstream change can break this crate
  without a version bump to warn us.
- The desktop workspace mirrors Rune's required official patches for Parley, winit, wgpu-hal and both AccessKit adapters at the same revision. Rune's `tooling` feature preserves the existing release headless commands.
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
- [ ] Focus mode's less-emphasized rows are still announced.
- [ ] Inline citations announce the comment's author and open the source.

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
- [ ] Each row reads as its keys then its action ("n, space, page down — Next page").
- [ ] `Esc` and `?` both close it.

**Everywhere**

- [ ] The focus ring is visible on every focused control, and it is amber, not AppKit blue.
- [ ] Nothing is reachable by `Tab` that is not visible, and nothing visible that is a control is
      skipped — with one deliberate exception: the inspector's grip is not a Tab stop, as AppKit's
      dividers are not. It exposes a range value and a step instead, so a screen reader can still
      resize the pane.
- [ ] With **Reduce Motion** on in System Settings, opening a story and re-rooting the thread
      still land in the same place, without the glide.

Discussion replies load on demand. Opening a story captures every top-level comment for the summary; expanding a branch fetches its actual child IDs in batches of up to 50. Loaded comments stay available while further replies load. Load more and retry controls keep partial failures explicit, and folding a pending branch does not reopen it. The article metadata keeps Hacker News’s total comment count. Replies do not regenerate the summary or change its citation capture.

Refreshing comments updates roots and replies already loaded at any depth. Newly discovered replies remain available through the load controls; previously captured citation text stays attached to its summary. If a known fetch fails, the old comments remain readable and the discussion reports an incomplete refresh.

Long discussions use Rune’s variable-height virtual rows: offscreen comment text and controls are not mounted. Keyboard navigation reveals the selected row; this bounds mounted content but is not a claim about measured native scroll performance.

The discussion observes its scroll offset and keeps Rune’s frame clock active for 50 ms after movement, so successive scroll events share a running display link. Its UI wake callback and poll timer do not rebuild the view, and the window returns to idle afterward. Rebuilding a scrolled discussion may also start a short pacing interval. Native trackpad deltas and momentum are unchanged. Assess smoothness in an optimized build using native presentation timestamps (`--native --trace`), since CPU frame cost alone does not measure the cadence visible on screen.

The desktop reading view keeps the story title and metadata in one article header, aligned with the summary. The footer gives the contextual back action. Collapsing replies keeps their parent comment readable; dead, deleted, and blank leaves are omitted. An unavailable parent with replies remains a compact disclosure so readable descendants stay reachable. Those comments are also excluded from summary sources.
