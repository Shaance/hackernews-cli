# rune-webview

A spike that answers one question: **can a wry `WKWebView` be hosted as a child view inside a
Rune window at our pin, and what does the integration look like?**

Answer: yes, on macOS, with no Rune changes. The child renders, follows window resizes, hides and
shows without losing page state, and loads remote pages. The whole integration is one Rune API
(`WindowHandle::with_raw_handle`) plus wry's own child-view builder.

- Pin: rune + rune-kit `d751f07106eac47bdd4e33756dee71e1f593c9e2`, wry 0.57.1.
- Scope: macOS only. `wry` sits under `[target.'cfg(target_os = "macos")'.dependencies]`, so
  metadata resolves on Linux and Windows without webkit2gtk or WebView2 build requirements; the
  binary there prints `webview spike is macOS-only` and exits 0.
- Evidence: [`verification/verification.md`](verification/verification.md) and six screenshots in
  [`verification/`](verification/).

This is a throwaway prototype. Nothing here is wired into `desktop/`, and no upstream issue was
filed.

## Run it

```sh
cargo build --locked
cargo run -- 'https://news.ycombinator.com/item?id=1'   # "u" loads this URL
sh scripts/verify.sh                                    # the six screenshots
```

Keys: `w` hides/shows the child, `u` loads the remote URL. The header has a clickable `w` button
too, because once the webview holds keyboard focus the `w` key no longer reaches Rune.

## The integration pattern

Three pieces: get the raw window handle from Rune, build the webview as a child of it, then pin it
to a rect that a Rune element reports.

### 1. Get the raw handle

Rune hands out the platform handle through `WindowHandle::with_raw_handle`
(`crates/rune/src/window/handle.rs:133`):

```rust
pub fn with_raw_handle<R>(&self, f: impl for<'a> FnOnce(winit::raw_window_handle::WindowHandle<'a>) -> R) -> Result<R, String>
```

The handle is lent for the duration of the closure only. That is enough, because
`build_as_child` adds the `NSView` to the window's content view and keeps no reference to the
handle afterwards.

Get the `WindowHandle` from `App::on_window` (`crates/rune/src/app.rs:484`), which fires when the
window exists and again if it is recreated:

```rust
app.on_window(move |window| {
    child::close();                       // a recreated window invalidates the old child
    let native_window = window.clone();
    app_handle.update(move |state, _| {
        state.native_window = Some(native_window);
        state.child = Child::Pending;     // `poll` builds a new one
    });
});
```

### 2. Build the webview as a child

`WebViewBuilder::build_as_child` (`wry-0.57.1/src/lib.rs:1571`) takes anything implementing
`HasWindowHandle`. `raw-window-handle` implements it for `WindowHandle<'_>`
(`raw-window-handle-0.6.2/src/borrowed.rs:278`), so `&raw` works directly:

```rust
window.with_raw_handle(|raw| {
    let webview = WebViewBuilder::new()
        .with_bounds(rect(bounds))        // wry lib.rs:1477
        .with_html(html)                  // wry lib.rs:1258
        .build_as_child(&raw)             // wry lib.rs:1571
        .map_err(|error| error.to_string())?;
    CHILD.with(|slot| *slot.borrow_mut() = Some(webview));
    Ok(())
})?;
```

`build_as_child` does the `addSubview` itself. There is no Rune-side "add child" call, and none is
needed.

### 3. Position and resize

The rect comes from a Rune element's layout, via `El::on_layout`
(`crates/rune/src/el/layout_watch.rs:33`), which fires after the first layout and again whenever
the element moves or resizes by more than half a point:

```rust
el()
    .key("child-area")                    // el.rs:1194
    .w_full()
    .grow(1.0)
    .min_h(0.0)
    .min_w(0.0)
    .on_layout(|state: &mut State, rect| {
        state.child_bounds = Some(rect);
        if state.started {
            child::set_bounds(rect);      // -> WebView::set_bounds, wry lib.rs:2264
        }
    })
```

`on_layout` gives `[x, y, w, h]` in window points, origin top-left, y down. wry's `Rect`
(`wry-0.57.1/src/lib.rs:420`) is the same space: `window_position`
(`wry-0.57.1/src/wkwebview/mod.rs:1521`) converts to AppKit's bottom-left origin unless the host
view is flipped, so the rect passes through unchanged. Verified empirically — the child lands
exactly at y=64 under a 64pt header at every window size.

Use `grow(1.0)` with a zero minimum, not `h_full()`. `h_full()` makes the element as tall as the
*window*, so a child area below a header overflows the bottom edge by the header's height. That
was a real bug here: the child rect read `0,64 960x680` in a 680pt-tall window.

### 4. Hide and show

`WebView::set_visible` (`wry-0.57.1/src/lib.rs:2269`) maps to `WKWebView.setHidden`, so scroll
position, form state and cookies survive a toggle. Do not drop and rebuild the webview to hide it.

### 5. Z-order

The child is a subview of the window's content view, so it draws above everything Rune paints.
Rune's own UI cannot cover it. In this spike the header is a separate region above the child's
rect, so nothing overlaps; a design that wants Rune chrome floating over a webview would need
either a second child view above it or a Rune-side overlay window.

### 6. Threading

`WKWebView` must be created, resized and dropped on the thread that owns the window. Every entry
point here (`poll`, `on_layout`, `on_key`, `on_close`, `AppHandle::update`) runs on the UI thread,
so the view lives in a `thread_local!` `RefCell<Option<WebView>>` and no locking is needed.

## What was proven

Six screenshots, all captured from a real window and all asserted non-blank by
`scripts/crop.py`:

| Screenshot | Proves |
| --- | --- |
| `01-initial.png` | The child renders the marker page inside the Rune window at 960x680; live JS clock |
| `02-resized-larger.png` | Child follows a grow to 1280x900: rect `0,64 1280x836`, viewport `1280 x 836` |
| `03-toggled-hidden.png` | `w` hides the child; Rune background shows through; toggle count 1 |
| `04-toggled-shown.png` | `w` shows it again; same page instance (clock advanced, no reload); count 2 |
| `05-resized-smaller.png` | Child follows a shrink to 760x560: rect `0,64 760x496` |
| `06-remote-hn.png` | `u` loads a real HN story; 70,846 px of HN header orange in the crop |

The `viewport` number in each shot is read back from `innerWidth`/`innerHeight` inside the page, so
it is the web engine's own view of its size rather than a value the host wrote into the DOM.

## What did not work, and the caveats

### Rune only relayouts a resize when the window is painting

This is the one finding that matters for a real integration. In
`crates/rune/src/window/lifecycle.rs:225`:

```rust
let paints = shown.is_some_and(|w| !w.occluded && !w.minimized());
if self.rt.window_frame.is_some() && !(paints && cfg!(target_os = "macos")) {
    self.take_frame_request(self.frame_secs(Instant::now()), false);
}
```

An occluded window gets its NSWindow frame jumped via `set_window_frame`
(`crates/rune/src/platform/macos.rs:144`) with no relayout. `on_layout` therefore never fires,
`set_bounds` is never called, and the child keeps its old bounds — and because AppKit anchors a
subview's origin at the bottom-left, it visually drifts toward the window's bottom edge.

Consequence: **the child follows `on_layout`, so it only follows a resize Rune actually lays out.**
A window that is resized while occluded (or minimized) will show a stale child until the next
layout. The verification script works around it with `--spike-on-top`, which keeps the window
painting. A first-class API should not inherit this: it should position the child from the window's
own geometry, not only from a layout pass.

### `screencapture -l` returns an empty buffer for a Metal-backed window

`screencapture -l <CGWindowID>` exits 0 and writes a single flat colour for this window in this
environment. Verification therefore captures the full screen and crops to the window frame from
`CGWindowListCopyWindowInfo` (`scripts/window-frame.swift`), with `scripts/crop.py` failing the run
if the crop is more than 99% one colour. Anyone re-running this should not trust `-l` here.

### osascript cannot drive the window

This machine denies assistive access (`-1719`), so no external process can resize the window or
inject a key. The `--spike-at SECONDS:ACTION` flag exists for that reason: it runs through
`AppHandle::update` (`crates/rune/src/app.rs:731`) on the UI thread and takes the same path a user's
drag and a `w` / `u` press take.

### Keyboard focus goes to the webview

Clicking the page gives the `WKWebView` first responder; typing goes into the page and Rune's
`on_key` (`crates/rune/src/el.rs:921`) does not see those keys. That is correct native behaviour,
but it means a Rune-level shortcut stops working while the webview has focus. The header's
clickable `w` button exists for exactly this reason.

### Headless has no child

`--snap` and `--tree` render the Rune chrome only. A `WKWebView` needs a real `NSWindow`, so the
child is absent by construction. Both still work and are useful for checking the Rune side.

## IPC

The spike does not need IPC, but the two directions are:

- **Host to page**: `WebView::evaluate_script` (`wry-0.57.1/src/lib.rs:2101`), or
  `evaluate_script_with_callback` (`lib.rs:2112`) when the result matters. Wrapped in
  `webview::evaluate` here, currently unused.
- **Page to host**: `WebViewBuilder::with_ipc_handler` (`wry-0.57.1/src/lib.rs:1175`) plus
  `window.ipc.postMessage(...)` in the page. The handler runs on the UI thread, so it can call
  `AppHandle::update` directly.

For an article pane the useful shape is: page posts `{type: "open", url}` when the reader clicks a
link, host decides whether to navigate the child or hand off to the system browser, and host pushes
`{type: "theme", dark: true}` down with `evaluate_script` when the Rune theme changes.

## What a first-class Rune child-view API would need

The spike works, but every piece of it is wry-specific and macOS-specific. A Rune API that made
this a supported feature would need:

**Geometry and compositing**
- Child views attached to a Rune element, with the element's rect as the source of truth.
- Z-order relative to Rune's own drawing, not just "always on top of the content view".
- Clipping to the element's rect and to the window, including rounded corners.
- Resize driven by window geometry as well as by layout, so an occluded or minimized window does
  not leave a stale child (see the caveat above).
- Correct behaviour across display changes, full-screen, and window recreation.

**Input**
- Keyboard focus handoff: a way for Rune to know the child has focus, to take it back, and to
  route shortcuts that the child does not consume.
- Clipboard, drag and drop, and context menus.
- Scroll and gesture routing where the child and Rune chrome meet.

**State**
- Cookies and a persistent data store, with a per-app or per-pane identity.
- A way to snapshot or tear down a child without losing session state.
- Navigation policy: which URLs stay in the child, which go to the system browser.

**Platform backends**
- macOS: `WKWebView` (proven here).
- Windows: WebView2 — `build_as_child` already has a Windows path in wry, unspiked.
- Linux: webkit2gtk — likewise unspiked, and the heaviest dependency of the three.

**Lifecycle**
- Creation and teardown tied to the window, on the right thread, with a defined behaviour when the
  window is recreated.
- A headless story: what a child view means when there is no window (`--snap` / `--tree`).

## How this would land in hn-desktop

Not implemented here — this is the shape the spike suggests.

**Article pane: extracted vs live.** The reader already has an extracted-text view. A child webview
gives a second mode for the same pane: the real page, for stories where extraction loses something
(comments, code blocks, interactive content). The pane becomes a toggle:

```
article pane
├── extracted   (today: Rune-rendered text)
└── live        (child WKWebView pinned to the pane's rect)
```

The toggle is a Rune element swap plus `set_visible` on the child, so switching back and forth
keeps the page's scroll position and session. The child's rect comes from the pane element's
`on_layout`, exactly as in this spike.

**Settings: "open articles in-window".** A preference that decides whether clicking a story opens
the live page in the pane or hands the URL to the system browser. Default off, so the current
behaviour is unchanged for anyone who does not ask for it.

**What to check before committing to it.** The occlusion caveat above is the main risk: a reader
that resizes the window while another app is in front would show a stale child. That needs either
a Rune-side fix (position the child from window geometry) or a workaround in the pane. The second
risk is weight: `wry` pulls in webkit2gtk on Linux, which is a large dependency for a desktop app
that currently has none.

## Layout

```
src/main.rs       entry; URL arg, non-macOS stub, headless passthrough
src/app.rs        the Rune app: header, child region, keys, --spike-at script
src/webview.rs    the macOS host: create / set_bounds / set_visible / load_url / close
scripts/verify.sh        launch, capture six shots, assert non-blank
scripts/window-frame.swift  window frame from CGWindowListCopyWindowInfo
scripts/crop.py          crop a fullscreen capture to the frame, assert content
verification/            the six PNGs, run.log, verification.md
```

## API names at this pin vs the reference patch

The reference patch (`reisi007/prime-photos` 065abb6) hosted wry 0.57 in a Rune window at rev
`53c7f60f`. At `d751f07` the names are unchanged for everything this spike uses:

| Purpose | `d751f07` | Reference patch (`53c7f60f`) |
| --- | --- | --- |
| Raw window handle | `WindowHandle::with_raw_handle` — `crates/rune/src/window/handle.rs:133` | same |
| Window callback | `App::on_window` — `crates/rune/src/app.rs:484` | same |
| Element rect | `El::on_layout` — `crates/rune/src/el/layout_watch.rs:33` | same |
| Child creation | wry `build_as_child` — `wry-0.57.1/src/lib.rs:1571` | same |

There is **no** `add_child` or `child_view` API at `d751f07`; grep finds neither. Two similarly
named APIs are *not* this:

- `LayerHost` (`crates/rune/src/platform/embed.rs:52`) is the reverse direction — hosting Rune
  inside a `CAMetalLayer`, iOS-style.
- `NativeSurface` (`crates/rune/src/lib.rs:193`, `crates/rune/src/native_surface/mod.rs`) is a
  latest-wins video-frame surface, not a child-view host.

So the child-view integration is entirely wry-side, and the only Rune API it needs is
`with_raw_handle`.
