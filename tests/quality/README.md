# Small summary quality review

These are minimal attributed excerpts from failures observed in the 19 October
2026 model benchmark. Bad claims are labeled paraphrases, not exact model quotes.
They are regression examples, not a representative accuracy benchmark.

When changing a prompt/model or source selection:

1. Review `cases.json`. For each proposed output, check numbers, attribution,
   certainty, causal relationships, and support for each cited clause against
   the exact excerpt sent. A whole article containing a fact elsewhere does not
   make an unsupported citation acceptable.
2. Compare real ordinary discussions and a busy thread with the root-first and
   reply-inclusive samples. Check that corrections/replies are included and
   useful breadth remains. Record the story IDs, selected IDs, and result that
   would cause a sampling adjustment.
3. Use actual generated output in the full pinned layout at 120×40 and 160×50;
   check 80×24 as a fallback. Check article prose against the prompt's three
   sentences and 60–90 words. Fast discussion uses 1–3 points; Thorough allows
   additional points when they preserve useful reasons or qualifications. Both
   target at most 30 words per point. Record violations separately from factual
   quality. Retain
   readable comment space and the comment anchor when results arrive or change.
4. Exercise sources/back, model replacement, Retry, and Refresh via
   `scripts/smoke.py` and the existing Rust tests. The script makes paid requests;
   ordinary `cargo test` uses local boundary fixtures and requires no API key.

Do not turn word-count failures into a user-facing rejection or silently cut
prose. Do not add a paid verifier to the normal reading flow. Report concrete
failures instead of a fabricated accuracy score. The short examples identify
specific regressions; a broader factual review still needs the full captured
input for that run.
