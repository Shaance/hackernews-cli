# Contributing to hn

- The terminal app lives at the repository root. The `hn` crate uses Rust 1.88.0 and edition 2021, and exposes the shared `hn_lib` library. [Terminal development guide](README.md#development)
- The macOS app lives in `desktop/`. The `hn-desktop` crate is a separate Cargo workspace using Rust 1.99.1 and edition 2024. It uses Rune, a pinned Git dependency, for GPU rendering and wry to host the article in a native `WKWebView`. [Desktop development guide](desktop/README.md)

## Prerequisites

Install Git, rustup, and a native build toolchain. On macOS, install the Xcode Command Line Tools. The browser preview needs Node.js 22 or newer, and terminal reader verification uses uv. Desktop golden checks need Bash and jq, plus ImageMagick for PNG comparisons on macOS. The [desktop workflow](.github/workflows/desktop.yml) lists the Linux packages for headless rendering with Mesa lavapipe.

Run desktop Cargo commands **from inside `desktop/`**. Rustup selects the toolchain from the current directory. Passing the desktop manifest path from the root still selects Rust 1.88.0.

## Run with temporary settings

In a dedicated shell, set these variables before launching either app. Both apps can browse Hacker News without AI provider keys. The live feed still needs network access.

```sh
scratch=$(mktemp -d /tmp/hn-contrib.XXXXXX)
export XDG_STATE_HOME="$scratch/state" XDG_CONFIG_HOME="$scratch/config" XDG_DATA_HOME="$scratch/data"
unset OPENROUTER_API_KEY ANTHROPIC_API_KEY
```

These paths isolate local state, summary settings, and saved stories. Use a fresh directory for a clean run. Launch directly with Cargo to avoid loading keys from an env file through the terminal development wrapper.

Launch the terminal app from the repository root.

```sh
cargo run --locked
```

On macOS, launch the window from inside `desktop/` using the [desktop run instructions](desktop/README.md#run-it). Use the [live provider check](README.md#live-provider-check) only when you intend to test paid summary requests.

## Checks

### Terminal app

Run from the root. The tests use local fixtures and need no API keys. [Rust workflow](.github/workflows/rust.yml)

```sh
scripts/check-nested-crates.sh
cargo build --locked --verbose
cargo test --locked --verbose
```

After building, verify that terminal settings persist using a fake key and temporary settings.

```sh
uv run --with pyte python scripts/verify-installed.py --binary target/debug/hn
```

### Browser preview

Run the preview CI checks from the repository root. [Preview guide](preview/README.md)

```sh
npm ci --prefix preview
npm run format:check --prefix preview
npm test --prefix preview
npm run build --prefix preview
```

### macOS app

Run from inside `desktop/`. The headless fixtures need no provider keys. For local golden checks, set `HN_GOLDEN_ACTUAL` to a temporary directory outside the checkout so generated images, trees, and traces stay out of the working tree.

```sh
cargo build --locked
cargo test --locked
```

Run the desktop PNG and tree checks on macOS only.

```sh
HN_GOLDEN_ACTUAL=$(mktemp -d) bash scripts/check_goldens.sh
```

The [desktop workflow](.github/workflows/desktop.yml) also builds and tests on Windows. Linux CI uses the same gate with `HN_TREES_ONLY` set to `1` and Mesa lavapipe selected as the Vulkan driver. Linux CI checks the element trees without comparing PNGs.

Run the release-mode tests on macOS to match the [release workflow](.github/workflows/desktop-release.yml).

```sh
cargo test --release --locked
```

Signing, notarization, and publishing `desktop-v*` releases are maintainer-only and require repository secrets.

## Pull requests

- Keep each PR to one theme. Explain what changed, why, and which checks passed or were skipped.
- Add or update tests for behavior changes.
- Update goldens only when the expected output changes on purpose. Include before and after images.
- Follow the commit style in the project history, for example `fix(desktop): preserve keyboard selection`, `feat(desktop): remember window size`, or `docs: clarify setup`.

The [desktop design record](mockups/desktop/DESIGN.md) and [design notes](docs/notes/) explain past decisions. Follow the [code of conduct](CODE_OF_CONDUCT.md) and use the [security policy](SECURITY.md) for private vulnerability reports.
