# hn v0.2.1

`hn` is a terminal reader for Hacker News. This release changes what `h` does, stops a broken summary from reaching the screen, and adds two ways to install.

## Changes

- `h` now folds the selected comment's replies, as in the Mac app, both in the discussion and in a citation's source view. To go back, use Esc, `q` or ←.
- If the model returns a discussion summary whose overview or point is a single word, such as "placeholder", `hn` shows "Discussion summary came back incomplete. M → Enter Retry" instead of displaying it.
- `hn` is now on crates.io as `hn-reader`. The installed command is still `hn`.

## Install

With Homebrew:

```sh
brew install shaance/tap/hn
```

Or download the `tar.gz` and matching `.sha256` for your system from the release assets. Verify the archive with `shasum -a 256 -c <archive>.sha256`, extract it, and put `hn` on your PATH.

To build from source with Rust 1.88 and Cargo:

```sh
cargo install hn-reader --locked
```

## Requirements and optional setup

- macOS binaries target macOS 13 or newer on Apple Silicon and Intel. macOS may need network access the first time it checks the notarization ticket.
- Linux binaries are built on Ubuntu 22.04 for x86-64 and ARM64, with glibc 2.35 and OpenSSL 3. Install the OpenSSL 3 runtime if it is missing.
- Browsing and saving stories work without an API key. Summaries need an OpenRouter or Anthropic API key in the `M` settings form, `OPENROUTER_API_KEY`, or `ANTHROPIC_API_KEY`.
