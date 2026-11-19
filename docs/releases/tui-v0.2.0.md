# hn v0.2.0

`hn` is a terminal reader for Hacker News. This release adds downloadable binaries for macOS and Linux and makes it easier to follow long discussions.

## Highlights

- Reopen folded branches without fetching their replies again. In deep threads, choose “continue thread” to start at that comment. The reader shows the ancestor trail, Backspace steps back up it, and `f` dims comments outside the selected chain.
- Open a summary citation to read the cited comment in context and browse its replies. Replies loaded here also appear in the main discussion.
- Pick up saved article and discussion summaries after restarting `hn`. The app can reuse these summaries without another provider request. Article extraction also works better on GitHub release pages and the Cloudflare blog.
- On terminals 160 columns or wider, the feed shows its stories in two columns; ← and → move between them and turn the page at the edge.
- Move across feed pages with `j` and `k`. Use `g` and `G` to jump to the first and last story on the page, or use Space, Page Down, and Page Up to turn pages. Press `M` to open summary settings from any view.

## Install

Download the `tar.gz` and matching `.sha256` for your system from the release assets. Verify the archive with `shasum -a 256 -c <archive>.sha256`, extract it, and put `hn` on your PATH.

With Homebrew:

```sh
brew install shaance/tap/hn
```

To build from source with Rust 1.88 and Cargo:

```sh
cargo install --git https://github.com/Shaance/hn --tag tui-v0.2.0 --locked hn
```

## Requirements and optional setup

- macOS binaries target macOS 13 or newer on Apple Silicon and Intel. macOS may need network access the first time it checks the notarization ticket.
- Linux binaries are built on Ubuntu 22.04 for x86-64 and ARM64, with glibc 2.35 and OpenSSL 3. Install the OpenSSL 3 runtime if it is missing.
- Browsing and saving stories work without an API key. Summaries need an OpenRouter or Anthropic API key in the `M` settings form, `OPENROUTER_API_KEY`, or `ANTHROPIC_API_KEY`.
