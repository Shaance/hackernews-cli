# OpenRouter implementation, 2026-10-19

Endpoint: local. Root is the only writer. Preserve existing prototype work. No edits to private summarizer or original checkout, no commit/push. Codex authorized by the existing conversation.

Use the accepted CLEAN_IMPLEMENTATION_PLAN.md product contract. Replace bridge calls with direct OpenRouter and local Readability extraction. Initial profiles: openai/gpt-oss-20b low reasoning / openai/gpt-oss-120b medium reasoning; no guaranteed quality or latency claim.

Milestones: (1) source preparation + transport; (2) settings and reader lifetime; (3) focused regressions + real PTY and provider acceptance. Keep one reader owner. Rename disposable summary_prototype to reader; preserve browser module behavior except latest-target reconciliation and platform gating.

New surfaces before edits: reader/config.rs owns config file + Profile (bookmarks owns unrelated data); reader/settings.rs owns one settings screen; reader/jobs.rs owns bounded request admission and completion (keeps main reader data definitions small); reader/sources.rs owns article extraction and captured discussion inputs; reader/network.rs becomes OpenRouter HTTP transport. Existing reader interaction/text/view files remain their current homes. No provider trait, registry, daemon, SSE parser, result history, or persistent jobs.

Config: $XDG_CONFIG_HOME/hn/summary.json or ~/.config/hn/summary.json, with key, automatic, profile. Existing OPENROUTER_API_KEY is usable without copying it into a file. Typed keys are masked and saved with owner-only Unix permissions. M opens settings; Tab moves, Space edits choice, Enter applies, Esc cancels; refresh is an action in the same screen. No extra confirmation.

Budgets additions+deletions excluding preserved browser code: production 2600 lines; tests 700; fixtures/harness 250; docs 200; review reserve 350. Only one extraction dependency. State owners: configuration at Reader; each result/attempt at its story section; browser target at Reader. In-process concurrency limited to 4 section tasks, submitted work retained on navigation, queued work admitted only for active story. Attempt identity is required by the accepted stale Fast/Thorough completion scenario. No other hardening mechanism without a concrete failure.

Behavior matrix: baseline covers pinned layout, source/back, comment position and tree navigation. Extend tests for auto vs feed movement, missing key/setup/cancel, manual mode, stale completion, partial replacement/targeted retry, credential recovery, refresh, source snapshot staging, A/B/A reuse, bounded concurrency, HTTP failures/malformed results, config persistence, local extraction. Keep one local HTTP boundary fixture; never add a production test endpoint flag.

Baseline: cargo test passed 35 library + 27 binary tests. Snapshot /private/tmp/hn-openrouter-baseline. Live credential availability: no OPENROUTER_API_KEY in process or inspected summarizer .env files; user asked for path. Final provider gate remains open.

Review re-entry budget: 5. Evidence belongs to this worktree and exact final dirty snapshot. Independent simplify/review after integration, then PTY acceptance. Native Ghostty automation remains unavailable; do not claim it tested.

Formatted size review: move reader lifecycle methods into existing interaction.rs; reader.rs keeps state definitions. view.rs is an existing renderer (~220 lines), retained as one coherent render operation. Tests remain grouped by behavior with one HTTP boundary file. Test inventory: reader/tests.rs covers keyboard/state/render contracts; network_tests.rs covers external HTTP serialization/parsing and source bounds. Unit state tests intentionally bypass real paid requests; PTY acceptance covers binary wiring. No fixture snapshots or mock-domain tests. Native Ghostty remains a separate access limitation.

Review correction: preparation.rs owns the accepted cancel-before-submission boundary, separated from jobs.rs completion/attempt ownership to keep both readable. A Preparing state carries Tokio AbortHandle; navigation/profile changes stop source preparation, submitted inference survives. Request enum replaces ambiguous bool pair and ensures retry only touches failures. Auto origin on attempts distinguishes manual generation from automatic work when Auto is disabled. This machinery is directly required by the accepted lifecycle contract.

Size checkpoint after review fixes: 1834 additions / 727 deletions versus this turn baseline, including rename comparison. Production remains below 2600; tests 762 changed lines uses 62 of the 350 review reserve (700 initial test budget). Added source/publication contracts justify the increase; no new testing framework. Help now includes M settings and accurate retry wording.

## Acceptance evidence

The direct Rust path is implemented and live-tested. Private Summarizer and Docker remain outside its runtime. Changes are local and uncommitted.

- Final suite: 35 library + 36 binary tests passed; build and format passed. Strict Clippy reports an existing manual_unwrap_or lint in src/ui/comments.rs:68, unchanged from the task baseline. Clippy passes with that one existing lint allowed. Built on installed Rust 1.97; Rust 1.82 and Windows compilation were not exercised.
- Real PTY: live HN, comments expanded/collapsed 43→47→43 at 120×40, setup/cancel, masked key, Auto/Profile persistence, feed o/Enter, existing article target reuse, terminal restoration. At 80×24, comments and settings passed and o used the intended external-browser fallback. Browser split commands were tested with a boundary double; actual Ghostty focus/close remains unverified because automation access was denied.
- Live OpenRouter in the actual hn TUI: HN story 50142573, “Someone behind the software”. Fast article+discussion completed in 8.6s; Thorough upgrade in 8.6s while retaining Fast; citations opened captured source/parent context; return/revisit retained results; Auto off persisted; explicit refresh completed in 11.7s. These single-run times are observations, not a model benchmark. Environment key was not written to settings.
- Earlier real failures were visible and recoverable: an article returned HTTP 451; a Fast discussion produced invalid citations. The final provider schema restricts citation IDs to captured sources and requires 1–3 cited points. Runtime validation remains. OpenRouter HTTP-200 provider-error envelopes now produce a retryable failure.
- Render/state coverage includes 40×12, 80×24, 120×40 and 160×50; comment position stays independent of pinned-summary visibility and source navigation.
- Five isolated source mutations were caught: removed automatic starts, stale completion guard, citation validation, settings persistence, retry-only-failures behavior. Restored baseline passed.
- Codex authorized. Seven delegated review runs: correctness, efficiency, reuse, then four correctness rechecks after fixes. Root was the sole writer.

Evidence: /private/tmp/hn-openrouter-live/*.txt; /private/tmp/hn-openrouter-pty/*.txt; /private/tmp/hn-openrouter-pty-80/*.txt. Re-runnable acceptance harnesses: /private/tmp/hn-openrouter-live.py, /private/tmp/hn-openrouter-pty.py, /private/tmp/hn-openrouter-pty-80.py. Mutation harness: /private/tmp/hn-openrouter-mutations.py (isolated source and build cache). These are local acceptance tools, not production dependencies.

Final task-baseline delta (rename-aware, excluding lockfile and this note): production +1287/-491, tests +565/-239, README +49/-7. Tests use 104 lines of the 350-line review reserve; production remains within 2600. Preserved browser code is included in these counts. Change map: reader config/settings own credentials and defaults; sources/network own local extraction and direct OpenRouter; jobs/preparation own request lifetime; interaction/text/view and reader_actions own user flow.

Run from this worktree after exporting the existing ignored .env.local file: set -a, source .env.local, set +a, then cargo run. The previous bridge runner is unnecessary. API credentials remain in the ignored owner-only local file.
