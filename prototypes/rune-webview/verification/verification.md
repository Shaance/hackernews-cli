# Verification log

Machine: this macOS host, Retina (2x), screen 1728x1117 points.
Pin: rune + rune-kit `d751f07106eac47bdd4e33756dee71e1f593c9e2`, wry 0.57.1.
Date of run: see the clock in the screenshots.

## How to reproduce

```sh
cd prototypes/rune-webview
cargo build --locked
sh scripts/verify.sh 'https://news.ycombinator.com/item?id=1'
```

`scripts/verify.sh` launches the binary with `--spike-on-top` and a `--spike-at` script, captures
the screen at six points, crops each capture to the window frame, and fails the run if any crop is
blank. Screenshots land in this directory; the app's stderr lands in `run.log`.

## Capture method, and why it is not `screencapture -l`

`screencapture -l <CGWindowID>` returns an empty buffer for a Metal-backed window in this
environment: the window is listed, the command exits 0, and the PNG is a single flat colour. So
the script captures the whole screen (`screencapture -x /tmp/rune-webview-full.png`) and crops to
the window frame reported by `scripts/window-frame.swift`, which reads
`CGWindowListCopyWindowInfo` (not accessibility-gated) and picks the highest-layer, largest window
owned by `rune-webview` — layer 3 when `--spike-on-top` is set, layer 0 otherwise.

`scripts/crop.py` does the crop at 2x and asserts content: it fails if more than 99% of the crop
is one colour. That assertion is what makes a blank capture a failed run rather than a silent
pass.

## Why the actions are scripted rather than driven from outside

This machine denies osascript assistive access (`-1719`), so no external process can resize the
window or inject a keystroke. `--spike-at SECONDS:ACTION` runs through `AppHandle::update` on the
UI thread and takes the same path a user's drag and a `w` / `u` press take: a real winit resize, a
real relayout, a real `on_layout`, and the same `toggle` / `load_remote` the key and the button
call.

## Why `--spike-on-top` is required

Rune only relayouts a resize when the window is actually painting
(`crates/rune/src/window/lifecycle.rs:225`):

```rust
let paints = shown.is_some_and(|w| !w.occluded && !w.minimized());
if self.rt.window_frame.is_some() && !(paints && cfg!(target_os = "macos")) {
    self.take_frame_request(self.frame_secs(Instant::now()), false);
}
```

An occluded window gets its NSWindow frame jumped via `set_window_frame`
(`crates/rune/src/platform/macos.rs:144`) with no relayout, so `on_layout` never fires, `set_bounds`
is never called, and the child keeps its old bounds. Keeping the window above other apps keeps it
painting, which is what makes the resize steps meaningful. This is a real integration caveat, not
a test artifact — see the README.

## Results

`sh scripts/verify.sh` output, verbatim:

```
verification/01-initial.png  1920x1360  distinct=2750  top=(21, 37, 31) 74.3%  content
verification/02-resized-larger.png  2560x1800  distinct=2638  top=(21, 37, 31) 81.9%  content
verification/03-toggled-hidden.png  2560x1800  distinct=1182  top=(18, 19, 20) 92.6%  content
verification/04-toggled-shown.png  2560x1800  distinct=2710  top=(21, 37, 31) 81.9%  content
verification/05-resized-smaller.png  1520x1120  distinct=2658  top=(21, 37, 31) 66.7%  content
verification/06-remote-hn.png  1520x1120  distinct=1725  top=(246, 246, 240) 60.6%  content  hn-orange=70794px
done  app log: verification/run.log
```

`file` output:

```
01-initial.png: PNG image data, 1920 x 1360, 8-bit/color RGB, non-interlaced
02-resized-larger.png: PNG image data, 2560 x 1800, 8-bit/color RGB, non-interlaced
03-toggled-hidden.png: PNG image data, 2560 x 1800, 8-bit/color RGB, non-interlaced
04-toggled-shown.png: PNG image data, 2560 x 1800, 8-bit/color RGB, non-interlaced
05-resized-smaller.png: PNG image data, 1520 x 1120, 8-bit/color RGB, non-interlaced
06-remote-hn.png: PNG image data, 1520 x 1120, 8-bit/color RGB, non-interlaced
```

`run.log`:

```
SPIKE at=5.0 action=Resize(1280.0, 900.0)
SPIKE at=9.0 action=Toggle
SPIKE at=12.0 action=Toggle
SPIKE at=15.0 action=Resize(760.0, 560.0)
SPIKE at=18.0 action=Remote
SPIKE load_remote url=https://news.ycombinator.com/item?id=1
SPIKE load_remote ok
```

### What each screenshot shows

| File | Window | Rune header | Child webview |
| --- | --- | --- | --- |
| `01-initial.png` | 960x680 | `local page · RUNE-WRY-CHILD-OK · child rect 0,64 960x616`, `w toggles · 0` | Marker page: `RUNE-WRY-CHILD-OK` heading, viewport `960 x 616`, clock `15:55:53`, textarea |
| `02-resized-larger.png` | 1280x900 | `child rect 0,64 1280x836`, `w toggles · 0` | Same page, viewport `1280 x 836`, clock `15:55:57` |
| `03-toggled-hidden.png` | 1280x900 | `w toggles · 1`, button reads `w show` | Absent; Rune background shows through the child region |
| `04-toggled-shown.png` | 1280x900 | `w toggles · 2`, button reads `w hide` | Same page instance, viewport `1280 x 836`, clock `15:56:04` |
| `05-resized-smaller.png` | 760x560 | `child rect 0,64 760x496`, `w toggles · 2` | Same page, viewport `760 x 496`, clock `15:56:07` |
| `06-remote-hn.png` | 760x560 | `remote page · https://news.ycombinator.com/item?id=1 · child rect 0,64 760x496` | Real HN story page: orange header, story, comment tree, footer |

### Resize proof

The child rect in the Rune header and the `viewport` value inside the page agree at every size,
and both track the window:

```
960x680  -> child rect 0,64 960x616   viewport 960 x 616
1280x900 -> child rect 0,64 1280x836  viewport 1280 x 836
760x560  -> child rect 0,64 760x496   viewport 760 x 496
```

Height is always window height minus the 64pt header, and the child's top edge stays at y=64, so
the child is pinned to the Rune element rather than to the window. The `viewport` number is read
back from `innerWidth`/`innerHeight` inside the page, so it is the web engine's own view of its
size, not a value the host wrote into the DOM.

### Toggle proof

`03` and `04` differ only in the child's visibility and the header's toggle count, and the clock
in `04` (`15:56:04`) is later than in `02` (`15:55:57`) while the page is otherwise identical. The
page was hidden and shown, not reloaded: `set_visible` maps to `WKWebView.setHidden`, so scroll
position, form state and cookies survive.

### Remote-load proof

`06` shows the real HN story page inside the child, and the crop contains 70,794 pixels of HN's
header orange. The orange is measured as `(237, 112, 45)` rather than the literal `#ff6600`
because the capture is colour-managed; `scripts/crop.py` allows for that shift.

### Keyboard focus

Clicking the textarea in the child gives the `WKWebView` first responder: typing goes into the
page, and the Rune window's `on_key` does not see those keys. That is the expected behaviour for a
native child view and it is why the header carries a clickable `w` button as well as the `w` key —
once the webview holds focus, the key no longer reaches Rune. Focus returns to Rune when the
webview is hidden or the Rune chrome is clicked.

## Headless

`--snap` and `--tree` render the Rune chrome only; the child webview is absent by construction,
because a `WKWebView` needs a real `NSWindow`. Verified:

```
./target/debug/rune-webview --snap /tmp/headless.png --size 900x600 --scale 2
  -> 1800x1200 PNG, 599 distinct colours, 89.3% one colour (the Rune background)
./target/debug/rune-webview --tree /tmp/headless.json --size 900x600
  -> 6641-byte tree, 10 layout nodes
```

## Gates

```
$ sh scripts/check-nested-crates.sh
PASS  .
PASS  ./desktop
PASS  ./prototypes/rune-hn
PASS  ./prototypes/rune-mocks
PASS  ./prototypes/rune-webview
All crates resolve against the frozen root manifest.

$ cargo build --locked
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.26s

$ cargo test --locked
test app::tests::local_page_is_synthetic_and_carries_the_marker ... ok
test result: ok. 1 passed; 0 failed

$ cargo metadata --locked --format-version 1
metadata OK
```
