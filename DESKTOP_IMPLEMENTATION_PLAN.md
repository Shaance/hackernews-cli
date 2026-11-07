# Desktop implementation plan — `hn-desktop` on Rune

Status: v4 — FINAL after 3 review rounds (Codex + DeepSeek in parallel each round; round-3 verdicts:
Codex NO-GO→fixed here, DeepSeek GO-WITH-CHANGES→fixed here — see "Review log"). Baseline committed.
MILESTONE 1 LANDED on main (ed518d3): WS-A core seams + WS-A2 reader lift + WS-B `desktop/` crate with
passing acceptance gate. Next: WS-C story split + summary, WS-D comments, WS-E settings/help/a11y.
Inputs: `mockups/desktop/DESIGN.md`, `mockups/desktop/index.html`, `prototypes/rune-hn/`,
`prototypes/rune-mocks/`.

## Goal

Native desktop frontend for `hn` on Rune (macOS first, then Linux/Windows), TUI unchanged, both
frontends sharing `hn_lib`. Milestone 1: shell + feed + Saved view, headless-tested. Later: story
split + summary (C), comments (D), settings/help/polish/a11y (E).

Non-goals: no TUI behavior/UX changes; no web target; no publishing; executors never commit/push
(the coordinator commits the baseline and merges workstreams).

## Architecture (confirmed by all review rounds)

- Standalone crate `desktop/`, bin `hn-desktop`, own empty `[workspace]`, own `rust-toolchain.toml`
  (1.99.1; root stays 1.88 — the pin binds where rustup exists, i.e. CI; locally rustc is already
  1.99.1, so local green does NOT prove 1.88 compat — CI does), committed `desktop/Cargo.lock`,
  `desktop/.gitignore` (`/target`).
- Path dep on root package; binds only to the frozen public API below.
- Deps: `rune` AND `rune-kit`, both git-pinned at rev `53c7f60f51ce12bd5da53145d9d86ce1552324e4`;
  `[profile.dev.package."*"] debug = false`.
- Root `cargo build` / `cargo test --locked` stay green and never compile Rune.

## Frozen interface (the A/B parallelism gate)

Frozen — no signature changes to existing items; additive only:

- `hn_client::{HackerNewsClient, HackerNewsItem, HackerNewsClientConfig}`
- `hn_cli_service::{HackerNewsCliService, HackerNewsCliServiceImpl, HNCLIItem}` — paging +
  row-formatting seam; desktop must NOT reimplement HTML decoding / `time_ago` / URL fallback.
  Additive trait additions (WS-A phase 0): `fetch_items_by_ids(&self, ids: &[i32]) -> Vec<Option<HNCLIItem>>`
  (implementable today via the existing per-item fetch) and a provided
  `fetch_pages_against(&self, id_snapshot: &[i32], from_rank: usize, count: usize)` composing it.
- `event::{handle_stories_key, StoryAction}` + additive `PartialEq` derive on `StoryAction`
- `bookmarks::{default_path, load, save}` + additive free `toggle_saved(store: &mut SavedStore, item)`
  (existing `toggle(&mut App)` untouched). `default_path` already honors `XDG_DATA_HOME` → tests
  isolate bookmarks with a temp `XDG_DATA_HOME`.
- `snapshot` — untouched incl. 20-id cap; desktop tests use a deterministic fake service, not
  Snapshot fixtures.
- `app::FeedPosition` visibility `pub` (additive).
- NEW `src/feed_state.rs` — lands as WS-A phase 0 BEFORE WS-B starts (branch point below).
- Root `Cargo.toml`, `Cargo.lock`, `.github/workflows/rust.yml`, `README.md` — coordinator-owned,
  frozen during A/B.

## `feed_state.rs` contract (frozen signatures; WS-A phase 0 implements exactly this)

Neutral value types, no TUI types leak. `App` KEEPS field ownership; `feed_state` holds pure
transition functions taking `&FeedState` + inputs and returning a `FeedAction` (a description of the
change), which `App` applies to its own fields. `src/ui/ui_tests.rs` stays byte-identical; root
`cargo test --locked` green.

```rust
pub struct FeedState {
    pub tab: StoryType,
    pub id_snapshot: Vec<i32>,          // stable ranked ids for the tab (refresh = explicit action)
    pub stories: Vec<HNCLIItem>,        // loaded items for the current window (mirrors App.stories)
    pub story_cache: HashMap<i32, HNCLIItem>,
    pub page_size: u32,                 // 30
    pub display_cursor: usize,          // rank offset of the first visible story (0-based)
    pub selected: usize,                // index into the visible window
    pub status: FeedStatus,             // request status ONLY (see below)
    pub stories_for: Option<(StoryType, u32)>,
    pub feed_positions: HashMap<StoryType, FeedPosition>,  // FeedPosition made pub
    pub page_landing: PageLanding,
    pub bookmark_notice: Option<String>, // separate from status, as in App/ViewStatus
}
pub struct FeedStatus { pub loading: bool, pub error: Option<String>, pub stale: bool }
// NOTE: loading/error/stale coexist (mirrors ViewStatus); they are NOT one enum.
pub enum FeedAction { /* Move, TurnPage, SwitchTab, ToggleSaved, ApplyPage{..}, Resize{..}, … */ }
pub fn fill_grid(visible_rows: usize, columns: usize, page_size: u32, display_cursor: usize) -> (usize /*from_rank*/, usize /*count*/, u32 /*pages_to_fetch*/)
```

Paging semantics (normative, cursor-aware):
- API page = 30 ranked ids from `id_snapshot` (fetch cursor). Display window = ranks
  `[display_cursor, display_cursor + visibleCells)`. Required interval = that window; fetch any
  missing intersecting 30-id pages (aligned: pages `floor(from/30) ..= floor((from+count-1)/30)`).
  Resize growth and navigation both use this rule; shrinking never drops held pages.
- Advancement: `j` past the last visible story moves `display_cursor += visibleCells`
  (rank 38 → 39 at 38 cells; 28 → 29 at 28 cells). Stepper = `display_cursor / 30 + 1` (integer div):
  11-cell and 28-cell windows show "1"; after advancing once at 28 cells (cursor 28) shows "1"
  still (28/30+1=1)... — no: stepper shows `floor(display_cursor/30)+1`; cursor 28 → "1", cursor 30
  → "2". The 38-cell case at cursor 0 shows "1" with ids 1–38 visible. No "of N".
- Rank continuity: all fetches go through `id_snapshot`; a refresh replaces the snapshot explicitly
  and re-anchors `display_cursor` to the selected story's id (selection by id, survives resize).
- Partial item failure: `fetch_items_by_ids` returns `Option` per id; a `None` renders as an error
  row placeholder — never a silent shrink. Exhaustion (snapshot shorter than window) → "no more
  stories" footer state.

Responsive feed (normative): WINDOW width ≥ 1400pt → two columns else one (matches goldens:
1180×760 → 1 col, 1600×900 → 2 cols), measured synchronously via `Cx::layout_with` on the keyed
window root (settles on first layout; WS-B's first script proof confirms window-vs-content rect).
Column-major: item i → column `i / rowsPerColumn`. One shared scroll, hairline divider,
`scroll_into_view` resolves (row, col).

## Async bridge (normative)

Desktop owns a tokio runtime. `AppSpec::setup` spawns ONE aggregate fill-grid request per
invalidation; the `rune::Latest` worker closure drives the complete async operation to completion via
the owned runtime (block_on inside the worker), and `poll` takes the settled result. Cancellation =
`Latest` generations only (`request_if_changed` / `cancel`); do NOT port the TUI's
`story_request_generation`. Accepted limitation: Latest discards superseded RESULTS but does not
abort already-running futures.

## Keyboard parity (normative)

`desktop/` owns a total Rune `Key` → crossterm `KeyCode` mapping table with a totality test over the
pinned variants; case-sensitive bindings preserved; parity binds to `event::handle_stories_key`;
per-variant `--tree` assertions scoped to tree-observable variants; selected row must be
`focusable`+`focused` in the tree.

## Workstreams and scheduling

| WS | Owns | Scope | When |
| --- | --- | --- | --- |
| A — core seams | `src/` except `src/ui/ui_tests.rs` (byte-identical) | Phase 0: `feed_state.rs` per frozen contract + `PartialEq` derive + additive bookmark fn + additive service methods + `FeedPosition` pub; root `cargo test --locked` green; COMMIT recorded by coordinator. Phase 1: `App` delegates via pure fns; tests green, ui_tests untouched. Phase 2 (A2, separate later run): lift reader into hn_lib — keep private unit tests beside lib code; move only binary-composition tests, rewritten against named public config/summary interfaces; widen `reader::{snapshot,sources}` to `pub`. | worktree cut from main; phase 0 commit is the BRANCH POINT |
| B — desktop shell+feed+Saved | `desktop/` + `.github/workflows/desktop.yml` | Milestone 1 per this plan: shell, tabs, responsive feed, saved dot+toast, Saved view (corrupt-file banner; tests isolate via temp XDG_DATA_HOME), key mapping + parity assertions, async bridge, headless goldens + scripts + motion lint, CI workflow, `desktop/verification/expected/`. | worktree cut from main AT/AFTER the phase-0 commit; parallel with A phases 1–2 |
| C/D/E | `desktop/` | per DESIGN.md | after A+B |

Parallelism holds only while: disjoint worktrees + explicit file ownership + frozen interface +
B's worktree includes the phase-0 commit. Combined A+B result gets a full root+desktop acceptance
rerun. Otherwise serialize under WS-A.

**Pre-flight gate against repo-wide constraint files** (learned from the icu/parley conflict,
enforced by `scripts/check-nested-crates.sh` + a CI `resolve` job in `.github/workflows/rust.yml`):
before any parallel writer starts — (1) run `scripts/check-nested-crates.sh` in a scratch clone
with the workstream's proposed new deps; every crate must resolve `--locked` against the frozen root
manifest; (2) any root-manifest / toolchain / CI change a workstream needs must be DECLARED in its
brief and applied to main by the coordinator BEFORE the worktree is cut (root `Cargo.toml`,
`Cargo.lock`, `rust-toolchain.toml`, `.github/workflows/*` stay coordinator-owned); (3) CI resolves
every crate on main so a conflict can never merge silently. Rule of thumb: a file that constrains
the whole repo is never workstream-owned; a workstream may only declare, not edit, such files.

## Acceptance gate (executable)

- Root: `cargo test --locked` (CI also runs it on toolchain 1.88 via rustup; local 1.99.1 green is
  not the 1.88 claim). `git diff` limited to WS-A scope.
- Desktop: `cd desktop && cargo build --locked && cargo test --locked`.
- Goldens, frozen scenario matrix (name@size, script): `feed@1040x760`, `feed@1180x760`,
  `feed-wide@1600x900`, `feed-saved@1180x760`, `story@1180x760`, `comments@1180x760`,
  `thread@1180x760`, `resize-1600-to-1100` (script), each: `mkdir -p actual && cargo run --locked
  --bin hn-desktop -- --scene … --snap actual/<name>.png --tree actual/<name>.json --size WxH
  --scale 2 --theme dark [--script … --trace actual/<name>.trace.jsonl]`. Compare: tree =
  `jq -S` canonical diff against `verification/expected/<name>.json` must be EMPTY; PNG =
  `compare -metric AE -fuzz 2% expected actual diff.png`, fail if AE > 0.5% of pixels; motion lint:
  only intended track ids move, 0 animating frames at rest, reduced-motion run
  (`HN_REDUCED_MOTION=1` → app calls `cx.reduced_motion(true)`) shows zero spatial tracks.
- Cross-platform: macOS runner compares PNG goldens; Linux (Mesa lvp) and Windows assert TREES
  semantically (no exact geometry) — cross-platform pixel equality is not claimed.
- Goldens prove stability, not fidelity: fidelity invariants (column x-positions, rank order,
  stepper text, focus flags) are separate tree assertions so a self-generated golden can't ratify a
  wrong layout.
- Static mocks are not proof of interactive capability: WS-B produces `--script` proofs (resize,
  split morph, virtualized list) at the pinned rev.

## Risks

Pre-release Rune (pin, rebase, re-run goldens; mappings drift) · packaging/notarization (bare binary:
no bundle id/Info.plist, Gatekeeper; rune's `scripts/mac-app` stages/signs; notarization unscoped,
later milestone) · supply chain (git dep → commit Cargo.lock) · size (debug 92–98 MB prototypes,
worse with hn_lib linked; measure release in E) · security/privacy (reuse `url_open`; redact API keys
from trees/diagnostics; Rune MIT compatible with hn's MIT) · a11y (headless AccessKit assertions;
VoiceOver/feel = manual pass in WS-E) · Linux/Windows deps verified on macOS only.

## Review log

- R1 (Codex+DeepSeek, parallel): accept-with-changes ×2 → v2: feed contract gate, frozen-list fix,
  Snapshot-cap bypass, reader lift (A2), WS-A acceptance, Saved+CI ownership, async bridge,
  index.html fix, risk register.
- R2 (Codex+DeepSeek, parallel): accept-with-changes ×2 → v3: full contract sketch, paging units,
  stable-id continuity op, breakpoint fixed (window ≥1400pt, `Cx::layout_with`; DESIGN.md false
  claim fixed), Latest-only cancellation, key-mapping ownership, headless reduced-motion, A2
  test-only deps, ownership gaps, acceptance gate, rune-kit dep.
- R3 (Codex+DeepSeek, parallel): Codex NO-GO, DeepSeek GO-WITH-CHANGES → v4 (this file): contract
  given frozen signatures mirroring App (status struct ≠ enum; bookmark_notice separate; FeedPosition
  pub; App keeps fields + applies FeedAction), cursor-aware fill formula with aligned pages and
  integer stepper arithmetic + corrected examples, implementable service seam
  (`fetch_items_by_ids` + provided `fetch_pages_against`, per-item failure placeholders, snapshot
  owner = feed_state), phase-0 branch point made checkable (commit hash; B's worktree includes it),
  executable gate (frozen matrix, `compare -metric AE -fuzz 2%` ≤0.5%, jq tree diff, motion/idle
  rules, temp XDG_DATA_HOME isolation, per-platform comparator policy), Latest/tokio completion path
  specified, A2 test split fixed, golden-fidelity guard via independent tree assertions, CI 1.88
  claim moved to CI.
