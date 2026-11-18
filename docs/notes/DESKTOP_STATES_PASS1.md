Seven candidates have a nonempty kill list. The strongest is the citation state, where one displayed summary, its captured sources, selection, and an open source viewer currently rely on several separate fields staying aligned.

Activity is the number of **distinct commits touching the affected current paths** in the last 90 days, including directly affected tests. It uses `git log --since=90.days` without following earlier paths through the recent splits. Kill counts are source sites; F3 explicitly includes checked indexing expressions.

| ID | Impossible state | New shape | Kill sites | Activity | Leverage |
|---|---|---|---:|---:|---:|
| F1 | Focused citation or open source lacks its displayed summary/capture | Owned summary snapshot and validated citation selection | 10 | 14 | **140** |
| F2 | Open link chooser is empty or selects outside its items | Nonempty chooser with a selected item | 4 | 14 | **56** |
| F3 | Comment ID maps to a missing or different vector slot | One ID-keyed node store | 7 checked indexes | 5 | **35** |
| F4 | Native child marked shown without a child or bounds | Owned child with explicit visibility/layout state | 3 | 8 | **24** |
| F5 | Feed marked loading without request metadata | Request lifecycle owns loading and the last request | 2 | 7 | **14** |
| F6 | Runtime zoom is not one of seven supported steps | `ZoomStep` | 1 | 6 | **6** |
| F7 | Source-rendering context has no source navigation | Render context carries the source view | 2 | 1 | **2** |

### F1. Citation snapshot and selection

**State and boundary.** [Story](../../desktop/src/story.rs:143) stores the summary, captured sources, staged replacement, citation index, focus, and open source separately. A source can be opened only through a citation derived from a readable summary and its sources ([citation construction](../../desktop/src/story.rs:350), [opening](../../desktop/src/story.rs:411)). A replacement must wait while that source is open ([test](../../desktop/src/story.rs:732)).

Build a `DisplayedSummary` at `Story::land_summary`, pairing the summary with its capture and validated citations. Give each citation its captured source and first matching bullet. Represent an available selection with a nonempty cursor, and let an open source own the displayed snapshot and any staged replacement. Keep `current_sources` separate: refreshed comments can be newer than the capture behind the displayed summary ([test](../../desktop/src/app/tests/summary_lifecycle.rs:267)).

**Kill list, 10 sites.**

- The previous-summary presence check and unwrap at [story.rs:284](../../desktop/src/story.rs:284) and [story.rs:286](../../desktop/src/story.rs:286).
- Fallible selected-citation and captured-source lookups at [story.rs:391](../../desktop/src/story.rs:391), [story.rs:416](../../desktop/src/story.rs:416), and [story.rs:439](../../desktop/src/story.rs:439).
- The three corresponding fallbacks when revealing a focused citation at [input.rs:385](../../desktop/src/app/input.rs:385), [input.rs:386](../../desktop/src/app/input.rs:386), and [input.rs:392](../../desktop/src/app/input.rs:392).
- The second captured-source lookup at [summary.rs:140](../../desktop/src/story_view/summary.rs:140), and the optional readable-summary branch inside an open source viewer at [discussion.rs:476](../../desktop/src/story_view/discussion.rs:476).

No whole test is solely defensive. Keep the missing-citation boundary test and the replacement, reordering, and disappearance behavior tests ([story.rs:753](../../desktop/src/story.rs:753), [story.rs:829](../../desktop/src/story.rs:829)).

**Blast radius and risk.** `SummaryState`, `Story` and citation accessors change, as do the summary/discussion views and app input, reading, polling, scene, settings, and window consumers. Citation, discussion, keys, and summary/story lifecycle tests need adaptation. Nothing here is serialized. Preserve selection reconciliation and delayed replacement; the new type must not merge `current_sources` into the displayed capture.

A code-path edge exists for a **direct** successful update with no citations: with no source open, replacement clamps the index without clearing `citation_focus` ([story.rs:281](../../desktop/src/story.rs:281)). Desktop tests inject zero-citation replacements while a source is open ([test](../../desktop/src/app/tests/citation.rs:299)); the live provider rejects successful results without valid citations ([network.rs](../../src/reader/network.rs:117)). I did not establish that edge through the live provider.

### F2. Nonempty link chooser

**State and boundary.** `LinkChooser` permits an empty `items` vector or an out-of-range `selected`, although construction refuses an empty list ([story.rs](../../desktop/src/story.rs:107), [construction](../../desktop/src/story.rs:450)). Use a nonempty zipper such as `before/current/after`, preserving duplicate links and wrapping order. Parse the collected links when opening; validate the raw row index again when a pointer event is dispatched ([discussion.rs](../../desktop/src/story_view/discussion.rs:413)).

**Kill list, 4 sites.** The empty-list return in [story.rs:132](../../desktop/src/story.rs:132), fallible selection in [story.rs:127](../../desktop/src/story.rs:127), the selection-related `?` in [story.rs:504](../../desktop/src/story.rs:504), and the redundant optional URL branch after the chooser-presence check in [input.rs:230](../../desktop/src/app/input.rs:230). No whole test is killed; retain the empty-input and wrapping tests ([story.rs:855](../../desktop/src/story.rs:855)).

**Blast radius and risk.** `LinkChooser` fields and `selected_url` change in `story.rs`, the discussion view, and input routing. `take_link` still needs to represent a *closed* chooser. No persistence changes.

### F3. Comment ID-to-slot correlation

**State and boundary.** `Thread` stores both `nodes: Vec<Node>` and an `index` from ID to vector position ([comments.rs](../../desktop/src/comments.rs:102)). A stale in-range position could return the wrong node; an out-of-range position can panic. Build one `HashMap<Id, NodeData>` from captured comments, with the ID as the key rather than another mutable copy in the node. Preserve the separately ordered roots and reply IDs.

**Kill list, 7 checked indexing sites.** The indirect accesses at [comments.rs:155](../../desktop/src/comments.rs:155), [188](../../desktop/src/comments.rs:188), [260](../../desktop/src/comments.rs:260), [268](../../desktop/src/comments.rs:268), [269](../../desktop/src/comments.rs:269), [272](../../desktop/src/comments.rs:272), and [299](../../desktop/src/comments.rs:299). These are Rust checked subscripts, not seven observed machine-code branches. No whole test is killed.

**Blast radius and risk.** Primarily `comments.rs`, including its public `Node` shape and inline tests; no serialization. Missing-ID handling remains necessary. The rewrite must preserve root and reply order, unreadable-node pruning, and retry ordering; a new map lookup needed for borrowing should not be counted as a net guard reduction without checking the implementation.

### F4. Native child ownership

**State and boundary.** `Live`, `native_window`, `live_bounds`, `live_shown`, and the thread-local child can disagree ([app fields](../../desktop/src/app/mod.rs:187), [child storage](../../desktop/src/webview.rs:134)). A shown child requires both an actual child and bounds. Return an owned child from successful `webview::create`; represent its hidden or shown state with bounds attached to the shown case. Window and layout callbacks remain genuine boundaries.

**Kill list, 3 sites.** The bounds unwrap in [live_page.rs:84](../../desktop/src/app/live_page.rs:84), and the absent-child no-op checks in [webview.rs:199](../../desktop/src/webview.rs:199) and [webview.rs:218](../../desktop/src/webview.rs:218). Keep the absent-child handling for operations such as zoom that are currently invoked before a child exists ([input.rs](../../desktop/src/app/input.rs:41)). No whole test is killed.

**Blast radius and risk.** `webview` creation and methods, app live/window/input/reading/poll code, and native lifecycle, summary, window-layout, and support tests change. No persistence. Keep the child on its creation thread and retain a hidden page when returning to the same story. `Live::Ready` currently means **created**, while page readiness is reported separately ([live_page.rs](../../desktop/src/app/live_page.rs:66), [webview.rs](../../desktop/src/webview.rs:175)); the new names should make that distinction explicit.

### F5. Feed request lifecycle

**State and boundary.** `requested` and `FeedStatus.loading` are independent fields, although the production dispatch records request metadata before starting a request ([feed_actions.rs](../../desktop/src/app/feed_actions.rs:15)). Use a request lifecycle with before-first-request, settled-last-request, and loading-with-request cases. Project the loading bit into the shared `FeedState`; retain the last request after settlement because resize and Saved rendering use it.

**Kill list, 2 sites.** The missing-request `Ranking::Keep` fallback while loading at [poll.rs:150](../../desktop/src/app/poll.rs:150), and the second optional match at [poll.rs:159](../../desktop/src/app/poll.rs:159), whose preceding equality already implies a request exists. No whole test is killed.

**Blast radius and risk.** App request, poll, feed/window/input views, `FeedModel`’s shared-state projection, and feed arrival/paging, keys, Saved, layout, and support tests. No persistence. Test setup currently constructs loading without a request directly ([feed_arrivals.rs](../../desktop/src/app/tests/feed_arrivals.rs:409)); retarget that fixture. Keep stale-result rejection, timeout behavior, and independent error information.

### F6. Supported zoom

**State and boundary.** `App.zoom` is an `f32` even though runtime controls only use seven values ([layout.rs](../../desktop/src/layout.rs:18), [app field](../../desktop/src/app/mod.rs:163)). Parse the persisted float into a `ZoomStep` on layout restore; expose its float only at Rune, webview, and geometry boundaries. Keep `desktop.json`’s optional numeric field and its current nearest-step/default behavior.

**Kill list, 1 site.** The search and `expect` that the runtime float is a supported step at [input.rs:26](../../desktop/src/app/input.rs:26). The persistence clamp and step-end behavior remain valid boundary behavior ([layout.rs](../../desktop/src/layout.rs:40), [input.rs](../../desktop/src/app/input.rs:28)). No test is killed.

**Blast radius and risk.** Layout restore, app state, input, window and native geometry, plus zoom/layout, Saved, and support tests. Preserve the existing on-disk number and rounding behavior unless the separate width issue below is deliberately addressed.

### F7. Source row rendering context

**State and boundary.** `comment_row` takes a `context: bool` separately from the optional source navigation, then unwraps the latter ([discussion.rs](../../desktop/src/story_view/discussion.rs:177)). `source_viewer` receives captured content separately and unwraps the navigation again ([discussion.rs](../../desktop/src/story_view/discussion.rs:464)). Pass `Discussion` or `Source(&SourceView)` as the render context. At the virtual-row callback, obtain the current source from the current app state; a source that has since closed remains a legitimate boundary case.

**Kill list, 2 sites.** The navigation unwraps at [discussion.rs:182](../../desktop/src/story_view/discussion.rs:182) and [discussion.rs:483](../../desktop/src/story_view/discussion.rs:483). No whole test is killed.

**Blast radius and risk.** The discussion view and its caller/tests in `story_view/mod.rs`; no persistence. This does not establish that Rune currently invokes a stale callback after source closure. Preserve a boundary check there rather than assuming it cannot happen.

### Rejected or excluded seeds

- **Toast sentinel:** confirmed: normal notices use infinity and `poll` starts their timer ([construction](../../desktop/src/app/feed_actions.rs:94), [poll](../../desktop/src/app/poll.rs:199)). Pending and running are real phases, so replacing infinity with an enum still requires the transition. No qualifying kill without proving a changed timer start preserves behavior.
- **Remaining thread structures:** known reply IDs and loaded children intentionally differ ([comments.rs](../../desktop/src/comments.rs:217), [reply loading](../../desktop/src/comments.rs:250)). Ordered roots also exclude pruned unreadable nodes ([comments.rs](../../desktop/src/comments.rs:230)). Collapsing those structures has no demonstrated additional kill list.
- **Feed failures and cursors:** failed IDs correlate with placeholders, but refusing to open or save a failed row remains necessary with a row enum ([feed.rs](../../desktop/src/feed.rs:245), [reading_actions.rs](../../desktop/src/app/reading_actions.rs:72)). Display and pending cursors deliberately differ during a page turn, and pending anchors also serve reflow ([feed.rs](../../desktop/src/feed.rs:55), [resize test](../../desktop/src/app/tests/feed_arrivals.rs:19)). The request-metadata portion qualifies as F5.
- **Inspector drift:** confirmed by calculation from the checked-in conversion and the pinned Rune implementation’s whole-point rounding. Starting at 543 window points, zooming `1 → 1.1 → 1.25 → 1.5 → 1` produces 545 ([conversion](../../desktop/src/app/input.rs:34), [Rune `set_width`](</Users/shaance/.cargo/git/checkouts/rune-c9b805e9ed8a3730/55ef526/crates/rune-kit/src/shell/pane.rs:123>)). This is a separate geometry issue with no demonstrated impossible-state guard to delete, so it is excluded from the ranking. The existing test allows rounding tolerance ([window_layout.rs](../../desktop/src/app/tests/window_layout.rs:37)).

### Suggested landing order

1. **F1**, with the staged-replacement and capture-snapshot tests retained.
2. **F2**, then **F3**; each has its own verification surface.
3. **F4**, with native lifecycle verification; then **F5** and **F6**.
4. **F7** after F1, because both touch source-view arguments. F1/F2 also overlap `story.rs`, and F4/F6 overlap window input, so sequence those edits even though their behaviors can be verified independently.

This was a static, read-only inventory at `8774f82`. I did not run Cargo or exercise the UI. Context7 returned documentation for a different product named Rune, so the pane-rounding evidence uses this checkout’s pinned Rune source.

