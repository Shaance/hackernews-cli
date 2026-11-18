The highest-value candidates are pairing successful comment captures with their sources and preserving positive grid dimensions through the desktop code. The feed lifecycle seed is real at the public model boundary, but I found no current production path that accepts a **matching** result after settlement.

This is a static inventory at `0db3f1c`. I read the four pass 1 commits and did not run Cargo or exercise the UI. Activity counts are distinct commits touching the listed current paths in the last 90 days, without following paths through earlier splits. A kill is one source or assertion site; no whole test is listed unless all of it becomes unnecessary.

| ID | Impossible state | New shape | Kill sites | Activity | Leverage |
|---|---|---|---:|---:|---:|
| F1 | Failed root capture carries sources | `Result<CapturedComments, Error>` | 4 | 17 | **68** |
| F2 | Desktop grid has zero rows or columns | Positive grid dimensions | 4 | 11 | **44** |
| F3 | Nonempty reply batch has no parent | Validated batch owns parent and depth | 1 | 19 | **19** |
| F4 | A timeout check has no loading request | Timer owned by loading state | 1 | 10 | **10** |
| F5 | A narrowed key reaches an excluded match arm | Exhaustive, single-pass dispatch | 2 | 5 | **10** |
| F6 | Failed feed ranking also carries successful rows | `Result<FeedWindow, Error>` | 1 | 7 | **7** |
| F7 | A rendered comment has neither captured nor loaded content after the presence check | Match a present content source once | 2 | 3 | **6** |
| F8 | A displayed tab maps from an out-of-range index | Capture `StoryType` with its label | 1 | 6 | **6** |
| F9 | A subtree containing its root is empty | Traverse descendants directly | 1 | 6 | **6** |
| F10 | A pending arrival is `Some(Arrival::None)` | `Option<Arrival>` with only real arrivals inside | 1 | 5 | **5** |

### F1. Keep successful comments and their capture together

[StoryPhase](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/bridge/mod.rs:67), [StoryFilled and StoryUpdate](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/story_io.rs:20) allow failed comments alongside unrelated sources. Production creates an empty source vector on failure; a test can construct the contradictory combination ([story.rs:730](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/story.rs:730)).

**Restructuring and boundary.** After the service returns roots, construct `Result<CapturedComments, String>`, with comments and their derived sources in the success case. Carry that pair through initial capture and refresh. Pair a successful summary with its captured sources as well, so synchronous landing does not recreate an empty source vector on failure.

**Kill list, 4.** The error-to-empty-source fallbacks at [bridge/mod.rs:123](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/bridge/mod.rs:123), [155](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/bridge/mod.rs:155), [285](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/bridge/mod.rs:285), and [reading_actions.rs:235](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/app/reading_actions.rs:235).

**Blast radius and risks.** Bridge phases, `StoryFilled`, `StoryUpdate`, refresh landing, poll patterns and their direct tests change. No serialization changes. Retain the late-initial-result test, changing its failure fixture to `Err`; retain independent descendant refresh when root refresh fails ([story.rs:675](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/story.rs:675)). Preserve the distinction between current sources and the older capture behind an open summary ([summary_lifecycle.rs:267](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/app/tests/summary_lifecycle.rs:267)).

### F2. Preserve positive grid dimensions

The window produces one or two columns and clamps measured rows to at least one ([window.rs:63](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/app/window.rs:63)). The default is also positive ([app/mod.rs:53](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/app/mod.rs:53)), but those facts are stored and passed as raw `usize` pairs.

**Restructuring and boundary.** Parse Rune’s measured geometry into a grid with positive rows and a one/two-column enum. Carry it through request lifecycle and views. Make the desktop page size positive or fixed as well: the shared `fill_grid` returns zero when its raw page size is zero ([feed_state.rs:82](/Users/shaance/Documents/projects/hackernews-cli/src/feed_state.rs:82)); the desktop currently initializes it to 30 and never changes it ([feed.rs:112](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/feed.rs:112)).

**Kill list, 4.** `capacity > 0` and `cols.max(1)` in [feed_view.rs:67](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/app/feed_view.rs:67) and [feed_view.rs:72](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/app/feed_view.rs:72); `cols.max(1)` in [input.rs:127](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/app/input.rs:127); `need.count > 0` in [poll.rs:146](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/app/poll.rs:146), provided reflow uses the positive capacity rather than laundering it back through a raw fill.

**Blast radius and risks.** App measurement, request/lifecycle signatures, feed views, input and grid fixtures change. `FeedModel` exposes raw grid/page-size APIs, so their callers need an explicit boundary decision. Keep the shared library’s zero-dimension behavior and tests; do not change `desktop.json`. Preserve resize and page-turn anchoring ([feed_arrivals.rs:19](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/app/tests/feed_arrivals.rs:19)).

### F3. Carry the validated reply parent

A nonempty `reply_batch` proves its parent exists: the function returns an empty vector if lookup fails ([comments.rs:230](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/comments.rs:230)). `Story::request_replies` then passes the raw ID to a second fallible lookup ([story.rs:443](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/story.rs:443)).

**Restructuring and boundary.** At the thread lookup, produce a batch containing the parent, its depth and nonempty requested IDs. Calculate depth from the already found node. Missing parents and empty batches remain legitimate rejections at that boundary.

**Kill list, 1.** The second missing-parent `?` at [story.rs:448](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/story.rs:448). The general `tree_depth` check remains for its other callers.

**Blast radius and risks.** `reply_batch`, `request_replies`, discussion controls and their tests change; no persistence. Preserve the 50-item batches, retry ordering and empty-batch behavior ([comments.rs:889](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/comments.rs:889)).

### F4. Put the timeout clock inside loading

`loading_since` is separate from `RequestLifecycle` ([app/mod.rs:124](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/app/mod.rs:124)). Poll checks that the request is loading before calling a method that checks the same fact again ([poll.rs:164](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/app/poll.rs:164)).

**Restructuring and boundary.** Let `Loading` own a clock state such as awaiting its first poll or running since a frame time. Perform timeout settlement through that loading state.

**Kill list, 1.** The `!self.request.is_loading()` half of [feed.rs:353](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/feed.rs:353). The elapsed-time check remains.

**Blast radius and risks.** Lifecycle patterns, feed timeout API, polling and fixtures change; no persistence. A new tab or explicit refresh resets the clock, while a resize of the same in-flight request preserves it ([feed_arrivals.rs:358](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/app/tests/feed_arrivals.rs:358)). Do not count `settle`’s other guard as deleted without also changing its other callers.

### F5. Make key dispatch exhaustive once

The outer zoom match has already excluded other window actions before the inner match reaches `unreachable!()` ([input.rs:23](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/app/input.rs:23)). The story-key handler similarly returns for `Back` before a later match still includes an unreachable `Back` arm ([input.rs:206](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/app/input.rs:206)).

**Restructuring and boundary.** Use one exhaustive dispatch per action: have the window match produce the new zoom for zoom cases and return for other cases; handle `Back` in the story match while retaining its priority over chooser and source navigation. Key parsing remains in [keys.rs:221](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/keys.rs:221).

**Kill list, 2.** [input.rs:30](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/app/input.rs:30) and [input.rs:274](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/app/input.rs:274).

**Blast radius and risks.** Local control flow in `input.rs`; existing keyboard and navigation tests remain behavior tests. No new enum or persistence change is needed. Verify that Escape still unwinds chooser, source and story in that order.

### F6. Separate a failed ranking from feed rows

`Filled` permits an error alongside IDs and items ([feed.rs:61](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/feed.rs:61)). The bridge fabricates empty vectors when ranking fails ([bridge/feed.rs:67](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/bridge/feed.rs:67)).

**Restructuring and boundary.** Return `Filled { tab, from_rank, outcome: Result<FeedWindow, String> }`; construct the result at the ranking boundary and keep request identity outside it for stale-result rejection.

**Kill list, 1.** The assertion that a failed ranking has an empty item vector at [bridge/mod.rs:391](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/bridge/mod.rs:391) becomes a fact of the error case. The same test must retain its error and other-tab assertions.

**Blast radius and risks.** Bridge fill construction, held-window construction, `FeedModel::apply_fill` and inline tests change; no serialization. The real failure branch that keeps existing rows and shows retry remains ([feed.rs:267](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/feed.rs:267)).

### F7. Select comment content once

After [discussion.rs:186](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/story_view/discussion.rs:186), either a captured source or loaded node exists. Two later fallbacks still allow neither to supply text.

**Restructuring and boundary.** Match `(captured, node)` once into captured content, loaded-node content, or the existing no-row return. The virtual-row callback remains a genuine boundary because the current story can differ from the one that produced the row.

**Kill list, 2.** The empty-string fallbacks for author and body at [discussion.rs:195](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/story_view/discussion.rs:195) and [196](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/story_view/discussion.rs:196).

**Blast radius and risks.** Local to `comment_row`; no persistence. Keep captured-source precedence and the no-content return for a stale row.

### F8. Carry tab identity into the callback

The tab view enumerates exactly four labels and turns each index back into a `StoryType` ([feed_view.rs:21](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/app/feed_view.rs:21)). `tab_at` accepts every `usize` and maps unexpected values to Top ([feed.rs:406](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/feed.rs:406)).

**Restructuring and boundary.** Iterate over `(StoryType, label)` pairs and capture the enum in the press callback. Keep the index only for the visual grid.

**Kill list, 1.** The invalid-index fallback in [feed.rs:411](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/feed.rs:411); the whole one-caller `tab_at` helper can go. Its legitimate index-zero mapping is expressed by the Top pair.

**Blast radius and risks.** `feed_tabs` and `tab_at`; no persisted tab format change. Preserve tab ordering and `tab_index`, which has other callers.

### F9. Return descendants directly

`subtree` always pushes its input before returning, so its returned vector cannot be empty ([comments.rs:418](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/comments.rs:418)). Both current callers immediately exclude that root.

**Restructuring and boundary.** Replace it with a descendant traversal seeded from the root’s children. An empty descendant vector is then a valid result; no external parse is involved.

**Kill list, 1.** The defensive `saturating_sub(1)` in [comments.rs:414](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/comments.rs:414). The now-unnecessary `skip(1)` at [comments.rs:194](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/comments.rs:194) also disappears, but is not counted as a defensive site.

**Blast radius and risks.** Two in-repository callers and the public `Thread::subtree` method change. Preserve depth-first order and check for consumers of the exported library API before landing. No serialization changes.

### F10. Remove the nested absence in arrival state

`pending_arrival: Option<Arrival>` can encode both `None` and `Some(Arrival::None)`, but constructors use the latter enum variant only for the separate active-arrival field ([app/mod.rs:79](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/app/mod.rs:79), [feed_actions.rs:115](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/app/feed_actions.rs:115)).

**Restructuring and boundary.** Remove `Arrival::None`; use `Option<Arrival>` for both pending and active arrivals. Construct real arrivals at initial setup, tab switch and page turn.

**Kill list, 1.** The `Arrival::None` match arm inside a block already guarded against it at [feed_view.rs:82](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/app/feed_view.rs:82).

**Blast radius and risks.** App initialization, arrival acceptance, feed rendering, polling and arrival tests change; no persistence. Preserve refresh and story-open suppression of arrival motion.

The sweep also found a separate behavior issue with no qualifying kill list: if the Saved path cannot be resolved, `SavedView::load` records an error with no path ([bookmarks.rs:26](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/bookmarks.rs:26)). A later toggle skips the unreadable-file guard, updates memory and clears the error ([bookmarks.rs:140](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/bookmarks.rs:140)), allowing the normal success toast ([feed_actions.rs:90](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/app/feed_actions.rs:90)). Correcting that would change behavior, so it is outside the ranking.

### Rejected seeds

- **Matching fill in `BeforeFirst` or `Settled`.** `apply_fill` checks only tab and cursor, so it accepts such a value; the feed test applies a failure and then success after settlement ([feed.rs:263](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/feed.rs:263), [feed.rs:732](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/feed.rs:732)). I found no matching production path: synchronous fills follow `Loading`; timeout cancels the worker; a Saved switch can leave an older network answer pending, but its tab differs and must be rejected ([feed_actions.rs:15](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/app/feed_actions.rs:15), [poll.rs:163](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/app/poll.rs:163)). Rune’s pinned `Latest` also drops superseded or cancelled answers ([latest.rs:142](/Users/shaance/.cargo/git/checkouts/rune-c9b805e9ed8a3730/55ef526/crates/rune/src/latest.rs:142), [latest.rs:158](/Users/shaance/.cargo/git/checkouts/rune-c9b805e9ed8a3730/55ef526/crates/rune/src/latest.rs:158)). An in-flight witness could constrain acceptance, but a boundary must still reject a result with no matching in-flight request. I cannot claim a net guard deletion from that change alone.
- **Toast timer.** Infinity means “start on the next frame”; poll has the frame clock needed to start it ([feed_actions.rs:93](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/app/feed_actions.rs:93), [poll.rs:199](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/app/poll.rs:199)). A scene also supplies an already-running deadline ([scenes.rs:12](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/app/scenes.rs:12)). An enum would name both real phases but does not establish a deletion. F4 concerns the separate feed timeout clock.
- **Other pass 1 exclusions.** Known reply IDs still differ from loaded children ([comments.rs:72](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/comments.rs:72)); failed feed rows still need open/save refusal ([reading_actions.rs:72](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/app/reading_actions.rs:72), [feed_actions.rs:87](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/app/feed_actions.rs:87)); pending and displayed cursors still serve different page-turn and reflow states ([feed.rs:82](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/feed.rs:82)). The inspector drift exclusion was addressed by the pass 1 window-point width change ([window_layout.rs:28](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/app/window_layout.rs:28)).
- **Citation and chooser checks after pass 1.** Empty or invalid pointer selections and a closed source remain real input or lifecycle cases ([story.rs:538](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/story.rs:538), [discussion.rs:418](/Users/shaance/Documents/projects/hackernews-cli/desktop/src/story_view/discussion.rs:418)). I did not count checks that would merely move to those boundaries.

### Suggested landing order

1. **F1**, then **F2**. Verify capture replacement and grid reflow separately.
2. **F3**, then **F4** and **F5**. F4 overlaps F2’s request lifecycle; F5 overlaps F2’s input file.
3. **F6**, then **F7**, **F8** and **F9**. F7 overlaps F3’s discussion view; F9 overlaps its thread code.
4. **F10** after the feed changes, since it overlaps feed rendering and polling.

A possible standing rule is: *Carry evidence established at a boundary to its consumers, and keep validation where raw events, geometry or external data enter.*

