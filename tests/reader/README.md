# Recorded reader verification

Run `scripts/verify-reader.sh` from a checkout with Rust and [uv](https://docs.astral.sh/uv/) installed. It runs formatting and the Rust tests, then drives the real terminal event loop against a local HTTP server. It needs no provider key and makes no HN or model-provider requests. uv may download pyte on the first run.

The default terminal sizes are **240 columns × 80 rows** and **200 × 60**. Choose your own sizes or output directory:

```sh
scripts/verify-reader.sh --size 220x70 --output /tmp/hn-reader-review
```

Open the printed `index.html` to inspect styled terminal captures. Each size also has plain-text captures and a `result.json` recording the fixture requests, reply counts and exit result. A failed run keeps its screen and request list. These captures are review artifacts, not golden snapshots to approve blindly.

The scenario covers the feed, Saved, article-before-discussion loading, Fast, citation/back, retained output during upgrade, an HTTP 503 for Thorough discussion, retry, reuse on revisit, reply navigation and terminal restoration. Assertions check the displayed content, comment position and request counts. The fake server holds requests until the driver releases them; it does not approximate provider latency with sleeps. Unrecognized requests fail locally.

`nordwand.json` contains a saved public HN discussion and the unedited Fast and Thorough outputs from the accepted prompt comparison on 20 October 2026. It includes source IDs and minimal provenance. Transport envelopes, request IDs, usage and credentials are excluded. The article response is a labelled synthetic control; the discussion summaries are recorded model outputs. This is a UI fixture, not an accuracy score or a claim that its summaries are error-free.

The ignored Rust test in `src/reader/replay.rs` supplies the existing service clients to the same `run_app` used by `hn`. The Python driver owns only the HTTP boundary, keys and captures. Normal `cargo test` skips this PTY-only entrypoint; the verification command runs it explicitly. User settings and keys are isolated. To add a regression, extend the existing journey with an assertion on observable behavior rather than creating a second key dispatcher.

The server and scenario live in `scripts/reader_fixture.py` and `scripts/replay_reader.py`. Adjust the response gate or controlled failure there when investigating timing. Keep the fixture small and label synthetic additions. Keep any changed expectations tied to an intentional product behavior change.

This replay does not verify live provider availability or model quality, external article extraction, or Ghostty's native pane focus and closing. Use `scripts/smoke.py` for the separate paid live-provider check, and Ghostty for pane integration. Smaller sizes can be requested explicitly; they are not the default design target.
