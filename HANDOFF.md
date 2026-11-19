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
- Homebrew formula was updated by hand for 0.2.0. From the next release the `homebrew` job in `tui-release.yml` (#70) renders `packaging/homebrew/hn.rb` with `packaging/homebrew/render-formula.sh` and pushes it to the tap with the `TAP_TOKEN` secret (fine-grained, `Shaance/homebrew-tap` only, Contents read/write, expires 2027-11-18). Its first real push is the next tag; if it fails, the release is already out and rerunning that job is enough.

## Known follow-ups (not started)

- A crates.io name (crate `hn` is taken).
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
| Tap | `Shaance/homebrew-tap` `Formula/hn.rb`; `brew install shaance/tap/hn`, `brew test`, `brew audit --strict` pass |

Next release: bump `Cargo.toml` + both lockfiles, write `docs/releases/tui-vX.Y.Z.md`, merge, push the tag, then check that the `homebrew` job pushed `Formula/hn.rb` to the tap (`brew update && brew upgrade hn`).
