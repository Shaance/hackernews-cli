# Desktop implementation plan — `hn-desktop` on Rune

Status: draft v3 (after review rounds 1–2; Codex + DeepSeek, both accept-with-changes — see
"Review log"). Inputs: `mockups/desktop/DESIGN.md`, `mockups/desktop/index.html`,
`prototypes/rune-hn/`, `prototypes/rune-mocks/`.

## Goal

Native desktop frontend for `hn` on Rune (macOS first, then Linux/Windows), TUI unchanged, both
frontends sharing `hn_lib`. Milestone 1: shell + feed + Saved view, headless-tested. Later: story
split + summary (C), comments (D), settings/help/polish/a11y (E).

Non-goals: no TUI behavior/UX changes; no web target; no publishing; executors never commit/push
(the coordinator commits the shared baseline and merges workstreams).

## Architecture (confirmed by both review rounds)

- Standalone crate `desktop/`, bin `hn-desktop`, own empty `[workspace]`, own `rust-toolchain.toml`
  (1.99.1; root stays 1.88 — binds where rustup exists, i.e. CI; locally rustc is already 1.99.1),
  committed `desktop/Cargo.lock`, `desktop/.gitignore` (`/target`).
- Path dep on root package; binds only to the frozen public API below.
- Deps: `rune` AND `rune-kit`, both git-pinned at rev `53c7f60f51ce12bd5da53145d9d86ce1552324e4`;
  carry `[profile.dev.package."*"] debug = false` from the prototypes.
- Root `cargo build` / `cargo test --locked` stay green and never compile Rune.

## Frozen interface (A/B parallelism gate)

Frozen — no signature changes; additive only:

- `hn_client::{HackerNewsClient, HackerNewsItem, HackerNewsClientConfig}`
- `hn_cli_service::{HackerNewsCliService, HackerNewsCliServiceImpl, HNCLIItem}` (re-exported at
  `src/lib.rs:12`) — the paging + row-formatting seam; the desktop must NOT reimplement HTML
  decoding / `time_ago` / YC-URL fallback
- `event::{handle_stories_key, StoryAction}` — keyboard parity binds here. WS-A adds
  `PartialEq` to `StoryAction`'s derives (additive, allowed)
- `bookmarks::{default_path, load, save}`; `bookmarks::toggle` takes `&mut App` and stays TUI-only —
  WS-A adds an additive free function for the desktop save-toggle
- `snapshot` — untouched, including the 20-id validate cap; desktop tests do NOT use Snapshot
  fixtures (Saved tab is desktop-local state, not a Snapshot)
- NEW `src/feed_state.rs` — the shared feed contract, specified below; lands as WS-A phase 0
  (compilable, root tests green) BEFORE WS-B consumes it
- Root `Cargo.toml`, `Cargo.lock`, `.github/workflows/rust.yml`, `README.md` — coordinator-owned,
  frozen during A/B

## `feed_state.rs` contract (coordinator-specified; WS-A phase 0 implements exactly this)

Typed fields (mirroring what `ui_tests.rs` asserts, so WS-A extracts pure transition logic and `App`
keeps field ownership — `src/ui/ui_tests.rs` stays byte-identical):

- `tab: StoryType`, `page: u32` (fetch cursor, see below), `ids: Vec<i32>`, `selected: usize`
- load state enum: `Loading | Loaded | Error(String) | Stale` (covers `set_story_error`,
  `set_story_loading`, `showing_stale_stories`, `bookmark_notice`)
- `stories_for: Option<(StoryType, u32)>` — which feed+page is loaded
- `feed_positions: HashMap<StoryType, FeedPosition>` — per-feed remembered page/selection/scroll
- `page_landing` — the j/k page-edge landing behavior
- `initial_selection` — snapshot-driven initial selection
- save-toggle is a compound transition: `show_saved_stories` = bump generation + reload from
  bookmarks — not a plain toggle
- motion state (`selection_glide`, `feed_switched`, `outgoing_feed`) stays in `App`, NOT here
- `visible_metrics` is a PARAMETER (`fill_grid(visible_rows, columns) -> pages_to_fetch`), never stored

Transitions: `move_selection(d)`, `turn_page(d)`, `switch_tab(t)`, `toggle_saved(id)`,
`apply_page_result(...)` with stale-generation rejection, `resize(visible_rows, columns)`.
Invariants: rank continuity (no skip/dup across the prefetch boundary), exhaustion behavior defined.

## Paging and responsive-feed semantics (normative)

- API page = fetch cursor of 30 ids (`page_size` is caller-chosen; not API-imposed). TUI paging
  semantics unchanged.
- Displayed range = the prefetched buffer starting at a display cursor; advancement moves the
  DISPLAY cursor by `visibleCells` (not by 30) — `j` from rank 38 lands on rank 39. The toolbar
  stepper shows the display page number = `display_cursor / pageSize + 1`, never "x of N".
- `visibleCells = visibleRows × columns`; `pagesToFetch = ceil(visibleCells / 30)`;
  `fill_grid` refetches only when `pagesToFetch` grows; shrinking never drops held pages.
- Rank continuity: pages are fetched against a STABLE feed-ID snapshot (additive service op —
  WS-A adds `fetch_pages_against(&self, feed_snapshot_ids, from, count)` to the service trait as an
  additive default method so the TUI method is untouched). Ranking changes between requests can
  then never dup/skip. Partial item failure: a failed item yields a placeholder row (error row),
  not a silent shrink; exhaustion (fewer ids than requested) is a terminal state with a "no more
  stories" footer.
- Responsive: WINDOW width ≥ 1400pt → two columns, else one (matches the committed goldens:
  1180×760 → 1 column, 1600×900 → 2). Measured synchronously via `Cx::layout_with` on the keyed
  window root (settles on first layout; WS-B's first script proof confirms it reports window width,
  not a chrome-inset rect). Column-major: item i → column `i / rowsPerColumn`. One shared scroll,
  hairline divider, `scroll_into_view` resolves (row, col).
- Worked examples (asserted by goldens): 11 cells → ids 1–11, stepper "1", next → 12; 28 cells →
  ids 1–28 (1 page), `j` from 28 → 29; 38 cells → 2 pages (60 ids), visible 1–38, `j` from 38 →
  39, stepper "2". Resize 1600→1100 mid-page keeps the selected story (selection by id, not row).

## Async bridge (normative)

- The desktop app owns a tokio runtime; Rune callbacks stay synchronous: `AppSpec::setup` spawns
  requests onto the runtime with a `Waker`; `poll` consumes settled results.
- Cancellation: use `rune::Latest`'s own generation (`request_if_changed` / `cancel`) ONLY. Do NOT
  port the TUI's `story_request_generation` — two schemes are forbidden. A fill-grid prefetch is ONE
  request returning all needed pages coherently (a `Latest` slot replaces whole requests, so two
  separate page requests through one slot would discard the first).

## Keyboard parity (normative)

- `desktop/` owns a total Rune `Key` → crossterm `KeyCode` mapping table with a totality test over
  `Key` (it is `#[non_exhaustive]`, so the test pins current variants); case-sensitive bindings
  (`o` vs `O`) preserved.
- Parity binds to `event::handle_stories_key` / `StoryAction`. Per-variant tree assertions are
  scoped to tree-observable variants (~12); the selected row must be `focusable` + `focused` in
  `--tree`.

## Workstreams and scheduling

| WS | Owns | Scope | When |
| --- | --- | --- | --- |
| A — core seams | `src/` except `src/ui/ui_tests.rs` (byte-identical) | Phase 0: implement `feed_state.rs` contract + `StoryAction: PartialEq` + additive bookmark fn + additive service op; root `cargo test --locked` green. Phase 1: `App` delegates to pure transition fns from feed_state (field ownership unchanged; ui_tests untouched). Phase 2 (A2): lift reader into hn_lib — the five reverse deps (`toggle_bookmark`, `handle_app_message`, `request_stories`, `TerminalSession::new`, `run_app`) are test-only: move `reader/replay.rs` + `reader/tests/` to the binary test tree; widen `reader::{snapshot, sources}` `pub(crate)`→`pub`; name the config/summary interfaces C/E consume. | own worktree; phase 0 lands FIRST |
| B — desktop shell + feed + saved | `desktop/` + `.github/workflows/desktop.yml` | Everything in milestone 1: shell, tabs, responsive feed per normative section, saved dot+toast, Saved view incl. corrupt-file banner (isolated bookmark path in tests), key mapping + parity assertions, async bridge per normative section, headless golden tests (scenes + `--script` incl. resize case + `--tree` assertions + `--trace` motion lint), CI workflow (macOS Metal PNGs; Linux Mesa lvp `--tree`; Windows smoke), `desktop/verification/` with expected/ vs actual/ separated. | own worktree; starts after A phase 0 lands; parallel with A phases 1–2 |
| C/D/E | `desktop/` | per DESIGN.md sequencing | after A+B |

Coordinator owns before A/B start: commit the baseline (this plan, mockups/desktop, prototypes);
mockups/desktop edits; prototypes pin rebases; root workflow; README. Parallel writers hold only
while: disjoint worktrees + explicit file ownership + frozen interface. Otherwise serialize under WS-A.

## Acceptance gate (exact, reproducible)

- Root: `cargo test --locked` green; `git diff` limited to WS-A scope.
- Desktop: `cd desktop && cargo build --locked && cargo test --locked`.
- Headless goldens, per named scenario+dimension (e.g. `feed@1040x760`, `feed-wide@1600x900`,
  `resize-1600-to-1100`): `cargo run --locked --bin hn-desktop -- --scene … --snap actual/….png
  --tree actual/….json --size WxH --scale 2 --theme dark [--script … --trace …]`; compare
  `actual/` against committed `verification/expected/` — tree diff exact, PNG diff with declared
  tolerance, `--trace` motion lint: only intended tracks animate, 0 animating frames at rest,
  reduced-motion run (`HN_REDUCED_MOTION=1`, read by the APP itself, which calls
  `cx.reduced_motion(true)`) shows zero spatial tracks.
- Static mocks are not proof of interactive capability: WS-B produces small `--script` proofs
  (resize, split morph, virtualized list) at the pinned rev before committing to those mappings.

## Risks

Pre-release Rune (pin + rebase + re-run goldens; mappings drift) · packaging/notarization (no bundle
id/Info.plist on a bare binary; Gatekeeper; rune ships `scripts/mac-app` for staging/signing —
notarization unscoped, packaging milestone later) · supply chain (git dep, no crates.io checksum →
commit Cargo.lock) · size (debug 92–98 MB prototypes, worse with hn_lib linked — measure release in
E) · security/privacy (reuse `url_open`; redact API keys from captured trees/diagnostics; Rune MIT
verified compatible with hn's MIT) · a11y (headless AccessKit assertions possible; VoiceOver/feel =
manual pass in WS-E) · Linux/Windows deps (libxkbcommon, wayland, Vulkan; Mesa lvp headless) verified
on macOS only.

## Review log

- R1 (Codex + DeepSeek, parallel): accept-with-changes ×2. Fixed in v2: shared feed contract before
  parallel work; frozen list corrected (service/event/bookmarks); Snapshot cap bypassed via fake
  service; reader lift made explicit (A2); WS-A acceptance restated; Saved view + CI owned; async
  bridge planned; index.html column-major fixed; risk register extended.
- R2 (Codex + DeepSeek, parallel): accept-with-changes ×2. Fixed in v3: full feed_state contract
  spelled out; paging units separated (API page / display cursor / stepper) with worked examples;
  rank continuity via stable id snapshot + additive service op; breakpoint corrected to window ≥
  1400pt via `Cx::layout_with` (DESIGN.md's "no viewport accessor" claim was false — fixed);
  Latest-only cancellation; Rune→crossterm mapping owned by desktop with totality test; headless
  reduced-motion via app-set `cx.reduced_motion`; A2 test-only reverse deps handled; ownership gaps
  closed (coordinator owns spec/prototypes/root workflow/README; root Cargo files frozen);
  verification rewritten as an exact acceptance gate; rune-kit added to deps; drive-3j artifacts
  wording fixed.
- R3: pending.
