# HackerNews TUI

## Description

An interactive terminal user interface (TUI) for browsing [HackerNews](https://news.ycombinator.com/). Built with Rust to provide a fast, keyboard-driven experience for reading HN stories and comments.

## Screenshots

<img width="835" height="1054" alt="image" src="https://github.com/user-attachments/assets/66b94295-9673-4af6-ac1f-b5fb704e0462" />


## Installation

### From Source

```bash
cargo install --path .
```

Then run with:
```bash
hn
```

### Running Directly

```bash
cargo run --release
```

## Features

### Stories View
- **Browse Stories**: View top, new, or best stories from HackerNews
- **Keyboard Navigation**: Navigate with vim-style keys (j/k) or arrow keys
- **Pagination**: Move between pages of stories
- **Quick Story Type Switching**: Press 1/2/3 to switch between Top/New/Best
- **Read a Story**: Press `o` or Enter for the story workspace and article pane; `O` opens your default browser
- **View Comments**: Press 'c' to view story comments
- **Saved Stories**: Press `s` to save or unsave a story; `4` opens Saved

### Comments View
- **Nested Comments**: View comment threads with unlimited nesting depth
- **Lazy Loading**: Comments and replies are loaded on-demand for faster initial load
- **Expand/Collapse**: Press Enter or `l` (or →) to expand or collapse comment replies
- **Smooth Navigation**: Navigate through comments with j/k keys; collapse a whole thread with `c`
- **Jump to Top/Bottom**: Press 'g' for top, 'G' for bottom
- **Skip Threads**: Jump to the next sibling thread with `]` or previous sibling with `[`
- **Climb Threads**: Jump to the parent comment with `u`
- **Thread Guides**: Tree lines with depth-based colors to follow nested conversations at a glance
- **Read Long Comments**: Page Up/Down scroll through text without changing the selected comment
- **Original Posts**: Read self-post text above the replies, including paragraphs and indented code
- **Web Links**: `L` lists links from the original post and selected comment; Enter opens a link
- **Keep Your Place**: Reopening a discussion in the same session restores its loaded replies, selection, and scroll position
- **Retry**: `r` retries failed summary sections and failed comment loading; interrupted reply loads can be expanded again

Saved stories are local snapshots and survive restarts. They are stored in
`$XDG_DATA_HOME/hn/saved.json`, or `~/.local/share/hn/saved.json` when
`XDG_DATA_HOME` is unset. No account or setup is required. Thread positions are
kept for the current session. Saved story metadata reflects when it was saved.
If the saved file cannot be read, the app preserves it and displays a notice;
open Saved with `4` and press `r` after correcting the file or its permissions.

### General
- **Gentle Loading**: Cached pages show instantly, lists stay visible/dim while updates stream in
- **Asynchronous Loading**: Non-blocking API requests with loading indicators
- **Error Handling**: Graceful error messages with retry options
- **Help System**: Press '?' for keyboard shortcuts
- **Clean UI**: Inherits your terminal colors for a native look

## Keyboard Shortcuts

### Stories View

| Key | Action |
|-----|--------|
| `j` / `↓` | Next story |
| `k` / `↑` | Previous story |
| `n` / `→` | Next page |
| `p` / `←` | Previous page |
| `1` | View Top stories |
| `2` | View New stories |
| `3` | View Best stories |
| `4` | View Saved stories |
| `s` | Save/unsave selected story |
| `Enter` / `o` | Read story with companion article pane |
| `O` | Open story in default browser |
| `c` | View comments |
| `r` | Refresh current page |
| `?` | Toggle help |
| `q` | Quit |

### Comments View

| Key | Action |
|-----|--------|
| `j` / `↓` | Next comment |
| `k` / `↑` | Previous comment |
| `]` | Next sibling thread |
| `[` | Previous sibling thread |
| `u` | Jump to parent |
| `g` | Go to top |
| `G` | Go to bottom |
| `Page Up` / `Page Down` | Read previous/next screen of text |
| `Enter` / `l` / `→` | Expand/collapse replies |
| `c` | Collapse current thread |
| `o` | Open/focus companion article pane |
| `O` | Open story in default browser |
| `S` | Set up, generate, or show/hide summary |
| `M` | Summary settings: key, Auto, Fast/Thorough, refresh |
| `Tab` / `Shift-Tab` | Select summary source |
| `Enter` on a source | Read cited comment and parent context |
| `J` / `K` | Scroll summary when it exceeds available space |
| `L` | Choose a web link from the post or selected comment |
| `s` | Save/unsave current story |
| `r` | Retry failed summary sections / failed comments |
| `Esc` / `q` / `h` / `←` | Back to stories |
| `?` | Toggle help |

## Optional AI summaries

Run `hn`, open a story, then press `M`, and enter an OpenRouter key. Alternatively, export
`OPENROUTER_API_KEY`. Environment keys are not saved in summary settings.
Settings are stored in `$XDG_CONFIG_HOME/hn/summary.json` or `~/.config/hn/summary.json`.
Keys entered through settings use owner-only file permissions on Unix. Requests
use your OpenRouter credits.

Automatic summaries are on by default. Fast uses Claude Haiku 5.5 with low
reasoning and prioritizes the main discussion insights in up to three points.
Thorough uses Claude Sonnet 5.5 with low reasoning and allows broader coverage
when another useful reason or qualification warrants it. Both work independently
from the captured sources; Thorough does not use the Fast summary as input.

The full summary stays pinned above comments. `S` shows/hides it without moving
comments. `J/K` scroll an overflowing summary; `j/k` continue through comments.
Tab and Enter inspect a cited comment, and Esc returns to your reading position.
The source viewer marks the exact excerpt sent to the model; additional captured
text remains directly below it.

Captured inputs and summaries live in memory for this run. Returning to a story
keeps its completed results, even after changing the default model elsewhere.
Applying a model choice in `M` summarizes that story's retained input and sets the
default for new summaries. Saving unrelated settings does not replace results.
Only the latest result is retained, so explicitly switching back to a previous
model can require another paid request. Restarting hn also starts a new session.

“Refresh sources & summary” in `M` fetches newer article and discussion content.
`r` retries failed work, reusing successful capture. Existing results stay readable
during replacement or failure. Captured citation text remains stable during a
refresh. Summaries use a bounded sample of comments, including replies; source
coverage is shown beside each result.

OpenRouter response diagnostics are saved locally to `$XDG_STATE_HOME/hn/openrouter/`,
or `~/.local/state/hn/openrouter/` when unset. Each JSON file contains the timestamp,
model, story title, HTTP status, generation ID, request ID when supplied, and raw
response body. Request headers and full prompts/article/comment inputs are not logged; an echoed active API key
is redacted. Files are owner-only on Unix. Logging failures do not interrupt reading
or trigger another request. These files can contain summary/error text and remain
until you delete them; deleting the directory is safe.

### Local development

From a checkout, run:

```sh
./scripts/dev.sh
```

The runner builds/starts hn, optionally loads `OPENROUTER_API_KEY` from the ignored
`.env.local`, and uses `.tools/terminal-browser/bin` if present. No private
Summarizer project, bridge, or daemon is required. The env file supports a single
`OPENROUTER_API_KEY=value` line, optionally quoted; it is read as data, never
executed. An existing environment key takes precedence. Without a key, configure
summaries through `M` or read without them.

### Optional Ghostty companion

On macOS in Ghostty at 110 columns or wider, `o` opens a companion pane using an
installed terminal-browser. Same-story `o` focuses its retained tab; a different
story replaces the owned article tab. Other terminals, narrower initial windows,
or a missing executable use your default browser. `O` always opens externally.

The prototype's pane-only Cmd+W behavior needs the versioned setup in
[scripts/ghostty/README.md](scripts/ghostty/README.md). The app never changes global
Ghostty configuration or adopts unrelated panes. Native Ghostty verification is
separate from the socket/PTY tests.

### Validation

```sh
cargo fmt --check
cargo test
cargo build
uv run --with pyte python scripts/smoke.py
```

The last command uses live HackerNews and OpenRouter, makes paid model requests,
and writes terminal captures to a temporary directory. It uses isolated settings
and tests Fast/Thorough, retained results, citations, Refresh, reply navigation,
and clean exit. It does not validate native Ghostty rendering or pane closure.
See [tests/quality/README.md](tests/quality/README.md) for the small claim-support
and rendering review used when changing prompts or models.

## Architecture

Built with:
- **[ratatui](https://github.com/ratatui-org/ratatui)**: Modern terminal UI framework
- **[crossterm](https://github.com/crossterm-rs/crossterm)**: Cross-platform terminal manipulation
- **[tokio](https://tokio.rs/)**: Async runtime for non-blocking API requests
- **[reqwest](https://github.com/seanmonstar/reqwest)**: HTTP client for HackerNews API

## Development

### Build

```bash
cargo build
```

### Run Tests

```bash
cargo test
```

### Build for Release

```bash
cargo build --release
```

## Project Structure

```
src/
├── main.rs          # Entry point, event loop
├── lib.rs           # Core service and data types
├── app.rs           # Application state management
├── event.rs         # Keyboard event handling
├── hn_client.rs     # HackerNews API client
├── time_utils.rs    # Time formatting utilities
└── ui/
    ├── mod.rs       # UI module exports
    ├── stories.rs   # Stories list rendering
    ├── comments.rs  # Comments tree rendering
    └── widgets.rs   # Reusable UI components
```

## License

This project was created as a learning exercise to explore Rust and terminal UI development.
