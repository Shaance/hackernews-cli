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
- **Open in Browser**: Press 'o' or Enter to open story URL in your default browser
- **View Comments**: Press 'c' to view story comments

### Comments View
- **Nested Comments**: View comment threads with unlimited nesting depth
- **Lazy Loading**: Comments and replies are loaded on-demand for faster initial load
- **Expand/Collapse**: Press Enter or `l` (or →) to expand or collapse comment replies
- **Smooth Navigation**: Navigate through comments with j/k keys; collapse a whole thread with `c`
- **Jump to Top/Bottom**: Press 'g' for top, 'G' for bottom
- **Thread Guides**: Tree lines with depth-based colors to follow nested conversations at a glance

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
| `Enter` / `o` | Open story URL in browser |
| `c` | View comments |
| `r` | Refresh current page |
| `?` | Toggle help |
| `q` | Quit |

### Comments View

| Key | Action |
|-----|--------|
| `j` / `↓` | Next comment |
| `k` / `↑` | Previous comment |
| `g` | Go to top |
| `G` | Go to bottom |
| `Enter` / `l` / `→` | Expand/collapse replies |
| `c` | Collapse current thread |
| `o` | Open story URL in browser |
| `Esc` / `q` / `h` / `←` | Back to stories |
| `?` | Toggle help |

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
