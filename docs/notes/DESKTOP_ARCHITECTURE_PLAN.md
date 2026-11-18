# hn-desktop architecture assessment

2026-11-17, read-only Codex (gpt-6.1-terra, max) at 24404bd. Citations predate the ship fixes; refresh before each step. The coordinator trims the proposed module list before briefing: modules that would hold one small function merge into their neighbour.

## Recommendation

After the other fixes land, start the behavior-preserving migration with **`app.rs` and `story_view.rs`**. They combine the most independently changing responsibilities and already have substantial behavior coverage. Follow with the bridge/service, keyboard catalog, discussion model, and story model. Keep `fixtures.rs` together: its size is mostly one coherent, deterministic scene dataset.

There is a necessary design correction to the requested acyclic graph: the **current graph is cyclic**. `app` composes `story_view`, which imports `App`; `app` also owns `Settings`, whose view callbacks import `App`; and `bridge` imports `service`, which imports `bridge::SummaryError` ([app.rs:38](../../desktop/src/app.rs:38), [story_view.rs:25](../../desktop/src/story_view.rs:25), [settings.rs:151](../../desktop/src/settings.rs:151), [service.rs:31](../../desktop/src/service.rs:31)). Separating composition from the state type, settings model from its view, and story request/result types from the bridge resolves those dependencies without a new event system.

“Split now” below means **first in the later migration**, not a change in this assessment. Counts and citations describe the working-tree snapshot at HEAD `24404bdda009e78a974a6b64b7c8d9b506da7b30`; they should be refreshed after the pending fixes.

## Inventory

Physical lines include comments and blanks. For source files, “test” begins at the terminal `#[cfg(test)]` section; integration-test files are entirely test lines. `fixtures.rs` counts as production because headless scenes and the fake service use it at runtime.

| File | Production | Test | Decision |
|---|---:|---:|---|
| [src/app.rs:2253](../../desktop/src/app.rs:2253) | **2,252** | 3,500 | Split now |
| [src/bookmarks.rs:158](../../desktop/src/bookmarks.rs:158) | 157 | 115 | Under threshold |
| [src/bridge.rs:482](../../desktop/src/bridge.rs:482) | **481** | 342 | Follow-up |
| [src/comments.rs:743](../../desktop/src/comments.rs:743) | **742** | 396 | Follow-up |
| [src/feed.rs:360](../../desktop/src/feed.rs:360) | 359 | 369 | Under threshold |
| [src/fixtures.rs:464](../../desktop/src/fixtures.rs:464) | **463** | 83 | Leave |
| [src/help.rs:1](../../desktop/src/help.rs:1) | 121 | 0 | Under threshold |
| [src/keys.rs:564](../../desktop/src/keys.rs:564) | **563** | 508 | Follow-up |
| [src/layout.rs:122](../../desktop/src/layout.rs:122) | 121 | 58 | Under threshold |
| [src/lib.rs:1](../../desktop/src/lib.rs:1) | 20 | 0 | Under threshold |
| [src/main.rs:1](../../desktop/src/main.rs:1) | 20 | 0 | Under threshold |
| [src/service.rs:410](../../desktop/src/service.rs:410) | **409** | 109 | Follow-up |
| [src/settings.rs:1](../../desktop/src/settings.rs:1) | 327 | 0 | Split to remove a cycle |
| [src/story.rs:587](../../desktop/src/story.rs:587) | **586** | 311 | Follow-up |
| [src/story_view.rs:1186](../../desktop/src/story_view.rs:1186) | **1,185** | 98 | Split now |
| [src/webview.rs:1](../../desktop/src/webview.rs:1) | 266 | 0 | Under threshold |
| [tests/bookmarks_env.rs:1](../../desktop/tests/bookmarks_env.rs:1) | 0 | 47 | Integration test |
| [tests/settings_env.rs:1](../../desktop/tests/settings_env.rs:1) | 0 | 71 | Integration test |

Total: **8,072 production / 6,007 test lines**, including the two integration tests.

## Responsibility maps

- **`app.rs` — split now:** The state and lifecycle cluster owns `App`, initialization, feed requests, story parking, settings, layout writes and native-page synchronization; it consumes user actions and bridge results, mutates state, and performs persistence/browser/native side effects (`app.rs:126`, `:237`, `:375`, `:545`, `:737`, `:899`). The input cluster maps window/feed/story keys into those transitions and Rune `Tx` operations (`:1069`, `:1219`); the presentation cluster measures and renders the shell, feed, rows, banners and footer (`:1440`, `:1617`, `:1701`, `:1852`). `run`, scenes and `spec` compose them (`:2088`, `:2186`, `:2229`). Tests exercise column paging, story phases, native ownership and persistence (`:2536`, `:2416`, `:4760`, `:5724`). These are distinct reasons to change.

- **`story_view.rs` — split now:** `article_pane` renders the live/fallback article from `Story`, theme and `Pane`; summary functions render generation, errors and citations; discussion functions render the tree, link chooser and source viewer (`story_view.rs:46`, `:59`, `:318`, `:622`, `:782`, `:1003`, `:1086`). They return Rune `El`s and register callbacks; the app’s `view` composes them (`app.rs:1496`), while view tests and app interaction tests cover comment geometry and citations (`story_view.rs:1212`, `app.rs:3573`). Article, summary and discussion presentation can change independently.

- **`comments.rs` — follow-up:** Tree construction and `merge_comments`/`merge_replies` consume comment captures and mutate the indexed tree; `rows`, folding, cursor movement and re-rooting derive or change its navigation (`comments.rs:104`, `:126`, `:180`, `:257`, `:321`, `:493`). `Story` owns the thread and the discussion view reads it (`story.rs:149`, `story_view.rs:645`); tests cover partial reply batches and depth-first navigation (`comments.rs:895`, `:919`). The tree’s data updates and its navigation are meaningful seams, but their invariants are tightly coupled.

- **`story.rs` — follow-up:** `land`/`land_update`, reply handling and refresh consume bridge results and mutate the active story; summary replacement, citation/source navigation and link selection own separate reading contracts (`story.rs:144`, `:207`, `:281`, `:302`, `:353`, `:412`, `:452`, `:537`). `App::poll` and the story view consume that state (`app.rs:919`, `story_view.rs:244`); tests cover failed replacements and staged citations (`story.rs:697`, `:732`). Split the contracts and actions only after preserving their shared snapshot relationships.

- **`bridge.rs` — follow-up:** Feed requests return ranked fills through `Latest`; story requests independently run captures and summaries, manage wakers and parked tasks, and return typed updates (`bridge.rs:39`, `:51`, `:200`, `:208`, `:302`, `:348`). `App` calls the bridge and polls results (`app.rs:269`, `:914`); tests cover superseded feed requests and story wakeups (`bridge.rs:567`, `:699`). The transport contracts and feed-job path can be separated from story-job orchestration.

- **`keys.rs` — follow-up:** `key_code`, `story_action`, `story_key` and `window_key` translate input into actions; `HelpAction`, `HelpGroup` and `HELP` describe the displayed shortcuts (`keys.rs:19`, `:63`, `:152`, `:221`, `:243`, `:279`). Routing is consumed by `App`, the catalog by `help::view`, and parity tests check both directions (`app.rs:1069`, `help.rs:21`, `keys.rs:893`, `:933`). The catalog is a separable presentation contract, provided those parity tests move with it or keep importing it.

- **`service.rs` — follow-up:** `Source::Live` obtains rankings, captures and summaries through `hn_lib`; `FakeService` supplies deterministic data, failures, counters and delays for headless execution (`service.rs:35`, `:66`, `:93`, `:156`, `:179`, `:218`). The bridge calls both; service and bridge tests exercise paging, captures and delayed requests (`bridge.rs:387`, `service.rs:415`, `bridge.rs:567`). The fake and live adapters have different reasons to change, although the fake is also used in production headless scenes.

- **`fixtures.rs` — leave:** `STORIES`, `ARTICLE`, `THREAD` and the functions producing items, comments, sources and summaries form one frozen dataset for the fake service and scenes (`fixtures.rs:13`, `:131`, `:170`, `:207`, `:261`, `:276`, `:309`; callers `service.rs:237`, `app.rs:2114`). Its tests check dataset consistency, including citation references (`fixtures.rs:469`, `:476`). Its single stable reason to change is the deterministic scene dataset; splitting the catalog would add navigation without separating ownership.

## Proposed module tree

Paths below are relative to `desktop/src`. A module’s listed dependencies are its intended *production* dependencies; test-only imports can point back to the relevant behavior tests. Existing callers should be updated to the new paths rather than given parallel compatibility paths.

| Module | Purpose, moves, dependencies |
|---|---|
| `app/model.rs` | Define `App`, its fields and state enums, `init`, `configured`, `scroll_observer`, `Live`, and the moved `Pane` contract; depends on domain types, bridge, `settings/model`, Rune and fixtures, not on view modules (`app.rs:75`, `:126`, `:217`, `:370`, `:806`; `story_view.rs:46`). |
| `app/feed_actions.rs` | Own `request`, `refresh`, `toggle_saved*`, `apply`, `accept_arrival` and `cells`; depends on the app model, feed, bookmarks, bridge and service (`app.rs:237`). |
| `app/reading_actions.rs` | Own story open/park/back, reply and citation activation, summary retry and comment refresh; depends on the app model, live-page actions, reading navigation, bridge and story (`app.rs:375`, `:468`, `:597`, `:674`, `:699`). |
| `app/live_page.rs` | Own `live_capable`, `live_mode`, `pane`, `live_rect`, `sync_live` and `hide_live`; depends on the app model and `webview` (`app.rs:504`). |
| `app/layout_lifecycle.rs` | Own `restore`, `snapshot` and `write_layout`; depends on the app model and existing `layout.rs` persistence (`app.rs:766`). |
| `app/settings_actions.rs` | Own settings open/close/apply and authentication-failure routing; depends on the app model, settings model, feed and reading actions (`app.rs:654`, `:660`, `:721`). |
| `app/input.rs` | Own `on_key`, `on_story_key`, `keyboard_navigation` and `pointer_wheel`; depends on `keys`, the app model and the relevant actions (`app.rs:201`, `:1069`, `:1219`). |
| `app/reading_navigation.rs` | Own `reveal_citation` and `thread_moved`, the Rune focus/scroll consequences of reading navigation; depends on `Story` and `Tx` (`app.rs:1391`). |
| `app/poll.rs` | Own `setup` and `poll`, including the order in which results, timeouts, native state and persistence are processed; depends on the model and action modules (`app.rs:889`). |
| `app/window.rs` | Own `view`, `story_main`, `run`, `flag_value`, `close` and `spec`; compose the shell, views, scenes, input and poll without owning their implementations (`app.rs:1440`, `:1605`, `:2186`). |
| `app/chrome.rs` | Own the shared window affordances `brand`, `pane_motion`, `saved_dot` and `passive_hint`; depends on Rune, not app state (`app.rs:1562`, `:1597`, `:1826`, `:1909`). |
| `app/feed_view.rs` and `app/feed_row.rs` | The first renders tabs, grid, loading/error banners and footer; the second owns `story_row`, `meta` and `act`; both depend on the app model/actions and chrome (`app.rs:1570`, `:1617`, `:1701`, `:1852`). Move `tab_index` and `tab_at` to existing `feed.rs`, where both feed actions and its view can use them (`app.rs:2070`). |
| `app/scenes.rs` | Own the existing `scene_*` functions registered by `spec`, using app actions and `fixtures`; `scene_story` remains production scene setup (`app.rs:2088`, `:2114`, `:2231`). |
| `settings/model.rs`, `settings/view.rs` | Keep `Settings`, draft edits and `apply` in the model; move `Settings::view` to the view, which depends on the model and typed `App` callbacks (`settings.rs:19`, `:104`, `:139`). |
| `story_view/mod.rs` | Compose `inspector_pane` from summary and discussion/source surfaces; depend on those child views and `Story` (`story_view.rs:244`). |
| `story_view/article.rs` | Own `article_pane`, `fallback_banner`, `article_header`, `article_body`, `comments_label` and `host_of`; depend on `Story`, the app’s `Pane` contract and reading chrome (`story_view.rs:59`, `:1161`). |
| `story_view/summary.rs` | Own `summary_card`, `expanded_summary`, `summary_header`, `bullets`, `generating`, `failure`, `unconfigured` and `folded_summary`; depend on `Story`, reading chrome and existing `App` callbacks (`story_view.rs:318`). |
| `story_view/discussion.rs` and `story_view/comment_row.rs` | The first owns `discussion_pane`, `discussion`, `discussion_header`, `trail_strip`, `link_chooser` and `source_viewer`; the second owns `comment_row`, shared by discussion and citation views. They depend on `Story`, app callbacks and the comment-row view (`story_view.rs:622`, `:782`, `:1003`, `:1086`). |
| `story_view/reading_chrome.rs` | Own `reading_transition`, `skeleton_line`, `story_footer`, `story_toolbar` and its `hint`; depend on Rune, `Story`, app callbacks and `app/chrome` (`story_view.rs:307`, `:496`, `:1122`). |
| `story_io.rs` | Define `StoryRequest`, `StoryFilled`, `StoryUpdate`, `CommentsRefreshed`, `ReplyRequest`, `ReplyFilled` and `SummaryError`; depend on `hn_lib` data types, so service, bridge and story can all consume the contracts without depending on one another (`bridge.rs:83`, `:125`, `:150`). |
| `bridge/mod.rs`, `bridge/feed.rs` | Keep `Bridge`, its runtime, story phases, story/reply/refresh methods and `run_story`/`run_replies`/`run_refresh` in the bridge; move `Request`, feed `request`/`fill_now`/`take`/`cancel_feed` and `run` to the feed job module. Both depend on the bridge state, service and relevant contracts (`bridge.rs:39`, `:51`, `:200`, `:387`, `:426`). |
| `service/mod.rs`, `service/fake.rs` | Keep `Source` and live dispatch in the former; move `FakeService`, its closures and `HackerNewsCliService` implementation to the latter. The fake depends on fixtures and `story_io`; `Source` depends on the fake (`service.rs:35`, `:212`, `:337`). |
| `keyboard_help.rs` | Own `HelpAction`, `HelpRow`, `HelpGroup` and `HELP`; depend on the action enums in `keys.rs`, and have `help.rs` and the parity tests import the catalog (`keys.rs:243`, `:279`, `help.rs:14`). |
| `comments/model.rs`, `comments/tree_updates.rs`, `comments/navigation.rs` | Define `Thread`, `Node`, `Navigation`, `Row` and basic queries in the model; move construction/merge/reply batching to tree updates and row walking/folding/cursor/re-root operations to navigation. Updates may call navigation; both depend on the model (`comments.rs:57`, `:104`, `:126`, `:257`, `:321`, `:493`). |
| `story/model.rs`, `story/summary.rs` | Define `Story`, `SourceView`, `LinkItem`, `LinkChooser`, `opening` and `id` in the model; put `Bullet`, `Summary`, summary states and `summary_of` in summary. The model depends on summary, comments and `story_io` (`story.rs:24`, `:101`, `:144`, `:573`). |
| `story/capture.rs`, `story/citation.rs`, `story/link_actions.rs` | Keep capture landing/retry/refresh in the first, citation selection/source navigation in the second, and `open_links`/selection/close in the third; capture may call citation repair, while all depend on the model and relevant lower-level contracts (`story.rs:207`, `:302`, `:343`, `:452`, `:537`). |

**Dependency direction.** At the responsibility-module level, the proposed graph is acyclic: window composition → views/input/poll → app actions and state → story/feed/bridge → service and data contracts. `story_view` may still register typed `App` callbacks such as `on_layout` and `on_link`; virtual rows may still read the current story through `app.story()` (`story_view.rs:79`, `:458`, `:684`). Settings/help similarly use typed callbacks (`settings.rs:151`, `help.rs:65`). Those are existing patterns, not a new reverse dependency on the window composer. This graph is a design assessment; it has not been compiler-verified.

## Test organisation

Keep the 102 app tests as **unit-test children of `app`**, so they can exercise state and Rune callbacks without making new production APIs. Preserve the six existing `#[ignore]` attributes and the macOS gate (`app.rs:2253`, `:3500`, `:3967`, `:4056`, `:4498`, `:4684`, `:4713`, `:4790`).

Suggested behavior files under `app/tests/`:

- `feed_paging.rs`, `feed_arrivals.rs`, and `saved.rs`: column navigation, requests, loading/motion, save and hover (`app.rs:2536`, `:2692`, `:2964`, `:5499`).
- `story_lifecycle.rs` and `summary_lifecycle.rs`: opening, parking, captures, retries and stale results (`app.rs:2416`, `:4760`, `:5040`, `:5230`).
- `discussion.rs` and `citation.rs`: thread navigation, reply loading, focus, source return and scroll (`app.rs:3001`, `:3226`, `:4289`, `:4562`).
- `keys.rs`, `window_layout.rs`, and `settings.rs`: routing, zoom, inspector, persistence and dialog contracts (`app.rs:2258`, `:3819`, `:5632`, `:5656`, `:5700`).

Put cross-file `app`, `press`, `route`, `poll_until`, `temp_dir`, `inspector` and `saved_store` in `app/tests/support.rs` only where the resulting files share them (`app.rs:2330`, `:2341`, `:2958`, `:4270`, `:5446`, `:5489`, `:5651`). Keep `settle_feed` with feed tests, `gated_summary_fixture` with summary tests and `long_discussion` with discussion tests. The nested `overflow` builder is used only in one ignored renderer test and should remain there (`app.rs:3501`, `:3510`, `:3534`). `scene_story` is **not** test-only: production scenes and tests both call it (`app.rs:2114`, `:2127`, `:2268`).

Keep the two XDG integration tests in separate binaries because they mutate process-wide environment variables (`bookmarks_env.rs:1`, `settings_env.rs:1`).

## Ordered migration steps with verification

First wait for the other fixes, refresh this inventory, and establish the baseline from their resulting tree. Each step below should be its own reviewable move.

**Verification for every step:** from `desktop/`, run `cargo test --release --locked` and `bash scripts/check_goldens.sh`; compare the frozen `verification/expected` files and the generated scene artifacts byte-for-byte with the pre-step baseline. Review the move diff for changed logic. The golden script builds and writes `verification/actual`, so schedule it after the other run releases the shared target directory (`check_goldens.sh:24`, `:72`, `:74`, `:111`).

1. Move app tests into behavior files and shared test support, retaining attributes and scene registration. Verify test discovery as well as the common checks.
2. Move the story request/result contracts to `story_io`; then split the fake service and feed bridge job. This removes the bridge/service cycle and should preserve cancellation, wakeup and headless behavior (`bridge.rs:567`, `:699`, `:811`).
3. Separate settings model/view, then separate app state/actions from window composition and input. Move `Pane` and shared chrome as needed to keep the dependency direction above.
4. Move feed presentation and scene registration. Keep row keys, callback order, measured-grid writes and motion identities unchanged (`app.rs:1455`, `:1471`, `:1643`).
5. Move article, summary, discussion/comment-row and reading chrome views. Preserve virtual-row closures and the order in which the inspector and overlays are assembled (`story_view.rs:244`, `:678`, `:1112`).
6. Move the keyboard-help catalog, retaining both routing-versus-help parity tests (`keys.rs:893`, `:933`).
7. Split comment tree updates from navigation, then split story contracts/capture/citation/link actions. Keep the underlying state representation and transition order unchanged (`comments.rs:180`, `:353`; `story.rs:281`, `:424`).

Steps 2–5 overlap imports and module declarations: **serialize them**. Steps 6–7 are also best serialized against story-view changes because they touch the types those views consume. Recheck the plan if the pending fixes change any of these boundaries.

The requested byte check is stronger than the existing script: on macOS it compares canonicalized trees exactly, but allows PNG differences; off macOS it defaults to a semantic tree comparison (`check_goldens.sh:1`, `:33`, `:54`, `:144`). Establish repeatable raw-artifact baselines first; if they are not repeatable, report that blocker instead of claiming byte identity. Even identical artifacts and passing tests do not alone prove every source change was a pure move; the diff review supplies that evidence.

## Risks

- **Visibility and public paths.** Sibling modules cannot access private fields as freely as the current inline code. Prefer narrowly scoped `pub(super)` where needed; do not widen everything to `pub(crate)`. The crate exposes its modules as a library, and the integration tests use public paths, although the package is unpublished (`lib.rs:1`, `:7`; `Cargo.toml:1`; `tests/settings_env.rs:8`). Check for consumers before removing any exposed path.
- **Rune signatures and captured state.** `AppSpec` requires function pointers, while view callbacks, `virtual_rows`, scroll observers and `on_layout` carry specific state types and lifetimes. Moves must preserve these signatures and callback ordering (`app.rs:217`, `:1504`, `:2229`; `story_view.rs:59`, `:244`, `:684`).
- **Platform and native lifetime.** `webview.rs` exports distinct macOS and non-macOS implementations; creation, bounds, visibility, callbacks and destruction are UI-thread work (`webview.rs:11`, `:21`, `:244`). Headless goldens exercise the extracted fallback, not a live `WKWebView` (`desktop/README.md:184`). After the live-page move, use macOS window proof and supported non-macOS CI in addition to the common checks.
- **Test-path relocation.** `bridge.rs` includes a test fixture via `#[path = "../../tests/support/http_fixture.rs"]`; changing its containing directory requires updating that relative path (`bridge.rs:482`).

## State-modelling smells noticed

These are candidates for the later pass, not diagnosed bugs:

- `Story::citation` and `citation_focus` rely on the current citation list and summary visibility; selection and replacement code explicitly clamp or clear them (`story.rs:161`, `:298`, `:376`, `:424`).
- The displayed `summary` and its `sources`, `current_sources`, and `staged_summary` have distinct but correlated lifetimes. In particular, an open source delays replacement; preserving that distinction is a requirement for any future model change (`story.rs:153`, `:281`, `:424`).
- `Thread` maintains `nodes`, `index`, `roots`, and each node’s `children`/`known_children` in parallel; merges and pruning update several of them together (`comments.rs:64`, `:104`, `:146`, `:232`, `:257`).
- `FeedModel` tracks failed IDs separately from placeholder stories and tracks a pending cursor separately from the displayed cursor (`feed.rs:48`, `:125`, `:227`).
- `Live`, `native_window`, `live_bounds`, and `live_shown` jointly describe native-child ownership and visibility. `Live::Ready` means a child was created; actual page readiness is checked separately (`app.rs:113`, `:187`, `:545`, `:571`).
- `LinkChooser` exposes an item vector and selected index, while the opening path refuses an empty chooser and initializes the index to zero (`story.rs:113`, `:452`, `:468`).
- Toast timing uses `f64::INFINITY` as an unstarted sentinel before `poll` replaces it with a deadline (`app.rs:75`, `:316`, `:1050`). Zoom is stored as `f32` but key handling expects it to be one of the fixed supported steps (`layout.rs:18`, `:41`; `app.rs:1087`).

I did not find an `Option` I can substantiate as never `None` from this read-only pass.

## Residue

`fixtures.rs` is the only flagged file recommended to remain intact: it owns one coherent offline dataset. The other flagged files remain unchanged in this assessment and are assigned to the later migration above. No files were edited, and no build, test, or golden command was run. No agents were delegated work.

