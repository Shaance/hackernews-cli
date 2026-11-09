# rune-hn — a Rune prototype

A standalone [Rune](https://github.com/anomalyco/rune) app: a native-UI Hacker News "top stories"
list, fetched live from the HN Firebase API with a bundled fixture fallback. It exists to answer two
questions with a running binary rather than a blog post — how does Rune work, and what would it look
like in this repo.

It is **not** a workspace member and nothing in the `hn` package references it. The root
`Cargo.toml`, `Cargo.lock` and `src/` are untouched.

## How Rune works

- **Crate layout.** `crates/rune` is the core: views, layout, animation, rendering, text and the
  native shell. Around it: `rune-motion` (springs and transitions), `rune-taffy` (a compact retained
  Taffy layout store), `rune-staging` (per-frame GPU uploads), `rune-kit` (components), `rune-fx` and
  `rune-light` (GPU effects, real-time lighting), `rune-ref` (a scalar reference renderer for pixel
  tests), `rune-mac` (macOS helpers), `rune-knobs` (live tuning), `rune-mermaid`, `rune-perf`,
  `procwatch`, `rune-register`. See `AGENTS.md` § Layout and each `crates/*/src/lib.rs` header.
- **The view is a plain function.** `fn view(s: &S, cx: &mut Cx<S>) -> El` builds a keyed element
  tree with `col()/row()/el()/text()`, `.key(..)`, layout and style methods, and
  `.on_press(|s: &mut S| ..)`. No macro DSL, no builder type per widget.
  (`crates/rune/README.md` § What exists → View API; `examples/hello/src/main.rs`.)
- **State and reactivity.** There are no signals. Handlers are queued and run between frames with
  `&mut S`; then the view re-runs and the diff retargets property tracks. Hover and press only
  retarget tracks — they never re-run the view. (`PLAN.md` § State; README View API bullet.)
- **Animation.** Every property is an analytic spring track, `Spring::new(duration, bounce)`,
  retargeted with exact position and velocity. Views produce targets; tracks animate between them,
  so a view never runs for an animation frame. (`rune-motion`; README § Animation.)
- **Layout.** Taffy 0.15 solved at the target state, text sizes rounded to whole device pixels, and
  keyed nodes keep Taffy's constraint cache between commits. Three CSS-flexbox gotchas are documented
  and all three bite: `.grow(g)` is `flex: g 1 0` (so a grown scroll viewport in an auto-height
  column collapses to 0 pt), flex items default to min-content (so a nested column overflows instead
  of scrolling), and `.virtualize` needs `.growth` on the same element. (README § Layout.)
- **Rendering.** One instanced "uber-quad" SDF pass plus a glyph atlas, on wgpu everywhere, with
  Rune's own Metal backend on Apple targets. Text is Parley 0.12 + swash 0.2 rasterised into a 2048²
  R8 shelf atlas. (`PLAN.md` § Renderer/GPU; `crates/rune/Cargo.toml` features.)
- **Tokens.** `tok::theme(dark)` returns semantic colours in OKLCH; `tok::accent(i, dark)`,
  `type_::BODY/LABEL/TITLE`, `radius::SM/MD/LG`, `motion::SNAPPY/DEFAULT/CALM`. Themes are data, not
  a trait. (`crates/rune/src/tok.rs`.)
- **App lifetime.** `AppSpec` (`crates/rune/src/tooling.rs`) carries `title`, `size`, `init`, `view`,
  `scenes`, `fonts`, `setup`, `poll` and `on_key`. `rune::run(spec)` opens one window;
  `App::new(spec)` adds lifetime controls (`windowless`, `keep_running`, `on_ready`, `on_close`).
- **Headless agent tooling.** The same renderer with no window and a fixed virtual clock:
  `--snap out.png`, `--tree out.json`, `--script in.json`, `--trace out.jsonl`, plus `--size`,
  `--scale`, `--theme`, `--at`, `--scene`. (README § Agent tooling; the full flag list is the `HELP`
  constant in `crates/rune/src/tooling/args.rs`.)
- **Accessibility is the same tree.** AccessKit drives the OS; `--tree` writes the element tree under
  `"accessibility"` (role, key, name, bounds, actions) and also lists every element with its path,
  role, text, rect, opacity and colours.

## Depending on it

Rune is pre-release and not on crates.io. It is consumed as a git dependency pinned to a `dev` rev;
`dev` is fast-forward-only and never rewritten, so a rev pin is stable.

```toml
rune = { git = "https://github.com/anomalyco/rune", rev = "d751f07106eac47bdd4e33756dee71e1f593c9e2" }
```

That rev is what this prototype pins (branch `dev`, 2026-11-09). Rune's workspace uses edition 2024,
resolver 3 and toolchain **1.99.1**; the repo root pins 1.88 for `hn`, so this crate carries its own
`rust-toolchain.toml`. It also declares an empty `[workspace]` so Cargo never folds it into the root
package.

## Platform notes

**macOS.** Default features include `metal-backend`: Rune's own Metal backend, plugged into wgpu's
`custom` backend on Apple targets. Opt out per app with `App::gpu_backend(GpuBackend::Wgpu)` or per
run with `RUNE_GPU_BACKEND=wgpu`. Shaders are **precompiled and committed**
(`crates/rune/src/uber.metallib`, `uber_native.metallib`); app builds embed them and never run
`xcrun`. `scripts/precompile-metal-shaders.sh` is only for regenerating them after editing
`clip.wgsl`/`shader.wgsl`, and needs Xcode's Metal toolchain — Xcode 27.2 is installed here, so it
would work, but nothing in this prototype needs it. `rune::prewarm_gpu()` first in `main` builds the
adapter, device and pipeline on a thread while AppKit starts.

**Linux.** Build-time dev packages: `libxkbcommon-dev libwayland-dev` (Debian/Ubuntu),
`libxkbcommon-devel wayland-devel` (Fedora), `libxkbcommon wayland` (Arch). Windowed runs also need
`libxkbcommon-x11-0`; GPU-less runs need a software Vulkan driver (`mesa-vulkan-drivers` plus
`VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json`). Headless `--snap` needs no display and no
XKB library.

## This prototype

`src/main.rs` (~200 lines). A 760×620 dark window: a header with a source badge (`live` or
`fixture`), twelve rows of rank / title / "N points · by author · M comments", and a footer hint.
The selected row gets a card fill and an accent rank.

- **Controls:** `j`/`k` or `↑`/`↓` move, `Home`/`End` jump, clicking a row selects it. Moving the
  selection calls `Tx::scroll_into_view` on the row's key.
- **Data:** `init()` fetches `topstories.json`, takes the first twelve usable items, and falls back
  to `fixtures/topstories.json` if the API is unreachable or returns fewer than three. The fetch runs
  in `init` rather than `setup`/`poll` because a headless `--snap` renders frame 0 on a virtual clock
  and would otherwise capture a loading state — a shipping app would use `AppSpec::setup` plus
  `rune::Latest` and land the answer in `poll`.
- **Fixture:** twelve real HN front-page stories captured 2026-11-07, in the same shape
  `/v0/item/<id>.json` returns, so one parser reads both. Captured through the Algolia HN API because
  the Firebase `topstories` endpoint was intermittently returning 404 during the capture.

```sh
cargo run --release                                     # the window
cargo run --release -- --snap verification/screen.png \
    --tree verification/tree.json --size 760x620 --scale 2 --theme dark
```

## Verification

`verification/` holds the artifacts; regenerate them with the two commands above and:

```sh
cargo run --release -- --script verification/select-last.json \
    --snap verification/screen-selected.png --tree verification/tree-selected.json \
    --size 760x620 --scale 2 --theme dark --at mark:end
```

`screen.png` (186 KB) is the twelve-story list; `tree.json` (52 KB) is the element tree, which
contains all twelve `story-<id>` keys and the titles as `label` names and `text` values.
`screen-selected.png` is the same list after eleven scripted `j` presses: selection on row 12, the
list scrolled to reveal it. Both runs took the fixture path, and the footer and the `source` badge
say so.

## How this maps onto hackernews-cli

1. **A native desktop companion.** A second binary that reuses `src/hn_client.rs` and the
   `src/snapshot.rs` `Snapshot` format but renders with Rune instead of ratatui. The repo already
   ships a versioned offline snapshot (`preview/fixtures/snapshot.json`) that a Rune window could
   read directly, so the data layer is done. Tradeoff: a second UI to maintain, on a pre-release
   framework, for a product whose whole identity is being a terminal app.
2. **A pixel-truth harness for the visual audits.** `VISUAL_AUDIT.md` and `preview/` describe the
   TUI's look in prose and static web mockups. Rune's `--snap`/`--tree` would render those same
   screens as real pixels with deterministic, diffable screenshots and a machine-readable element
   tree. Tradeoff: it is a parallel renderer, so it can drift from what ratatui actually draws.
3. **A model for agent-checkable UI.** The `--tree` + `--script` + `--trace` loop — dump roles,
   labels and rects, drive it with timestamped input, assert on the result — is a stronger contract
   than ratatui's `TestBackend` buffer diff, which has no roles or labels. Worth copying as a
   convention in `tests/` even without adopting Rune.

## Deliberate omissions

No comments view, no story opening, no paging, no search, no light theme, no animations beyond the
defaults, no tests. Each is a small addition; none was needed to answer the two questions above.
