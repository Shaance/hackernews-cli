---
name: hn-tui-manual-validation
description: Validate the HackerNews Rust terminal UI with a live PTY smoke test. Use when changes affect TUI behavior, comment tree navigation, story loading, rendering, keyboard handling, app state, or when iterative simplification needs manual behavior validation beyond cargo tests.
---

# HN TUI Manual Validation

## Purpose

Run a real terminal smoke test for this repository's `hn` TUI. Use it to validate behavior that unit tests cannot fully prove: alternate-screen rendering, live HackerNews loading, keyboard navigation, comment expansion/collapse, visible comment counts, and clean terminal restoration.

## Preconditions

- Run from the repository root.
- Follow the repo instruction to prefix shell commands with `rtk`.
- Treat this as a live-network smoke test: it calls the public HackerNews API.
- Do not press `o` during validation unless explicitly asked; it opens a browser.
- Keep the PTY session open only as long as needed, then quit cleanly.

## Workflow

1. Run deterministic checks first:

   ```bash
   rtk cargo fmt --check
   rtk cargo test
   rtk cargo build
   ```

2. Launch the real TUI in a PTY:

   ```bash
   rtk cargo run
   ```

3. Wait until the stories view renders actual stories, not just `Loading stories...`.

4. Enter comments for the selected story:

   - Send `c`.
   - Wait until comments render.
   - Record the visible comment count shown in the comments title.

5. Validate the comment tree path behavior:

   - Send `Enter` to expand the selected top-level comment.
   - Wait for replies to load.
   - Confirm the selected row changes from collapsed (`>`/collapsed indicator) to expanded (`v`/collapse indicator) and the visible comment count increases.
   - Send `j` to move into a child comment.
   - Send `]` to jump to the next sibling thread.
   - Send `u` to jump back to the parent comment.
   - Send `c` to collapse the current thread.
   - Confirm the visible comment count returns to the pre-expand count and the child rows disappear.

6. Optionally validate view restoration:

   - Send `q` or `Esc` from comments to return to stories.
   - Send `q` from stories to quit.
   - Confirm the process exits with status 0 and the terminal leaves alternate-screen mode.

## Pass Criteria

Report pass only when all are true:

- Stories load and render.
- Comments load and render.
- Expanding a thread loads child rows and increases the visible count.
- `j`, `]`, `u`, and `c` operate on the expected visible comment paths.
- Collapsing removes child rows and restores the prior visible count.
- The app quits cleanly.

## Failure Handling

- If live HN data has no comments or no child replies, choose another story or top-level comment.
- If the network/API fails, report the smoke test as blocked by live dependency and keep deterministic checks separate.
- If the PTY session hangs, send `q`, then `Ctrl-C` only if normal quit does not work.
- Include observed counts, key sequence, command results, and any visible anomaly in the final report.
