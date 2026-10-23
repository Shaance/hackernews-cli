# Visual and motion audit

2026-10-23. Scope: how hn looks and moves in the feed, discussion, AI summary, source viewer, and overlays (help, links, settings). Behaviour, key bindings, and summary logic are out of scope except where copy or layout depends on them. Nothing in the code was changed.

## Verdict

hn reads well and its structure is sound. It looks like a debug build: nine ANSI hues carry overlapping meanings, selection inverts entire blocks into white slabs, and the loudest element on every reader screen is the frame around the summary, not the summary itself. Motion is a 4 fps spinner and nothing else. The fix is a small theme module, one accent colour, a tinted selection, and a frame clock fast enough to animate four or five moments. Your own `mockups/story-reader.html` already specifies most of the look; the shipped TUI drifted away from it.

## Evidence

- **Live captures, 120×40**: the real `hn` binary in a PTY against the live HN API: feed, selection, help, discussion, expanded thread, links, settings, empty Saved.
- **Recorded captures, 120×40**: the `reader::replay::terminal` binary against `tests/reader/nordwand.json` through `scripts/reader_fixture.py`: partial summary, ready summary, citation focus, source viewer, settings, profile upgrade.
- **Capture method**: `scripts/terminal_capture.py` (pyte) to styled HTML, then headless Chromium to PNG.
- **Code read**: `src/ui/{stories,comments,widgets,links}.rs`, `src/reader/{view,text,settings}.rs`, `src/event.rs`, the draw loop in `src/main.rs`, and the mockup.

Limits you should know about:

- The pyte capture drops `Modifier::DIM` and maps the 16 ANSI colours to one fixed palette. Dimmed text appears at full brightness in the PNGs, and your Ghostty theme will render the hues differently. Findings that depend on DIM come from the code, not the pictures.
- The recorded discussion fixture cites comment IDs the current sampler no longer sends, so `network.rs:118` rejects it ("Discussion citations could not be verified"). For the summary captures I remapped each point's citations to sampled IDs. The rendering is real; the summary content is not a real model output.
- `scripts/replay_reader.py` fails on this checkout at both 120×40 and 200×60 at its third step (`count() == 40`), for the same reason. This predates the audit and is unrelated to style; it is listed under side findings.

## Decision 0: palette strategy

Every visual finding below depends on this, so decide it first.

The code uses only named ANSI colours (`Color::Yellow`, `Cyan`, and so on). There is no `Color::Rgb`, no `Color::Indexed`, and no `NO_COLOR` handling anywhere in `src/`. That respects the user's terminal theme, but it caps the design at sixteen colours whose exact values you never control, and it rules out the single most effective change available: a tinted selection background.

| Option | What it gives | Cost |
|---|---|---|
| A. Stay ANSI-16 | One accent (Yellow or LightRed), `DarkGray` background for selection. Works everywhere. | Selection tint looks different in every theme; no light-mode tuning; no colour interpolation for motion. |
| B. Truecolor tokens only | The mockup's tokens exactly: `--hn-fg`, `--hn-dim`, `--hn-line`, `--hn-accent`, `--hn-sel`, `--hn-link`. Enables fades. | Must know light or dark background; breaks on the rare terminal without truecolor. |
| **C. Tokens with fallback (recommended)** | Truecolor tokens when `COLORTERM` is `truecolor` or `24bit` (Ghostty sets it) **and** an OSC 11 query (or `COLORFGBG`) reports the background; ANSI-16 mapping of the same roles otherwise, including whenever the background is unknown. `NO_COLOR` strips colour and keeps bold, dim, and reverse. (Revised in the second pass: the first draft guessed dark when unknown.) | One `theme.rs` with about eight roles. Every `Color::` literal in `ui/` and `reader/` moves to a role. |

Why C: Ghostty is the primary target (the repo ships `scripts/ghostty/hn-article.conf`), so truecolor is the normal case and the mockup's palette can ship as designed. The fallback keeps tmux and older terminals readable. A theme module with eight roles used in about forty call sites clears the three-usage bar for a new abstraction.

Proposed roles, taken from the mockup's dark values:

| Role | Dark | Light | Used for |
|---|---|---|---|
| `fg` | `#e1e5e5` | `#24292c` | Titles, comment text, summary text |
| `dim` | `#a1acb0` | `#616b70` | Metadata, timestamps, hints, section labels |
| `line` | `#353d40` | `#d4d9d8` | Rules, thread guides, borders |
| `accent` | `#f3b77b` | `#995015` | Selection bar, active tab, keys in hints, OP badge, ★ |
| `sel` | `#25322f` | `#e9eeec` | Selected row background |
| `link` | `#80c9d5` | `#006b79` | Citations, URLs |
| `error` | soft red (not in mockup) | darker red | Failures and rejected keys only |

Green, blue, magenta, and the six-hue depth palette go away entirely.

## Findings

Severity is about how much each problem costs the look, not correctness. P1 means the screen reads as unfinished because of it.

### 1. P1: selection inverts whole blocks into white slabs

- **Observed**: in the discussion, the selected comment's author, timestamp, every line of text, the tree guides, and the "▾ Collapse" line all render reverse-video bold (`ui/comments.rs` `apply_highlight`, `highlight_style`). In the feed, the selected story inverts three rows: title, metadata, and the blank spacer line that belongs to the same `ListItem` (`ui/stories.rs`, the `highlight_style` on `List` plus the spacer pushed into `lines`). Both are visible in the live captures.
- **Inferred**: reverse video is the brightest thing a terminal can draw. A ten-line selected comment becomes a white rectangle that is harder to read than its unselected neighbours, which defeats the purpose of reading it.
- **Proposal**: selection = `sel` background on the row's full width + a one-column `accent` bar at the left edge (`▎` or `┃`) + text unchanged. Never reverse. Move the feed spacer out of the `ListItem` (or use `List::highlight_spacing` with explicit padding) so only content rows highlight. This is exactly the mockup's `.story.selected` and `.selected-comment`.
- **Verify**: select a long comment; its text keeps normal contrast and the guide column stays `line`-coloured, with only the bar in `accent`.

### 2. P1: nine hues, overlapping meanings

- **Observed**: Yellow alone means selection marker, feed name, comment count, collapsed state, summary frame, summary section labels, AI status line, "ORIGINAL POST", and the current-feed status. Green means points, expanded state, and "Summary ready". Cyan means authors, "DISCUSSION", "fetching", the active citation point, and the source header. Blue means loading. Thread guides cycle Gray, Cyan, Green, Yellow, Magenta, LightBlue by depth (`depth_color`).
- **Inferred**: when every element is coloured, colour stops carrying information. The feed metadata row alone has four hues (cyan author, green points, yellow comments, default time).
- **Proposal**: two-level hierarchy (`fg` for content, `dim` for everything about content) plus one `accent` reserved for "you are here" and "press this". Depth comes from indentation and `line` guides, not hue. Status colours appear only when something is wrong.
- **Verify**: count distinct foreground colours on a feed screen; target is four or fewer (fg, dim, line, accent).

### 3. P1: the shipped look diverged from your own mockup

- **Observed**: `mockups/story-reader.html` defines one accent, fg/dim/line tokens with light and dark pairs, selection as a tinted background with a 2 px inset accent bar, dim section labels with letter-spacing, citations in a link colour, and thread guides as a single thin `line` rule. None of those made it into the Rust renderer.
- **Inferred**: the summary UX work (`SUMMARY_UX_AUDIT.md`) concentrated on state and recovery; the visual layer was never ported.
- **Proposal**: treat the mockup as the spec. Findings 1, 2, 5, and 7 are mostly "do what the mockup does".

### 4. P2: feed hierarchy and density

- **Observed**: the title row and metadata row have nearly the same weight; metadata uses heavy `│` separators while the reader uses `·`; "1 days ago" and "1 hours ago" (`time_utils.rs:17-18`, asserted in tests); rank numbers are left-aligned so titles shift when ranks go from 9 to 10; the title bar says "Best stories │ Page 1" and the status bar repeats "showing Best · p1" in yellow directly below; a loading spinner shows in both bars at once.
- **Proposal**:
  - Header becomes a tab strip that keeps its keys: `1 Top  2 New  3 Best  4 Saved`, active tab in `accent` with an underline, page number at the right in `dim`. Drop the duplicate in the status bar. (Revised in the second pass: the first draft dropped the digits.)
  - Rank right-aligned in a fixed `dim` column, so titles align.
  - Title in `fg`, with ★ in `accent` when saved.
  - Metadata in `dim`, separated by `·`, except the comment count, which stays `fg` because it is what people scan to pick a discussion: `quad9.net · farrokhi · 1224 points · `**`412 comments`**` · 20h`. (Revised in the second pass.)
  - Compact relative time (`20h`, `1d`), which also fixes the pluralisation.
  - Optional brand mark: a ` Y ` block on an orange background at the far left of the header, the one place a brand colour belongs.
- **Verify**: at 80 and 120 columns, titles align across ranks 1–30 and the header names the feed once.

### 5. P2: discussion thread chrome is noisier than the comments

- **Observed**: each comment carries a coloured elbow (`├─`), a state glyph (`▸`/`▾` in yellow, blue, or green), a cyan author, a `•` separator, and, when expanded, a green "▾ Collapse" line. The original poster is not distinguished. Collapsed replies say "▸ 2 replies" in dim yellow.
- **Proposal**:
  - Guides: a single `│` in `line` at each depth, with no elbows and no hue.
  - Header: author in bold `fg`; an `OP` badge in `accent` when the author is the story's author (HN readers rely on this); time in `dim`.
  - Remove the "▾ Collapse" line. Collapsed threads end with `+ 2 replies` in `dim`, and the selection bar shows the key.
  - Quotes (`> …` lines, common on HN) in `dim` with a `line` left rule, so quoted text stops competing with the reply.
  - URLs in `link`, truncated with `…` in the middle rather than at the end.
- **Verify**: expand three levels; the guides form calm vertical rules and the only `accent` on screen is the selection bar and any OP badge.

### 6. P2: reader chrome takes six rows and says too much

- **Observed** (`reader/view.rs`): the title starts at column 0 with no gutter, while the feed uses one column. The AI status line packs state, action, profile, Auto, and a hint into one yellow run: "AI summary · S Set up summaries · Not requested · Auto on · M Settings". With no key configured it reports "Auto on", which cannot happen. Two footer lines of key hints render at full brightness.
- **Proposal**:
  - A two-column gutter on every screen.
  - Header: title in bold `fg` (wrapping to two lines max, as now), metadata in `dim`, and on the right of the metadata row a compact summary state chip: `● Fast` ready, a spinner plus `Fast` while working, `! retry` failed, or `set up` with no key.
  - One footer line: keys in `accent` and labels in `dim` (`j/k` nav · `Tab` sources · `S` summary · `M` settings · `?` help). Transient notices replace it temporarily (see motion).
- **Verify**: the reader uses three chrome rows instead of six, and nothing in the header contradicts the configuration.

### 7. P2: the summary frame is louder than the summary

- **Observed** (`reader/text.rs`, `reader/view.rs`): every summary line starts with a yellow `│ `; the panel closes with a full-width yellow `╰───…`; section labels "ARTICLE · Fast" and "DISCUSSION · Fast · Tab Sources · Enter Open" are yellow uppercase; the active point is marked with `>` and turns cyan; the focused citation is reverse-video bold cyan; "Captured 11:41" and coverage sit in DarkGray between paragraphs.
- **Proposal**:
  - The panel uses a `line`-coloured left rule, or a very faint `sel` background, and no closing rule. A blank row and the `DISCUSSION` label separate it from comments.
  - Section labels in `dim` uppercase; the profile becomes a small `dim` suffix; hints move to the footer.
  - Points use `•` in `dim`. The active point gets the same accent bar as selection everywhere else, so selection has one visual language.
  - Citations in `link`. The focused citation in `accent` with underline, not reverse.
  - Capture time and coverage merge into one `dim` line under the label: `28 excerpts (partial) · 11:41`.
- **Verify**: on a ready summary, the brightest text is the points themselves.

### 8. P2: overlays look like debug panels

- **Observed**: help is a 74-column box with no inner padding, square corners, and no backdrop; its key column is not aligned ("j/k/↑/↓  Select story" next to "1 Top · 2 New · 3 Best · 4 Saved"). Settings fills the entire terminal with a ten-line form and roughly 25 empty rows, its title flush against the border, the cursor a plain `>`. The links picker also fills the screen.
- **Proposal**:
  - All three use one modal recipe: centred, sized to content (settings about 64×16), `BorderType::Rounded` in `line`, title in `accent` with one space of padding, inner padding of one row and two columns. The screen behind is redrawn in `dim` as a scrim.
  - Help becomes a two-column table: keys right-aligned in `accent`, actions in `fg`, grouped under `dim` headings.
  - Settings rows show the label in `dim` and the value in `fg`, with `‹ Fast ›` arrows on the focused choice. The focused row gets the selection bar.
- **Verify**: open each overlay at 80×24 and 200×60; none touch the screen edges, and the content underneath is visibly dimmed.

### 9. P3: empty, loading, and error states disagree

- **Observed**: "Loading stories..." and errors are vertically centred; the empty Saved message is top-left at column 0. Errors read "Error: …" in bold default colour.
- **Proposal**: one centred empty-state recipe: a `dim` glyph, one `fg` line, one `dim` hint line (`No saved stories yet` / `Press s on any story`). Errors use `error` for the glyph only and keep the text `fg`.

### 10. P3: key hints are plain text

- **Observed**: hints are a single run of default-coloured text, sometimes with DIM over the whole line.
- **Proposal**: keys in `accent`, labels in `dim`, two spaces between groups. This one change makes every screen look intentional.

## Motion

### Prerequisite: there is no frame clock

`EventHandler::default()` (`src/event.rs`) polls input for 250 ms and the loop only redraws after each poll returns. Consequences, observed in code:

- The braille spinner advances at 4 fps. A smooth braille spinner needs about 12 fps (80 ms).
- Network results arrive on a channel but are only drawn after the current poll times out, up to 250 ms late.
- `tick_count` only advances on idle ticks, so the spinner freezes while a key is held down.
- Nothing can interpolate; there is no notion of elapsed time.

Fix: draw from a time-based clock instead of a tick count. Poll with a short timeout (about 16 ms) only while something is animating or pending, and block on input-or-channel otherwise. In practice that means `tokio::select!` over crossterm's `EventStream` (needs crossterm's `event-stream` feature) and the app message channel. Animations read `Instant::now()` and settle to their final state on the first frame after their duration. Rendering must never wait for an animation to finish, and state changes stay instant.

Without this clock, none of the motion below is possible, so it is phase 1 work, not polish.

### Candidates

Each candidate has a verdict. Durations assume the clock above. All of them are skippable under `HN_REDUCED_MOTION=1` (or a settings toggle), which snaps every animation to its end state.

| Moment | Today | Proposal | Verdict |
|---|---|---|---|
| Loading | 4 fps spinner, shown in both title and status bar | One 80 ms braille spinner in one place per screen | **Do**: free once the clock exists |
| Summary arrives | Panel jumps from 4 rows to about 12 and pushes comments down 8 rows in one frame (captures 02 → 03) | See second-pass finding 2: if the reader has moved, keep the panel at one row (`✓ Summary ready · S show`) so nothing moves. When it does expand (still at the top, or on `S`), ease the height over about 180 ms and fade new text from `dim` to `fg` over about 250 ms, 20 ms apart | **Do**, behind the no-move rule |
| Overlay open/close | Instant | Scrim fades the background to `dim` over about 120 ms; the modal appears at once | **Do**, if truecolor; with ANSI-16 use the static scrim only |
| Transient notices | "Opening article…" stays until something replaces it | Informational notices only: shown in `accent`, fade to `dim` after 2 s, clear at 4 s. Errors and anything with a recovery hint stay until acted on (second-pass finding 6) | **Do** |
| Save toggle | ★ appears | ★ flashes bright `accent` for 300 ms, then settles | **Do**: tiny, and confirms the action |
| Expand replies | New children appear instantly | Children render `dim` then fade to `fg` over 150 ms, so the eye finds them | **Maybe**: try it, cut it if it feels slow |
| PgUp/PgDn | Jumps a page | Scroll interpolates over 4–5 frames (about 80 ms) | **Maybe**: only for page jumps |
| j/k selection | Instant | Animated slide | **Reject**: key repeat must feel instant; animating it adds latency |
| Fake token streaming / typewriter | n/a | Reveal summary text character by character | **Reject**: there is no real streaming, so this would misrepresent latency |
| Loading skeletons | Content dims while reloading | Placeholder rows | **Reject**: the current dim-while-loading is already the right pattern |

Library option: `tachyonfx` provides ratatui effects (fade, sweep, dissolve) on a time clock. It would cover the summary reveal, scrim, and notice fade. Check that its release supports ratatui 0.29 before adopting it; otherwise, the three interpolations above are about 60 lines of hand-written colour lerp in the theme module.

## Plan

Each phase ships on its own and is verified by re-running the captures used for this audit.

1. **Clock and theme**: event loop on `select!` with an adaptive frame rate; `theme.rs` with the roles above and truecolor/ANSI/`NO_COLOR` selection; replace every `Color::` literal. No visual change beyond the spinner yet, so this phase is easy to review.
2. **Selection and hierarchy**: findings 1, 2, 4, 5, 10. This is where it starts looking good.
3. **Reader and summary**: findings 6 and 7, plus the summary-arrival motion.
4. **Overlays and states**: findings 8 and 9, plus the scrim, notice, and save motion.

### Test coupling to budget for

Rust unit tests assert strings and never colours (no `Color::` or `Modifier` in `ui_tests.rs` or `reader/*tests*.rs`), so most style changes are safe. Copy changes are not, and the PTY replay is tightly coupled to glyphs and styles:

- `replay_reader.py` matches `"▸ 60. Synthetic feed control 60 ★"`, `"Summary ready"`, `"S View"`, `"ARTICLE · Fast"`, `"DISCUSSION · (\d+) visible"`, `"> Retry failed summary"`, `"Source 3 of 8 · …"`, and splits the screen on a line starting with `╰`.
- Its `focus()` helper finds the focused citation by looking for cells that are cyan, reverse, and bold, which finding 7 removes on purpose.
- `time_utils.rs` tests assert "1 hours ago" and "1 days ago".

Update these alongside each phase rather than at the end.

## Side findings (not style)

- **Replay fixture drift**: `tests/reader/nordwand.json` cites comments that the current sampler no longer sends, so the recorded replay fails citation verification and `scripts/verify-reader.sh` cannot pass. Re-record the fixture or make its citations follow the sampled set.
- **Stale relative times**: `time_ago` is formatted once at fetch time and stored as a string, so "20 hours ago" never advances during a long session.
- **Untracked files**: `pbpaste` (empty) and `.DS_Store` in the repo root.

## Second pass: product and UX

2026-10-23, same day. This pass asks whether the proposals above help someone who opens hn to pick a story, read an argument, and get back to the feed. It reuses the first pass's evidence and adds live captures at 80×24, the size most terminals open at. Two findings correct the first pass's own recommendations; the edits above are marked "Revised in the second pass".

### 1. P1, correction: guessing a dark palette makes text invisible on light terminals

- **Moment**: first launch in a light-themed terminal where background detection fails (tmux without OSC 11 passthrough, SSH sessions, many Linux terminals).
- **Observed**: hn paints foreground colours only; nothing in `src/` fills the background. The first draft of option C fell back to dark tokens when the background was unknown.
- **Consequence**: `#e1e5e5` text on a white terminal. The user sees a nearly blank screen and has no way to fix it from inside the app.
- **Correction**: use truecolor tokens only when the background is known. When it is unknown, use the ANSI-16 role mapping, which follows the user's theme by construction. The comparison page's light toggle paints a light background that the real app would not paint, and fixed `sel` and `line` values assume a near-neutral background, so tinted themes such as Solarized or Gruvbox need a check.
- **Verify**: launch in a light Ghostty theme, in tmux with passthrough off, and with `NO_COLOR=1`; all three stay readable.

### 2. P1, correction (editable assumption): the summary arriving moves what you are reading

- **Moment**: you open a story, start reading comments, and the automatic summary finishes a few seconds later.
- **Observed**: captures 02 → 03 show the comment area dropping about 8 rows when the panel expands. `SUMMARY_UX_AUDIT.md` records that you rejected anything that resets reading position. The first pass proposed easing the jump, which softens it but still moves the text.
- **Consequence**: you lose your line mid-sentence, at the moment the app is supposedly helping.
- **Correction**: if the comment cursor or scroll has moved since the story opened, keep the panel at one row reading `✓ Summary ready · S show`; expand automatically only when you are still at the top. `reading.shown` and the `S` toggle already exist, so this is one condition, not a new mode. The full summary stays pinned above comments as you chose; only the arrival moment changes, and the expand animation applies when it does open.
- **Decided (2026-10-23)**: once the reader has moved (any `j/k` or scroll since opening), the summary stays collapsed to its one-row chip until `S`. Implemented in phase 3.
- **Verify**: open a story, press `j` three times, release the summary in the replay fixture; the selected comment stays on the same screen row.

### 3. P2: dimming all metadata hides how big each discussion is

- **Moment**: scanning the feed for a discussion worth reading.
- **Observed**: the first-pass mock dimmed the whole metadata row to one weight.
- **Consequence**: comment count is the main signal for a discussion reader. At one weight with host and author, you read every line to find the big threads.
- **Correction**: comment count in `fg`, everything else `dim`. Applied to the mocks.
- **Verify**: in the proposed feed, find the largest discussion without reading the hosts.

### 4. P2: tabs without numbers hide the keys

- **Moment**: switching feeds.
- **Observed**: the first-pass tabs read `Top New Best Saved`. The digits that switch feeds (`1`–`4`) appeared only in help. hn opens on Best (`app.rs:134`).
- **Consequence**: the most visible navigation in the app doesn't teach its own keys.
- **Correction**: `1 Top  2 New  3 Best  4 Saved`, digit in `dim`, active tab in `accent`. Applied to the mocks.

### 5. P2: the design was only checked at 120×40

- **Moment**: everyday use in a default-size terminal.
- **Observed** (live capture, 80×24): six stories fit, the longest metadata row is cut off ("1 days a"), the footer is cut off ("· q"), and three rows sit empty because a three-row story didn't fit. The footer's short-hint branch only triggers below 80 columns, so exactly 80 truncates. In the reader, six chrome rows leave 18 for reading.
- **Consequence**: the most common window size gets the weakest version of every screen.
- **Correction**: below about 30 rows, drop the blank spacer between stories; the tinted selection already separates them, and 80×24 then fits 10 stories instead of 6. Give each footer a priority order and stop at the first hint group that doesn't fit, never cutting mid-word. Truncate long titles with `…`. The comparison page now has both 80×24 screens.
- **Verify**: feed and discussion at 80×24 and 120×40; nothing is clipped mid-word, `?` is always visible, and the feed shows at least 10 stories at 80×24.

### 6. P2: auto-fading notices would swallow errors

- **Moment**: saving a story fails, or the article pane fails to open.
- **Observed**: the first pass faded every notice after 2 s. `bookmarks.rs:75` puts an error with recovery instructions ("Saved: 4, r retry") into the same notice slot. `browser_notice` says "Opening article…" and never reports the outcome.
- **Consequence**: a failed save disappears before you read it, so you believe the story is saved.
- **Correction**: fade only informational and success notices. Errors and anything carrying a recovery hint stay until acted on or replaced. "Opening article…" resolves to "Article opened" or the failure.
- **Verify**: make the saved file unwritable, press `s`, wait 10 s; the error is still visible.

### 7. P3: "set up" appears on every story for people who don't want summaries

- **Moment**: every discussion opened without an API key.
- **Observed**: today's header reads "AI summary · S Set up summaries · Not requested · Auto on · M Settings"; the first-pass chip kept "S set up".
- **Consequence**: a permanent call to action for an optional feature that some users have deliberately declined.
- **Correction**: with no key, show `S summary` among the footer keys in `dim` and nothing in the header. `S` still opens setup. No new state needed. Applied to the mocks.

### Optional, not a finding

`App.reading_sessions` already records which stories you opened this run. A `dim` "opened" marker in the feed would use that existing state and fits the README's promise that reopening restores your place. It is a new visible surface, so it should be agreed before it is built. A "summary ready" marker in the feed was considered and rejected for now: summary state lives in the binary's `Reader.readings`, and the library's feed renderer would need new plumbing to read it.

Withheld below the seven-finding cap: whether collapse state stays clear once the "▾ Collapse" line is gone, contrast ratios for `dim` text on `sel`, and how long titles wrap in the reader header.

## Reuse before rebuild

2026-10-23. For each piece of planned work, what already exists. Versions, release dates, and dependency requirements come from the crates.io API; APIs come from the published crate sources, not READMEs (the tachyonfx README still documents 0.7).

### The deciding fact: upgrade ratatui first

Every current release in the ratatui ecosystem targets ratatui 0.30 (through `ratatui-core` 0.1). On 0.29, hn is stuck with releases from 2024–2025: the last tachyonfx for 0.29 is 0.21 (December 2025; current is 0.25.2), and the last tui-popup is 0.6.0 (October 2024; current is 0.7.7).

The upgrade was tried on a scratch copy of `HEAD`: `ratatui = "0.30"`, `crossterm = { version = "0.29", features = ["event-stream"] }`, `rust-version = "1.88"` (ratatui 0.30.2 requires Rust 1.88; `rust-toolchain.toml` is already 1.88). `cargo check --all-targets` finished with no errors or warnings, and `cargo test` passed 39 + 52 tests with 1 ignored, identical to the current checkout. The only 0.30 breaking changes that touch hn are a renamed `Alignment` (a type alias keeps it compiling) and `TestBackend`'s error type, which the tests tolerate.

A second trial added every adopted crate together (`tachyonfx` 0.25.2 with only `std`, `tui-popup` 0.7.7, `terminal-colorsaurus` 1.0.3, `supports-color` 3.0.2, dev `insta` 1.49.0). It resolved on Rust 1.88, `cargo check --all-targets` was clean, and `cargo tree -d` shows no duplicate ratatui or crossterm.

### Decisions

| Need | Use | Verdict | Evidence |
|---|---|---|---|
| ratatui and crossterm | ratatui 0.30.2, crossterm 0.29.0 with `event-stream` | **Do first** | Trial upgrade above: clean build, same test results |
| Light or dark background | `terminal-colorsaurus` 1.0.3 | **Adopt** | `theme_mode(QueryOptions)` with a configurable timeout (default 1 s). Detects terminals that can't answer through a DA1 query instead of waiting. No crossterm dependency. Its supported Rust versions follow bat and delta. Call it once before entering raw mode; on error or timeout, use the ANSI-16 roles |
| Truecolor support and `NO_COLOR` | `supports-color` 3.0.2 | **Adopt** | `on_cached(Stream::Stdout)` returns `has_basic / has_256 / has_16m`. Handles `NO_COLOR`, `FORCE_COLOR`, `COLORTERM=truecolor/24bit`, and `TERM=*direct`. 75M downloads |
| Theme roles | Our own `theme.rs`, about 60 lines | **Build** | The roles and the fallback rule are hn-specific. `ratatui-themes` 0.3 (20k downloads) was checked only by its crates.io metadata and was not evaluated further |
| Fades, scrim, notice fade, save flash | `tachyonfx` 0.25.2, with default features off except `std` (drops the `dsl` parser) | **Adopt** | `fx::fade_from_fg`, `fx::fade_to_fg`, `fx::fade_to`, plus `EffectManager::add_unique_effect(key, fx)` (re-triggering replaces instead of stacking) and `process_effects(elapsed, buf, area)` after widgets render. It blends between RGB values, so skip effects in ANSI-16, `NO_COLOR`, and reduced-motion modes. The summary panel growing row by row is a layout change, not an effect, so that part is our own code |
| Event loop that waits on input and results | Pattern from ratatui's official `event-driven-async` template | **Copy the pattern** | A task runs `tokio::select!` over crossterm's `EventStream`, a `tokio::time::interval` tick, and app events, all feeding one channel. It is a template, not a crate. Change: run the interval only while something animates or loads |
| Spinner | Keep the existing 10-frame braille array | **Reject** `throbber-widgets-tui` 0.11.1 | It adds a stateful widget for what is three lines today. The fix is the frame clock, not the spinner |
| Help, settings, and links modals | `tui-popup` 0.7.7 | **Adopt** | Centres, clears, sizes to its `Text` body, title, `border_set` (rounded), `style`, `border_style`. No padding option, so pad the body. The links list needs `KnownSizeWrapper`. Three overlays meet the three-use bar |
| Padding, rounded borders, clearing, centring, list highlight spacing | ratatui built-ins: `Block::padding`, `BorderType::Rounded`, `Clear`, `Layout` flex, `List::highlight_spacing` | **Built in** | Already in ratatui; nothing to add |
| Comment threads | Keep the custom renderer | **Reject** `tui-tree-widget` 0.24.1 | Its items are prebuilt `Text`, and `TreeState` selects and scrolls by item. hn loads children lazily (`CommentState::Loading`), scrolls by line inside long comments (PgUp/PgDn), and restores positions between visits, all already implemented and tested. Adopting it would replace working state handling, not styling |
| Key-hint footer that drops hints by priority | Own code, about 20 lines | **Build** | No candidate found in this pass. Search coverage was limited because Codex could not run (see below) |
| Style regression tests | `insta` 1.49.0 snapshots of ratatui `Buffer` | **Adopt** for render-level tests | `Buffer`'s `Debug` output lists cell styles (`fg`, `bg`, `modifier`) wherever they change (checked in `ratatui-core` 0.1.2 source), so snapshots catch colour regressions that string assertions miss. Keep the PTY replay for end-to-end behaviour |

### Detection and the test harness

The PTY harness (`scripts/terminal_capture.py`) runs hn inside pyte, whose `write_process_input` is a no-op, so it never answers the OSC 11 or DA1 query. Two consequences for phase 1:

- Use a short detection timeout (200–300 ms) rather than the 1 s default. A timeout falls back to the ANSI-16 roles, which are always readable, so the only cost of a slow terminal is a plainer palette.
- Add an `HN_THEME=light|dark|ansi` override. Users in tmux without OSC passthrough, or over SSH, need it as the recovery path from second-pass finding 1, so it is a user-facing setting rather than a test hook. The harness can use it to exercise the truecolor path.

### What this removes from the plan

- Phase 1 becomes: upgrade (3 lines), detection with `terminal-colorsaurus` and `supports-color`, `theme.rs`, and the select-based event loop. No hand-written OSC 11 query, environment parsing, or colour interpolation.
- Every fade in the motion table becomes a tachyonfx effect. The only hand-written motion left is the summary panel's row-by-row growth.
- All three overlays use `tui-popup` instead of hand-built centring and `Clear` calls.

### Research gap

Codex could not run commands on this machine during this pass. The Homebrew upgrade left both `codex` and its helper binary `codex-code-mode-host` hanging under a stuck macOS Gatekeeper daemon (`syspolicyd`). The `fix-codex-install` workaround unblocked `codex`, but Codex finds the helper by a fixed path, so that workaround can't reach it, and disabling `code_mode_host` disables Codex's shell entirely. Running `sudo pkill syspolicyd` (or rebooting) clears it. All findings above come from crates.io, crate sources, ratatui's repository, and the trial build.

## Implementation status and final review

2026-10-23. All four phases are on branch `codex/visual-phase1`:

| Commit | Phase |
|---|---|
| `32e744d` | 1: ratatui 0.30, theme detection and roles, event-driven frame clock |
| `fbb298e` | 2: feed and thread restyle, tinted selection, 80×24 layout |
| `23e230e` | Replay harness made reliable again (fixture connection queue, spinner-tolerant waits, wide-glyph stubs) |
| `01f5979` | 3: reader chrome, status chip, summary panel, collapse rule |
| `1ae90ed` | 4a: settings, help and links as modals over a scrim; feed states |
| `7052194` | 4b: motion with `HN_REDUCED_MOTION` |

Verified on `7052194`: fmt, clippy with warnings denied, 47 library and 65 binary tests, `scripts/verify-reader.sh` at 240×80 and 200×60, the paid `scripts/smoke.py` with both providers (OpenRouter on every phase, Anthropic once), 0.01 s CPU and no output over 10 idle seconds, and 8 redraws in 144 ms when an overlay opens (1 with reduced motion).

Two premises in this audit were already stale when phase 4b was implemented: the frame-clock finding (fixed in phase 1) and the claim that browser notices never report an outcome (`Reader::poll` already reports success and failure).

### Final review findings

From live captures of the full journey in dark, light and 16-colour at 120×40 and dark at 80×24, plus unsettled frame samples of each animation.

1. **P2, backdrop fade passes through yellow-green.** Sampled at 60 ms after opening settings: `#d0df5b` on 28 cells. tachyonfx interpolates in HSL by default, so orange (≈30°) travels through yellow and green toward the blue-grey scrim (≈195°). Correction: `.with_color_space(ColorSpace::Rgb)` on every effect. Verify: frame samples contain no hue outside the orange-to-grey band.
2. **P2, the summary starves comments at 80×24.** The panel takes two thirds of the body (11 rows), leaving 5 comment rows and cutting the summary mid-sentence; the `J/K scroll` row also touches the `COMMENTS` heading. Correction: keep at least 8–10 comment rows on short terminals (two thirds stays the cap on tall ones), and always leave a blank row before the heading. Verify at 80×24.
3. **P2, offline feed error is technical and shown twice.** "Failed to load stories: Failed to get story IDs for type: best" appears centred and in the footer. Correction: centred `Couldn't load Best stories` (fg), the reason in dim, `r retry`; the footer omits the error while the empty state shows it, and keeps it when stale stories are on screen.
4. **P3, Saved tab repeats ★ and offers `n/p page`.** Saved is one page (`next_page` returns early) and every row is saved. Correction: hide both in Saved.
5. **P3, reader meta row drops author and age.** The feed shows `host · author · points · comments · age`; the reader shows `host · points · comments` (finding 6 proposed the full form). Correction: use the feed's form.

All five were fixed in the commit after `0c1feb5` and verified with the same suites, fresh frame samples (the backdrop's mid-fade orange is now `#bf966c`, not yellow-green) and recaptures. The comment-row floor gives way at half the body, so tiny terminals still show the summary.

Open question, not a defect: the summary panel's first non-empty frame was measured 71–85 ms after `S`, with the first redraw at 28–38 ms. Harness read lag accounts for most of it, and intermediate frames were coalesced, so no defect is established.

Not sampled live: the notice fade and clear, because the only producer is `o`, which opens a real browser. Unit tests cover the 2 s, 2.4 s and 4 s points, including the motion-off clear.

Withheld: 16-colour selection is a heavy grey block whose shade the terminal decides; long summary section labels clip at narrow widths (metadata only).
