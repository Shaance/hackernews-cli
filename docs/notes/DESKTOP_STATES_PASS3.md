**Verdict: 1 finding. Pass 3 has not converged.**

### Redundant clock reset after page and tab actions

**Impossible state.** An accepted page turn or tab switch reaches `App::apply` with an old timeout already running. Both actions replace the request with `Loading(..., None)` before returning a fill ([feed.rs:245](../../desktop/src/feed.rs:245), [feed.rs:265](../../desktop/src/feed.rs:265)).

**New shape.** Keep the loading-owned clock and rely on those constructors to start it unset.

**Kill list.** Delete the second `reset_clock()` call at [feed_actions.rs:128](../../desktop/src/app/feed_actions.rs:128). For the current `App::apply` callers, a fill-producing action is a page turn or tab switch ([input.rs:146](../../desktop/src/app/input.rs:146), [feed_view.rs:29](../../desktop/src/app/feed_view.rs:29), [feed_view.rs:336](../../desktop/src/app/feed_view.rs:336)).

**Blast radius.** One private app method; no signature changes. Keep the separate refresh reset, since `request` otherwise preserves an in-flight clock ([feed_actions.rs:17](../../desktop/src/app/feed_actions.rs:17), [feed_actions.rs:63](../../desktop/src/app/feed_actions.rs:63)). `FeedModel::apply` also accepts `Resize`, so a future caller passing that action through `App::apply` would need to choose its timeout behavior; current reflow goes through polling and preserves the clock ([feed.rs:291](../../desktop/src/feed.rs:291), [poll.rs:125](../../desktop/src/app/poll.rs:125)).

### Rejected candidates

- **Arrival direction.** Production constructs `-1` or `1`, but rendering multiplies the value directly; an enum would delete no guard or branch ([feed_actions.rs:119](../../desktop/src/app/feed_actions.rs:119), [feed_view.rs:81](../../desktop/src/app/feed_view.rs:81)).
- **Positive grid.** Conversion to `usize` occurs at the shared service boundary. Its empty-window guard remains necessary when the requested rank is beyond the snapshot, even with a positive count ([service/mod.rs:75](../../desktop/src/service/mod.rs:75), [hn_cli_service.rs:131](../../src/hn_cli_service.rs:131)). The grid tests exercise clamping and saturation, rather than only a type fact ([feed.rs:491](../../desktop/src/feed.rs:491)).
- **Reply batch.** The missing-parent guard still protects `merge_replies` from a batch made by another thread; a test constructs that case ([comments.rs:253](../../desktop/src/comments.rs:253), [comments.rs:937](../../desktop/src/comments.rs:937)). The one-production-caller `depth()` accessor keeps the batch fields private ([comments.rs:88](../../desktop/src/comments.rs:88), [bridge/mod.rs:298](../../desktop/src/bridge/mod.rs:298)).
- **Result captures and feed windows.** Failed root capture can coexist with successfully refreshed replies, so the empty-root fallback remains meaningful ([story.rs:668](../../desktop/src/story.rs:668), [discussion.rs:893](../../desktop/src/app/tests/discussion.rs:893)). A failed ranking still needs its error path ([feed.rs:298](../../desktop/src/feed.rs:298)).

This was a static, read-only check. No files were changed and Cargo was not run.

