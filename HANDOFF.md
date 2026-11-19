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
- Homebrew formula is updated by hand for 0.2.0; tap automation is a later idea.

## Known follow-ups (not started)

- Tap automation (fine-grained token scoped to the tap), cargo-binstall metadata, a crates.io name (crate `hn` is taken).
- Intel macOS binary is only cross-built; first native execution check happens on an Intel Mac or Rosetta.
- `scripts/verify-reader.sh` fails at `cargo fmt --check` on pre-existing drift.

## Status table

| Item | State |
|---|---|
| #63 feed | merged (desktop Linux CI job failed on an Ubuntu mirror outage, not required) |
| #64 release setup | merged |
| #62 README media | merged (re-rendered; one render got a bad model summary "placeholder [1]", re-rendered) |
| Dry run | running: https://github.com/Shaance/hn/actions/runs/44524383795 |
| Tag / tap / README install | not started (steps 5–7) |
