# hn, as a native window

Experience spec for a desktop `hn` built on [Rune](https://github.com/anomalyco/rune) at rev
`d751f07106eac47bdd4e33756dee71e1f593c9e2` (branch `dev`). Companion artifacts:
`mockups/desktop/index.html` (the screens, side by side) and `prototypes/rune-mocks/`
(seven of those screens rendered by Rune itself, not by CSS).

The premise: the terminal app keeps its keyboard. The window adds what an 80×24 grid cannot —
two panes side by side, an article you can actually read, comment threads with real indentation,
a summary that streams in place, and a feed that uses the width it has.

## Window anatomy

```
┌ titlebar (native, inset via cx.titlebar_inset()) ────────────────┐
│ toolbar: feed tabs only — Top · New · Best · Saved               │
├───────────────────────────────┬──────────────────────────────────┤
│ article pane (live page)      │ inspector: summary + discussion  │
│ WKWebView child, Rune header  │ 9pt drag grip between panes      │
└───────────────────────────────┴──────────────────────────────────┘
│ footer: contextual key hints (Keybind) · page stepper ‹ 1 ›      │
└──────────────────────────────────────────────────────────────────┘
```

The feed is the same window with the inspector collapsed (`PaneState::range(0.0, 0.0)`);
opening a story widens it. Wide windows lay the feed out in two columns, filled column-major:

```
┌ toolbar: Top · New · Best · Saved ───────────────────────────────┐
│ 1 ● Tesla to wind down…      ┊ 16  A barbarian tribe's mythic…   │
│ 2   Libghostty 1.0           ┊ 17  Chomsky and the two cultures… │
│ 3   A Jev that can ask Opus… ┊ 18  Can GROUP BY still be…        │
│ footer: j k move · Enter open · s save · n p page    ‹ 1 ›       │
└──────────────────────────────────────────────────────────────────┘
```

One window, one `SplitView`, no companion process — the Ghostty pane becomes a real pane.

## The article pane is the live page

The pane on the left is the story's **real page**, rendered by the system web engine in a child
view pinned to the pane's rect — not a reflow of extracted text. The reader gets the page the
author published: its own typography, images, code blocks, tables, and anything interactive.

```
article pane
├── Rune header   eyebrow · title · byline · "open original ↗"   (always Rune's)
└── live region   the story's page, in a WKWebView child view
```

The header stays Rune's for a mechanical reason: the child view is a subview of the window's
content view, so it draws above everything Rune paints. Anything the reader must still be able
to press — the title, the byline, `open original ↗` — has to sit outside the child's rect. The
child is pinned to the region below the header and follows it through `on_layout`, so a window
resize, a grip drag or `⌘\` moves the page with the pane.

**The readability capture still runs, and is still the point.** Opening a story fetches and
extracts the article exactly as before. That capture is what the summary card and its citations
read — the model never sees the page, it sees the extracted text, and a citation's excerpt is a
span of that text. The capture is a backend, not a surface: it is never drawn while the live
page is up.

**Fallback.** When no child view can exist, the pane is the extracted Markdown at the 66ch
measure, under a banner that says so:

```
live page unavailable — showing extracted text
```

That is the state in three cases, and the banner never appears otherwise:

| Case | Why |
| --- | --- |
| Headless (`--snap`, `--tree`, `--script`) | A `WKWebView` needs an `NSWindow`; a headless run renders offscreen. The scene trees are therefore deterministic: they always take this branch. |
| Not macOS | `wry` is a macOS-only dependency here; there is no child host to build. |
| The child view failed to build | The banner carries the platform's own message beside it. |

Summaries and citations work in every one of those states, because they read the capture rather
than the pane. `O` and `open original ↗` are unchanged in all of them: they hand the URL to the
system browser through `hn_lib::url_open`, which is a different action from what the pane shows.

Two consequences worth stating, because they are the cost of a native child view:

- **The page owns its own keyboard while it has focus.** Clicking into the page gives the
  `WKWebView` first responder, and Rune's `on_key` no longer sees those keys. Clicking any Rune
  chrome — the toolbar, the header, the inspector — takes focus back.
- **The child is hidden while a dialog is open.** The settings dialog and the help sheet are
  Rune's, and a webview over them would bury the controls. Hiding is `setHidden`, not a teardown,
  so the page keeps its scroll position and session.

## Screens

| | Screen | What it is |
| --- | --- | --- |
| 01 | Story split (hero) | The story's live page on the left, discussion right, summary pinned at the top of the inspector. The page is a `WKWebView` child view under a Rune header (eyebrow, title, byline, `open original ↗`); the extracted Markdown at a 66ch measure is the fallback, under a banner. Drag the grip to resize; double-click resets to 54/46; below the minimum the inspector collapses. `Esc` returns to the feed, `⌘\` collapses the split. See *The article pane is the live page*. |
| 02 | Feed, one column | `SegmentedControl` for Top/New/Best/Saved — the toolbar holds nothing else. Rows carry rank, title, points/author/comments, a saved dot beside the rank, and a hover-revealed save action. Moving over a row selects it; clicking opens it. Selection is a card fill plus an amber edge, not reverse video. |
| 03 | Feed, two columns | The same list at ≥1200pt: two columns, **column-major** — the left column fills first, then the right. One shared scroll, a hairline divider, and enough pages fetched to fill the visible grid. See *Responsive feed*. |
| 04 | Summary states | *Generating* (skeleton rows that grow and fade in as tokens arrive, with "2 of 3 sources captured"), *done* (numbered citations, `Tab` to select, `Enter` to read), *failed* (provider error + Retry, the old summary still readable underneath), *citation source* (the excerpt sent to the model, the extra captured text, the article dimmed to 0.55). |
| 05 | Comments | The discussion with the summary folded to one line. A guide rail per depth level, a hover target that folds the parent thread, hover-revealed save / links / collapse. `L` opens the link chooser as a popover. See *Comments*. |
| 06 | Comments overview | The trail at the top of the viewport, a folded thread stating "3 replies hidden · Enter expands", and a chain past the depth cap ending in "continue thread → 6 replies". |
| 07 | Focused thread | A deep reply under its ancestor trail, focus mode on: the chain at full contrast, sibling subtrees dimmed to 0.35. The trail is sticky, so "where am I" never scrolls away. |
| 08 | Collapsed subtree | A folded chain in place: header, author, and the count of what is behind it. Nothing moves except the rows below, which close the gap. |
| 09 | Saved | The feed's rows sorted by save time, `unsave` on hover, the `~/.local/share/hn/saved.json` path in the toolbar. Rows carry **no** saved dot — everything here is saved, so the dot would be noise. A corrupt file shows a banner with `Reload` and `Reveal in Finder` instead of silently emptying the list. |
| 10 | Settings | Provider radio group (OpenRouter / Anthropic), masked API key, automatic summaries switch, Fast/Thorough profile with the model each resolves to, `Refresh sources & summary`. `Apply` writes `~/.config/hn/summary.json`; `Cancel` and `Esc` discard. The TUI's `M` overlay, given room to breathe. |
| 11 | Help | The `?` sheet: two columns of `Keybind` rows, grouped Feed / Discussion / Summary / Window. Focus is trapped while open; `Esc` or `?` closes. |

## Responsive feed: one column, two columns

The terminal is 80 columns wide and always will be. A window is not: a feed that stays a single
900pt-wide column on a 1600pt display wastes half the screen and shows fewer stories than the
window could hold.

| | One column | Two columns |
| --- | --- | --- |
| Window width | < 1200pt | ≥ 1200pt |
| Order | `i` | `c * rows + r` — column-major: the left column fills first |
| Scroll | one list | one list, shared by both columns |
| Page size | 30 | 30 (unchanged — the page is a page, not a screenful) |
| Divider | none | 1pt hairline, `tok::separator`, full column height |

**Column-major, not row-major.** Story `i` sits at column `i / rowsPerColumn`, row
`i % rowsPerColumn`, so ranks 1–15 run down the left column and 16–30 down the right. `j` moves
straight down the column it is in; it never alternates columns on every press. Row-major
(`r * 2 + c`) was the first pass and it reads as two interleaved lists: the eye tracks one column
while the cursor zigzags across both. Column-major keeps the cursor and the eye on the same path,
and it makes the rank numbers monotonic down each column — which is how you scan a ranked list.

`scroll_into_view` resolves the selected index to its `(row, col)` cell and scrolls the shared
list to that row. `j` past the last story turns the page, as in the TUI. Crossing the threshold
reflows on `REFLOW` 0.40/0 and the selection keeps its story, not its row; below it the layout is
exactly the one-column feed — no intermediate three-column state, no card grid.

### Fill the grid

A two-column feed that renders one page of 30 into 14 visible rows leaves the right column half
empty at the bottom: the last row has a story on the left and nothing on the right. So fetch
enough pages to cover the visible capacity.

```
visibleCells = visibleRows × columns
pagesNeeded  = ceil(visibleCells / pageSize)
```

| Window | List viewport | Rows | Columns | Visible cells | Pages of 30 |
| --- | --- | --- | --- | --- | --- |
| 1180×760 | 656pt | 11 | 1 | 11 | 1 |
| 1600×900 | 796pt | 14 | 2 | 28 | 1 |
| 1600×1200 | 1096pt | 19 | 2 | 38 | 2 |

At 1600×900 one page of 30 already covers 28 cells, so the grid is full and the 15th row sits
just below the fold. At 1600×1200 the same page leaves 8 cells empty, so the app fetches a second
page and renders 38 of the 60 it holds. One column is the same rule: `ceil(11 / 30) = 1`.

`pages_needed()` in `prototypes/rune-mocks/src/main.rs` is that formula, and the wide mock renders
30 stories into 15 rows to prove the grid is full. The real app measures the viewport on resize
and refetches only when `pagesNeeded` grows; shrinking the window never drops a page it already
has, so scrolling back up does not re-fetch.

Measurement: `Cx::layout_with` / `Cx::layout_size` on the keyed window root
(`crates/rune/src/runtime/layout_read.rs:216,232`) — synchronous and settled on the first layout,
so there is no one-frame lag and no wrong-column first frame. (An earlier draft claimed `Cx` has no
viewport accessor; that was wrong — the accessors live in `runtime/layout_read.rs`, not `cx.rs`.)
WS-B's first script proof must confirm the root reports WINDOW width, not a chrome-inset content
rect (`titlebar_inset` at `runtime/cx.rs:128`); the committed goldens pin the expected outcome:
1180×760 → one column, 1600×900 → two columns.

## Comments: reading a thread you did not start

Hacker News threads are deep. A front-page story routinely runs ten levels, and the terminal's
two-spaces-per-level indent runs out of grid long before the conversation runs out of replies. The
window has the same problem in a different unit: the inspector is 535pt wide, 491pt of content,
and every indent level costs 13pt. At depth 8 the measure is down to ~390pt — under Butterick's
45-character floor and well under WCAG's 80.

So the discussion view is built around one question: **where am I, and what is the rest of this
thread?** Four mechanisms answer it, and they are the whole design.

**1. The ancestor trail (sticky).** Above the focused comment, a one-line strip:
`tptacek › kstrauser › jmillikin › simonw › dang`. The last segment is the comment you are
reading; the rest are its ancestors, each a click target and a `⌫` step. It is `sticky_top(0.0)`
inside the scroll viewport, so it holds the top edge while the thread moves under it — the answer
to "where am I" never scrolls away. Reddit's threadline reached the same conclusion for the same
reason: the collapse target has to stay in the viewport, or you scroll back up to find your place.

**2. Depth cap + "continue thread →".** Indent guides stop at four levels (`MAX_DEPTH`). A comment
at the cap that still has replies shows `continue thread → 6 replies` instead of its children;
`Enter` re-roots the tree there and pushes a segment onto the trail. Depth beyond four is carried
by the trail, not by whitespace. This is Reddit's "continue this thread", which exists for exactly
this reason: past a handful of levels, indentation has eaten the text column.

**3. Focus mode (`f`).** Every row that is neither an ancestor of the cursor nor in its subtree
dims to 0.35. The chain you are reading stays at full contrast; the sibling subtrees between you
and the next thing you care about recede without disappearing. Nothing is hidden, so nothing has
to be found again.

**4. Collapsed-thread counts.** A folded comment keeps its header and states what it is hiding:
`3 replies hidden · Enter expands`. The count is derived from the tree, not stored, so it cannot
drift from the rows it describes.
### What was rejected

| Pattern | Why not |
| --- | --- |
| A third pane, or a sub-pane inside the inspector, for the focused thread | The inspector is already the second pane. A third column inside it leaves ~330pt for comment text — below the 45-character floor, and the indent guides would eat what is left. Slack's flexpane works because a channel is a flat stream; a tree needs the width for its own structure. |
| A top-level outline / mini-map of the discussion | HN comments have no titles, only authors and a first line. An outline of `tptacek, kstrauser, dang…` is not scannable, and it is a second rendering of the same rows — which breaks the one-state-one-place rule below. |
| Focus mode alone, without the trail | Dimming tells you what to ignore; it does not tell you where you are. The trail is the part that answers the question. |
| The trail alone, without the cap | At depth 8 the measure is already under the readable floor. The trail tells you where you are in a column you can no longer read. |
| Auto-collapsing everything below depth 3 on open | It hides the conversation the reader came for, and the reader has to expand it back. The cap is per comment (`continue thread`), not a global default. |
| A separate window for thread focus | One window with panes keeps the article, the summary and the thread in one place. A second window loses the article you were reading against. |

### Keyboard

| Key | Behaviour |
| --- | --- |
| `j` / `k` | next / previous visible comment |
| `[` / `]` | previous / next sibling |
| `h` / `l` | fold / unfold the current comment (`Tree`'s Left/Right) |
| `Enter` | expand a folded thread; on `continue thread →`, re-root there |
| `⌫` | pop one segment off the trail (re-root to the parent) |
| `f` | toggle focus mode |
| `u` | jump to the parent comment |
| `L` | link chooser for the current comment |
| `Tab` / `S` | citation / summary, unchanged |
| `Esc` | leave focus mode, then back to the feed |

Every key in `README.md:111-148` keeps its meaning; `f`, `⌫` and `Enter`-on-continue are additions.

### Motion

Re-rooting (continue thread, `⌫`) is `GLIDE` 0.32/0 on the scroll offset with the rows crossfading
on `REDUCED_FADE`; a trail segment added or removed is `SNAP` 0.25/0 on the strip's width; focus
mode is `REFLOW` 0.40/0 on the dimmed subtrees' opacity, and the chain does not move; fold is
`REFLOW` 0.40/0 on height with the chevron turning on `SNAP`. The sticky trail has no motion of
its own — it follows the scroll offset. Under reduced motion, re-rooting lands at once and the
rows fade in place.

### Sources

- Reddit, "continue this thread" — appears when a tree is too deep to keep indenting on one page:
  <https://www.reddit.com/r/help/comments/2dob6n/exactly_what_is_continue_this_thread_at_the/>
- Reddit's threadline, and why the collapse target must stay in the viewport:
  <https://idevie.com/design/ux/reddit-collapse-comment-thread-redesign>
- Slack's thread design history — deep nesting tested as incomprehensible, so threads were cut to
  one level and moved to a side panel:
  <https://slack.design/articles/threads-in-slack-a-long-design-journey-part-2-of-2/>
- GitHub review threads — resolving collapses the thread so unresolved ones stand out, behind a
  "show resolved" toggle: <https://pyor.review/blog/resolve-conversation-etiquette>
- Butterick, *Practical Typography* — 45–90 characters, 2–3 alphabets:
  <https://practicaltypography.com/line-length.html>
- WCAG 1.4.8 — lines should not exceed 80 characters:
  <https://w3c.github.io/wcag/understanding/visual-presentation.html>

## Every state appears in exactly one place

**A state is rendered in exactly one place, and every other surface either omits it or points at
it.** Three things were in the first pass and are gone because of it.

| Removed | Why |
| --- | --- |
| Top-right "live" badge | It duplicated the refresh affordance and implied a push feed. `hn` polls the API on demand; there is nothing live to report. |
| "page 1 of 3" | The HN API returns no total, so the denominator was invented. False precision is worse than no precision. The stepper reads `‹ 1 ›` — current page only. |
| `fast` in the summary card header | The toolbar pill already said `summary · fast`. The same fact, twice, 200pt apart. |

The summary profile is the case that broke it. The toolbar pill is the one that stays: it is the
"a summary exists, and which one" indicator, and it is visible whether or not the card is on
screen. The card header now reads `Summary` plus `captured 2m ago · 3 of 3 sources` — its identity
and its provenance, which the pill does not carry. `verification/story.json` has exactly one
element whose text contains `fast`.

The same test applied elsewhere: the saved dot is the only persistent mark of a saved story (the
Saved view drops it, because there every row is saved); the footer carries each key hint once.

## The saved indicator

Saving is a confirmation, not a state you keep reading. A 6pt amber dot sits right of the rank,
vertically centred on the title line — the only persistent mark a saved story carries. Toggling it
plays `POP` 0.25/0.15 (0.6 → 1.0, small overshoot) and nothing else on the row moves. A `Toast`
bottom-right says "Saved · Press 4 for Saved" (or "Removed from Saved"), carrying its own copy of
the dot as its lead, and leaves after 2.4 s. The right edge stays empty: no label, no badge, no
timestamp — that edge belongs to hover actions. The Saved view has no dots at all, because every
row there is saved; hover reveals `unsave` instead.

## Interaction model

Keyboard parity is a hard requirement — every key in `README.md:111-148` keeps its meaning. The
window adds a pointer layer on top; nothing is mouse-only.

| Input | Behaviour |
| --- | --- |
| move over a feed row / comment | selects the feed row, or the comment when focus mode is on; feed Save appears on hover |
| click feed row | opens the story; Save remains a separate action |
| click chevron / indent guide | folds that thread / the whole parent thread |
| click a trail segment | re-roots the thread at that ancestor |
| click citation chip | opens the source viewer |
| drag grip | resizes the split (clamped by `PaneState::range`); double-click resets to 54/46 |
| wheel | scrolls the pane under the pointer, independently |
| right-click | context menu (save, open, copy link, collapse) |
| click `‹` / `›` | turns the page (same as `n` / `p`) |
| `⌘\` / `⌘,` / `⌘?` | toggle split / settings / help |

Focus is explicit: `focusable` + `focus_group` per pane, `focus_highlight` in amber, so `Tab`
never wanders into the article body. `scroll_into_view` keeps the selected row on screen when
`j`/`k` cross a page boundary, and resolves the row to its column when the feed is two-column.

The desktop now builds against Rune `55ef5268`; the original API source locations in this design document refer to the earlier pinned design snapshot.

## Motion spec

Rune's springs are duration + bounce, retargeted with exact position and velocity
(`crates/rune-motion/src/tokens.rs:12-31`). Nothing here is a CSS easing curve.

| Token | Spring | Used for | Reduced motion |
| --- | --- | --- | --- |
| `INSTANT` | 0.10 s / 0 | hover overlays, pointer feedback | unchanged (no travel) |
| `SNAP` | 0.25 s / 0 | tab indicator, citation chip, switch knob, chevron, trail segment | snaps |
| `SETTLE` | 0.30 s / 0 | failure card, occasional layout changes | snaps |
| `GLIDE` | 0.32 s / 0 | source viewer; thread re-root | fades in place |
| `REFLOW` | 0.40 s / 0 | thread fold; focus mode; row removal; one-column ↔ two-column reflow | lands at once |
| `MORPH` | 0.30 s / 0.16 | split opening from 0 width; settings, help, link chooser | fades |
| `POP` | 0.25 s / 0.15 | the saved dot beside the rank; the toast follows it | fades |
| `REDUCED_FADE` | 0.15 s curve | replacement for every spatial move | — |

Four of these carry the reading loop. **Open a story → `MORPH`**: the inspector's width springs
from 0 to its remembered fraction and the article's measure reflows under it — the feed does not
slide aside, the split grows into it. **Save → `POP` + toast**: the dot overshoots once, the toast
fades in behind it, nothing else on the row moves. **Fold a thread → `REFLOW`**: replies collapse,
the comments below close the gap, and the folded header stays put so the click target does not
move under the pointer. **Turn a page → directional arrival**: the incoming feed group moves 24 pt from the navigation direction while opacity rises from 0.2 to 1 over about 220 ms, without staggering. The old group disappears immediately. Routine row selection has a restrained 120 ms color response and reserves its accent marker slot. Reduced motion is instant. Left/Right move between feed columns at the same visual row (clamped on a short right column); at column edges arrows turn pages, landing at the first/last story. Explicit paging remains on n/p, Space, Page Down/Page Up and the buttons. Both columns move together as one group.

An empty pending feed shows static placeholder rows at the real grid's spacing, with one busy status and a footer loading label. Delayed startup results enter 12 pt upward with the same 220 ms fade; already-ready first-paint content appears immediately. Tab results enter horizontally according to tab order, including immediate Saved results. Placeholder rows have no controls or fake story text. Refresh and paging retain existing rows; failures and settled-empty states have their own messages. Reduced motion lands every arrival immediately.

Pane resize uses `PaneMotion::spring(SETTLE).scale_with_width(480, .15, .3)` so a narrow window
does not feel twitchy; the pane tail fades over the last 15% (`fade_tail`). Streaming summary rows
use `growth` + `streaming(true)` with `GLIDE`; the pinned card never moves while tokens arrive, so
the discussion below it does not jump. Reduced motion follows the OS through `cx.is_reduced_motion()`
(`crates/rune/src/runtime/cx.rs:223`), and `HN_REDUCED_MOTION=1` keeps working as an override —
the same variable the TUI honours. Nothing animates on the first frame, so a cold launch is a
still image, not an entrance.

## What deliberately differs from the terminal

| Desktop | Terminal constraint it removes |
| --- | --- |
| A real split, not a Ghostty companion | macOS + Ghostty only, one tab owned by `hn` |
| The story's real page in the pane | extracted text reflowed into the grid |
| Comment depth as a trail + fold target | two spaces per level, no fold target |
| Hover as a third input | every action needed a key or a mode |
| Summaries that arrive (streaming, retry card) | a spinner line, then a block of text |
| Two-column feed, resizable remembered layout | fixed 80-column composition |

Deliberately *not* changed: the feed is still a list, not a card grid; there is no infinite
scroll; `j`/`k` still move one story at a time; summaries still cite comments by number.

## Rune API mapping

| Design element | Rune API | File |
| --- | --- | --- |
| Two-pane window, drag grip | `SplitView`, `.sidebar()`, `.inspector()` | `crates/rune-kit/src/shell/split.rs:134,170,175` |
| Pane width, clamp, resize spring, tail fade | `PaneState::new`, `.range`, `PaneMotion::spring`, `.scale_with_width`, `.fade_tail` | `crates/rune-kit/src/shell/pane.rs:46,53,190,197,204` |
| Split motion + reduced motion | `.pane_motion()`, `.reduced_motion()` | `crates/rune-kit/src/shell/split.rs:221,235` |
| Collapsed inspector peek, hover previews | `SidebarPeek`, `HoverCard`, `TooltipStyle::neutral` | `crates/rune-kit/src/shell/pane.rs:282`, `overlay.rs:520,338` |
| Feed tabs | `SegmentedControl` | `crates/rune-kit/src/segmented.rs:20` |
| Toolbar, page stepper | `Toolbar`, `Page` | `crates/rune-kit/src/shell/toolbar.rs:16`, `shell/split.rs:383` |
| **Two-column feed** | `equal_columns(2)`, `grid_cell(c)` | `crates/rune/src/el/layout.rs:246,252` |
| **Column hairline, saved dot** | `el().w(1.0).fill(..)`, `el().size(6,6).radius(3).fill(accent)` | `crates/rune/src/el/layout.rs:15`, `el/visuals.rs:7,11` |
| **Save toast** | `Toast`, `ToastRegion`, `root(base, overlays)` | `crates/rune-kit/src/toast.rs:7,114,126`, `lib.rs:257` |
| **Comment tree, fold, depth** | `Tree`, `TreeItem`, `TreeModel`, `::depth`, `::parent`, `::subtree`, `::expand_to`, `::reveal` | `crates/rune-kit/src/tree/view.rs:312,13`, `tree.rs:86,215,199,219,310,425` |
| **Fold chevron, guide rail** | `TreeStyle::chevron_only_folders`, `::guides_on_hover` | `crates/rune-kit/src/tree/view.rs:280,302` |
| **"3 replies hidden", "continue thread →"** | `TreeItem::badge`, `TreeItem::trailing` | `crates/rune-kit/src/tree/view.rs:75,80` |
| **Ancestor trail (sticky)** | `sticky_top`, `stuck_shadow` | `crates/rune/src/el/sticky.rs:42,84` |
| **Focus mode dim** | `dim` | `crates/rune/src/el/visuals.rs:196` |
| **Disclosure row** | `Collapsible` | `crates/rune-kit/src/collapsible.rs:8` |
| **Keep the cursor on screen** | `scroll_into_view` | `crates/rune/src/el/tx/scroll.rs:18` |
| **Long comment lists** | `virtualize(est_h)` + `growth(GrowthSpec)` | `crates/rune/src/el/virtual_rows.rs:31`, `el/growth.rs:9` |
| **Settled subtrees** | `cache()`, `cache_key(u64)` | `crates/rune/src/el/visuals.rs:219,225` |
| **Article images** | `Markdown::images`, `.image_frame`, `ImageFrame` | `crates/rune-kit/src/markdown/image.rs:45,53,16` |
| **Links, text selection** | `link(Link)`, `.on_link`, `.on_link_secondary`, `.href()`, `selectable(bool)` | `crates/rune/src/el/text.rs:329,341,351,384`, `el/a11y.rs:87` |
| **Motion trace for CI** | `--trace out.jsonl`, `--trace-all` | `crates/rune/src/tooling/args.rs:175,177,278,280` |
| Footer key hints | `Keybind` | `crates/rune-kit/src/keybind.rs:15` |
| Right-click menu, popover | `Menu`, `context_menu`, `absolute(left, top)` | `crates/rune-kit/src/menu.rs`, `crates/rune/src/el/layout.rs:257` |
| Article body, citations (the fallback pane) | `Markdown`, `.selectable()`, `.highlights()` | `crates/rune-kit/src/markdown.rs:175,314,229` |
| **Live article pane** | `WindowHandle::with_raw_handle`, `El::on_layout`, wry `build_as_child`, `set_bounds`, `set_visible`, `load_url` | `crates/rune/src/window/handle.rs:133`, `el/layout_watch.rs:33`, `wry-0.57.1/src/lib.rs:1571,2264,2269,2101` |
| Generating + streaming rows | `Spinner`, `Loader`, `Progress`, `growth`, `streaming` | `crates/rune-kit/src/feedback.rs:11,71,111`, `crates/rune/src/el/growth.rs:9,15` |
| Settings / help sheets | `Dialog` | `crates/rune-kit/src/dialog.rs:5` |
| Theme, amber accent | `tok::theme(dark)`, `tok::accent(i,dark)` | `crates/rune/src/tok.rs:166,210` |
| Hover / press / dim, pointer events | `hover`, `press`, `press_scale`, `dim`, `on_hover`, `on_hover_change`, `on_pointer_move` | `crates/rune/src/el/visuals.rs:236,241,249,196`, `el/layout.rs:346,358,310` |
| Text measure, truncation | `wrap`, `truncate` | `crates/rune/src/el/text.rs:214,241` |
| Scroll fade, keep-on-screen, sticky | `scroll_edge_fade`, `scroll_into_view`, `keep_on_screen`, `sticky_top`, `stuck_shadow` | `crates/rune/src/el/scroll.rs:83`, `el/tx/scroll.rs:18,121`, `el/sticky.rs:42,84` |
| Help sheet, modal, a11y, focus | `help`, `modal`, `role`, `label`, `focusable`, `focus_group`, `focus_highlight` | `crates/rune/src/el.rs:1224,1152,1188,1192,1030,1046,1077` |
| Reduced motion, titlebar inset | `is_reduced_motion`, `titlebar_inset` | `crates/rune/src/runtime/cx.rs:223,128` |
| Headless verification | `--snap`, `--tree`, `--scene`, `--size`, `--scale`, `--theme`, `--html` | `crates/rune/src/tooling/args.rs` |

Bold rows are the ones this iteration added. `Badge` is no longer used anywhere: the saved dot is
a plain `el()` circle, because a badge is a label and the dot is not.

## Implementation sequencing

Reuse `hn_lib` unchanged; the desktop app is a second front end over the same core.

| Step | Days | Work |
| --- | --- | --- |
| Shell + feed | 2 | `SplitView` with the inspector collapsed, `SegmentedControl` tabs, rows from `hn_client.rs:15` `HackerNewsItem`. Paging and `j`/`k`/`g`/`G` port from `src/app.rs`; the saved dot reads `bookmarks.rs:21` `load`. |
| Two-column feed | 1 | `equal_columns(2)` above 1200pt, `c * rows + r` index mapping, `pages_needed()` on resize, `scroll_into_view` resolving row → column. Pure layout; no new state. |
| Story split | 3 | The story's live page in a `WKWebView` child view pinned to the pane's rect (`WindowHandle::with_raw_handle` + `El::on_layout`), with the extracted Markdown as the fallback; comment tree into `Tree`; drag grip + `PaneState` persistence. |
| Comments | 3 | `TreeModel` over the comment tree, the sticky trail, `MAX_DEPTH` + continue-thread re-root, focus mode. The cap and the trail are the two that need real use to tune. |
| Summary panel | 3 | The job lifecycle in `src/reader/jobs.rs` and prompts in `src/reader/summary_prompts.rs` are already provider-agnostic; swap the TUI's repaint for `growth`/`streaming` rows. Citation chips map to `src/reader/sources.rs`. |
| Settings + help | 1 | `src/reader/config.rs:94` already owns `summary.json`; the form is a `Dialog` over the same `Config`. |
| Polish | 2 | Hover layer, context menus, toasts, reduced-motion pass, window layout persistence. |

Roughly **15 working days** for one engineer familiar with the codebase, plus a day of packaging
per platform. The riskiest item is not UI: Rune is pre-release, so the pinned rev will need
periodic rebases.

## Now and future

**Now** — in this pass, and proven in `prototypes/rune-mocks/`: two-column feed at ≥1200pt,
column-major, one scroll, filled to the visible grid (`el/layout.rs:246`); saved dot + transient
toast with nothing on the right edge (`toast.rs:7,114`); minimal `‹ N ›` stepper with no total
(`shell/split.rs:383`); the discussion trail, depth cap and focus mode (`el/sticky.rs:42`,
`tree.rs:215,310`, `el/visuals.rs:196`); virtualized comment lists (`virtual_rows.rs:31` +
`growth.rs:9`); `cache()` on settled subtrees so a streaming summary does not re-lay-out the
thread below it (`el/visuals.rs:219`).

**Future** — real at the pinned rev, not in this pass:

- Article images and a TOC rail (`markdown/image.rs:45,53`; `ImageFrame` `:16`).
- Real link handling: `link(Link)` + `.on_link` / `.on_link_secondary` for primary and secondary
  click, `.href()` for the a11y tree (`el/text.rs:329,341,351`, `el/a11y.rs:87`).
- Text selection across panes (`selectable(bool)`, `el/text.rs:384`); find-in-page over the
  article and the comment tree.
- Motion-trace JSONL in CI: `--trace out.jsonl` / `--trace-all` emit per-frame spring state, so a
  regression in `POP` or `GLIDE` fails a test instead of needing an eyeball
  (`tooling/args.rs:175,177,278,280`).
- A `⌘K` command palette; per-story reading position; a "compare Fast vs Thorough" side-by-side
  summary; native notifications when a long summary finishes in the background; a per-thread
  "unread since last visit" marker, which needs a store the TUI does not have yet.

## Testing and verification strategy

Rune ships an agent-grade headless harness (`crates/rune/src/tooling/args.rs`), so the desktop app
is testable without a window, a GPU session, or human eyes. Layers, cheapest first:

1. **State-machine unit tests (no GPU).** The desktop frontend is thin over `hn_lib`; the feed
   order, fill-the-grid formula, trail stack and focus-mode set are pure functions of state. Test
   them in Rust like the repo's existing `src/*_tests.rs` pattern. Network is never hit: reuse
   `src/reader/snapshot.rs` and the fixture JSONs (`src/reader/replay.rs` is a test-only binary
   entrypoint and is NOT reusable by the desktop crate).
2. **Scripted interaction, asserted on the tree.** `--script in.json` replays timestamped input
   headless — keys, chords, `move/down/up/tap/drag`, `menu-pick`, `file-dialog`, and `mark` points;
   `--at mark:NAME` snapshots mid-flight. Assert on `--tree out.json` (roles, keys, labels, bounds,
   opacity — the accessibility tree is the stable surface) instead of pixels wherever possible.
   Demonstrated: `prototypes/rune-hn` driven with 3×`j` produced 46 animating frames (the selection
   spring) vs 0 at rest; saved under `prototypes/rune-hn/verification/drive-3j{,-trace.jsonl,-after.json,-after.png}`
   (script = 3×`j`; the older `select-last.json` drives 11×`j` to row 12).
3. **Golden renders.** Per scene, `--snap out.png --tree out.json` at a matrix of
   `--size/--scale/--theme` (narrow + wide, 1×/2×, dark/light), checked into `verification/` and
   diffed in CI: tree diff exactly, PNG diff with a small tolerance. `--list-scenes` keeps the
   scene inventory explicit.
4. **Motion lint.** `--trace out.jsonl` / `--trace-all` emit per-frame rect/opacity/track state
   ("motion-lint JSONL"); Rune flags tracks that were animated on purpose. CI asserts only intended
   tracks move, and that a reduced-motion run (`HN_REDUCED_MOTION=1`) has zero spatial animation.
5. **Perf budgets from the run itself.** Headless runs print frames/fps/CPU/layout-nodes/glyphs and
   at-rest frame counts; assert "at-rest draws nothing" and CPU p95 under a budget.
6. **CI topology.** macOS runner for Metal golden PNGs; Linux runner with Mesa software Vulkan
   (`VK_ICD_FILENAMES=...lvp...`) for `--tree` assertions, which need no real GPU; Windows smoke
   build. Manual pass on a real macOS window for pointer feel (drag grip, hover) — headless cannot
   verify feel.

## Regenerating the Rune renders

```sh
cd prototypes/rune-mocks
cargo run -- --scene feed       --snap verification/feed.png       --tree verification/feed.json       --size 1180x760 --scale 2 --theme dark
cargo run -- --scene feed-wide  --snap verification/feed-wide.png  --tree verification/feed-wide.json  --size 1600x900 --scale 2 --theme dark
cargo run -- --scene feed-saved --snap verification/feed-saved.png --tree verification/feed-saved.json --size 1180x760 --scale 2 --theme dark
cargo run -- --scene story      --snap verification/story.png      --tree verification/story.json      --size 1180x760 --scale 2 --theme dark
cargo run -- --scene citation   --snap verification/citation.png   --tree verification/citation.json   --size 1180x760 --scale 2 --theme dark
cargo run -- --scene comments   --snap verification/comments.png   --tree verification/comments.json   --size 1180x760 --scale 2 --theme dark
cargo run -- --scene thread     --snap verification/thread.png     --tree verification/thread.json     --size 1180x760 --scale 2 --theme dark
```

Scenes: `feed` (one column), `feed-wide` (two columns at 1600×900), `feed-saved` (the toast
state), `story`, `citation`, `comments` (the discussion overview), `thread` (a deep reply under
its trail); `--list-scenes` enumerates them. The `--tree` JSON is the assertion surface:

- `feed-wide.json` — ranks 1–15 share one left-x, 16–30 another, and consecutive ranks step y by
  exactly one row height: column-major, `j` goes straight down.
- `story.json` — exactly one element whose text contains `fast`: the toolbar pill.
- `comments.json` / `thread.json` — `comment-*` rows carry `author · depth N` labels, plus
  `more-*` (continue thread) and `fold-*` (replies hidden) buttons.
