# hn v0.2.2

`hn` is a terminal reader for Hacker News. This release fixes how the age of older stories and comments is shown.

## Changes

- Anything a week or older now shows its age as `1w`, `2w` and so on, matching `5m`, `3h` and `6d`. It used to read `1 weeks ago`.

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
