# Handoff: hn terminal app 0.2.0 release and README media (2026-11-19)

Written before this Mac is erased. Everything below lives on GitHub (`Shaance/hn`); nothing depends on the old Mac.

## Standing preferences (from the owner)

- Work through PRs and merge them; `main` is protected (PR + `build`/`resolve` checks; admins can bypass).
- Codex implementation: Sol medium (`gpt-6.2-sol`, effort medium) unless the owner names another model; read-only reviews have used Terra high and Astra high. Verify model/effort from the Codex session record.
- Desktop parity is the default for terminal-app (TUI) behaviour ("do it like desktop").
- Never read or print `~/.config/hn/summary.json` or secret values. Signing material lives in `~/.config/hn-signing` on the owner's machine; secrets are pushed with `gh secret set` via stdin.
- Ask before outward actions the owner hasn't approved. Approved on 2026-11-19: create `Shaance/homebrew-tap`, run the release dry run, push `tui-v0.2.0`, publish.

## State at handoff

See the status table at the end (updated last).

## Remaining steps, in order

1. **Feed PR #63** (two-column feed at >=160 cols, padding row, trail row + Backspace parity): merge when CI is green.
2. **Release PR #64** (`.github/workflows/tui-release.yml`, version 0.2.0 + both lockfiles, `packaging/homebrew/hn.rb`, `docs/releases/tui-v0.2.0.md`): merge when CI is green.
3. **README media PR #62** (`docs-tui-demo`): merge `origin/main` into it, keep `scripts/readme/dark.tape` at `Set Columns 172` so `feed.png` shows two columns, run `sh scripts/readme-screenshots.sh` (needs `vhs`, `ffmpeg`, an `OPENROUTER_API_KEY` in env or `.env.local`; one live summary). Inspect `docs/screenshots/{demo.gif,feed.png,reader.png,citation.png,help.png,reader-light.png}`: the GIF must show the citation's replies and a thread folding then reopening. Commit, push, merge.
4. **Dry run**: `gh workflow run tui-release.yml --ref main`, then `gh run watch`. Expect 4 build artifacts, both macOS notarizations `Accepted`, publish job skipped. Download (`gh run download <id>`), `shasum -a 256 -c`, run `hn --version` from the extracted arm64 archive; read the Linux jobs' `ldd` output (OpenSSL 3, glibc 2.35).
5. **Tag**: `git tag -a tui-v0.2.0 -m "hn 0.2.0" origin/main && git push origin tui-v0.2.0`. Watch the run; confirm 8 assets on the release and `gh api repos/Shaance/hn/releases/latest --jq .tag_name` still says `desktop-v1.1.0`.
6. **Homebrew tap**: `gh repo create Shaance/homebrew-tap --public --description "Homebrew formulae for hn"`; copy `packaging/homebrew/hn.rb` to `Formula/hn.rb`, replace the four `FILL_ME_*` with the hashes from the release's `.sha256` assets, commit, push. Test: `brew install shaance/tap/hn && brew test shaance/tap/hn && hn --version`.
7. **README install section** (small PR): the TUI "Get it" cell (README line ~9) and "Run it" (~line 39): `brew install shaance/tap/hn`; release download link `https://github.com/Shaance/hn/releases/tag/tui-v0.2.0`; source: `cargo install --git https://github.com/Shaance/hn --tag tui-v0.2.0 --locked hn` (the package name `hn` is required; the repo has five binary packages). Keep the from-checkout instructions.

## Decisions already made (don't relitigate)

- Feed page size stays 20 at every width (page cache keyed by page number); two columns are layout only.
- Linux builds are glibc on native Ubuntu 22.04 runners (reqwest native-tls needs OpenSSL); no musl.
- macOS CLI is signed + notarized, not stapled (Apple doesn't support stapling a bare binary).
- TUI releases use `--latest=false`; desktop keeps Latest.
- Homebrew formula was updated by hand for 0.2.0. From the next release the `homebrew` job in `tui-release.yml` (#70) renders `packaging/homebrew/hn.rb` with `packaging/homebrew/render-formula.sh` and pushes it to the tap with the `TAP_TOKEN` secret (fine-grained, `Shaance/homebrew-tap` only, Contents read/write, expires 2027-11-18). It first ran for 0.2.1 and pushed `hn 0.2.1` to the tap as github-actions[bot].

## Known follow-ups (not started)

- crates.io: the crate is `hn-reader` (#73; `hn` was taken), first published as 0.2.1 by the `crates-io` job with a stored token. Since #77 the job uses trusted publishing: crates.io trusts `Shaance/hn` / `tui-release.yml` / environment `crates-io`, and that GitHub environment only deploys from `tui-v*` tags. The `CARGO_REGISTRY_TOKEN` secret is deleted and the token revoked. hn 0.2.2 (#78 week ages, #79 release, run 44643723513) was the first trusted-publishing release: every job passed on the first attempt and crates.io records the publish against that run. If a later run fails at the auth step, check those three values match. Optional: turn on "Require trusted publishing for all new versions" in the crate settings.
- `macos-15` runners often go unassigned: jobs are cancelled after 15 min with zero steps. Rerun only the failed jobs (`gh run rerun <id> --failed`); 0.2.1 needed three attempts.
- cargo-binstall metadata is in `Cargo.toml` (#69); URLs and archive layout checked, but no real `cargo binstall` run yet, so the README doesn't mention it.
- Intel macOS binary is only cross-built; first native execution check happens on an Intel Mac or Rosetta.
- A discussion summary reading only "placeholder [1]" was seen twice (audit N8). #68 rejects single-word overviews/points with a retry prompt; the cause is unconfirmed. The README script now keeps its provider replies in `~/.local/state/hn/summary-responses/`.

## Status table

All steps done on 2026-11-19:

| Item | State |
|---|---|
| #62 README media, #63 feed, #64 release setup, #65 signing fix, #66 README install | merged |
| Release `tui-v0.2.0` | published, 8 assets, `--latest=false` (desktop-v1.1.0 stays Latest) |
| #67 fmt drift, #68 stub summaries, #69 binstall metadata, #70 tap automation | merged |
| #73 crate `hn-reader`, #74 replay uses ← after #71, #76 release 0.2.1 | merged |
| Release `tui-v0.2.1` | published, 8 assets, desktop-v1.1.0 stays Latest; tap updated by the workflow; on crates.io as `hn-reader` 0.2.1; `brew install` (Notarized Developer ID) and `cargo install hn-reader --locked` both give `hn 0.2.1` |
| Tap | `Shaance/homebrew-tap` `Formula/hn.rb`; `brew install shaance/tap/hn`, `brew test`, `brew audit --strict` pass |

Next release: bump `Cargo.toml` + both lockfiles, write `docs/releases/tui-vX.Y.Z.md`, merge, push the tag, then check the run: the `homebrew` job pushes `Formula/hn.rb` to the tap and the `crates-io` job publishes `hn-reader`. Rerun cancelled macOS builds with `gh run rerun <id> --failed`.

## Terminal keys and the browser trial (2026-11-19 to 2026-11-20)

| Item | State |
|---|---|
| #71 `h` folds the selected comment in the terminal discussion and citation views (Mac parity); `Esc`/`q`/`←` go back | merged |
| #72 README media re-rendered with `h` as fold | merged |
| #75 `.github/workflows/preview-ui.yml`: deploys the trial's static page (hn.hashcode.dev, Vercel `hn-preview`) when a page file changes on `main` | merged; first run deployed `dfab4b9` on 2026-11-20 and passed its live-page check |
| Gateway, visitor image and `/opt/hn-preview/hn` at `db03844` | released by hand: development 2026-11-19, production 2026-11-20; record and runbook in `preview/deploy/README.md` (#80) |

Decisions (owner-approved, don't relitigate):
- Terminal `o` stays "open the article pane". The Mac app's `o` (same as Enter) differs on purpose: the terminal has no always-on article split.
- Page deploys: GitHub Actions with a Vercel token in the `production` environment (protected branches only), not Vercel's Git integration (the build writes its CSP `vercel.json` inside `dist/`). The token "hn GitHub Actions (preview UI deploy)" is scoped to the `hn-preview` project and expires 2027-11-20; replace the `VERCEL_TOKEN` secret before then.
- Server releases stay manual, never pushed from CI (no SSH keys in GitHub), and there is no release script: follow "Manual server release" in `preview/deploy/README.md`, development first, production only after the owner says go. The `sudo -n -u hnpreview` steps are an owner-approved exception to the TLDW no-sudo routine-deploy rule. `tldw-infra` provisions the development host only.
- Release order: snapshot readers (visitor image + host `hn`) on both hosts before the writer; page↔gateway message change → release gateway first, then merge the page PR (it auto-deploys).

Trial state on 2026-11-20: both gateways run image `sha256:1f64317d…` and `hn 0.2.0` (`db03844`); a real public session showed the new `h/c Fold` help, gVisor, no network, read-only, 256 MB. The 03:00 UTC publish on 2026-11-20 already ran on the new `hn` (55 stories, three summaries) and both hosts adopted snapshot `6ff3cfa2…`. Rollback: restore `/opt/hn-preview/hn.previous-20261119` and `gateway.env.previous-20261119` on the host and restart `hn-preview.service`; previous images stay loaded. `main` has since gained 0.2.1/0.2.2; the only trial-visible change is "1w" ages (`src/time_utils.rs`), so it waits for the next release. Not cleaned up on purpose: development's rootful BuildKit cache (about 3 GB, shared with TLDW builds) and older rootless `hn-preview` tags on production.
