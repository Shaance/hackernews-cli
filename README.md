# HackerNews TUI

Read [Hacker News](https://news.ycombinator.com/) stories and comment threads in your terminal. Save stories for later, keep your place in a discussion, and optionally generate article and discussion summaries through OpenRouter.

On macOS with Ghostty, you can open the original article in a companion browser pane beside the discussion. Other setups open it in your default browser.

## Run it

Build from this checkout with Rust and Cargo:

```sh
cargo install --path .
hn
```

Or run without installing:

```sh
cargo run --release
```

Browsing and saving stories work without an API key. Summaries and the Ghostty article pane each have optional setup below.

## Read a story

Use `j/k` or the arrow keys to select a story. Press `1`, `2`, or `3` for Top, New, or Best; `4` opens Saved. Use `n/p` to change pages.

Press `o` or Enter to open the discussion and its article. Press `c` to read the discussion without opening a new article pane. If a companion pane is already open, it follows the selected story. `O` always opens the story in your default browser.

In a discussion, `j/k` moves between comments. Enter expands or collapses replies, `[/]` skips between sibling threads, and `u` returns to the parent. Page Up/Down scrolls through long comments without changing the selection. Original post text appears above the replies, with paragraphs and indented code preserved. Press `L` to choose a link from the post or selected comment.

Return to the feed with Esc. Reopening the discussion during the same run restores loaded replies, selection, and scroll position. Press `?` for help.

### Save for later

Press `s` in the feed or discussion to save or unsave the story. Saved stories survive restarts and need no account. They store the story details as they were when saved; article content, loaded replies, and reading position are kept separately for the current run.

If the saved file cannot be read, hn preserves it and displays a notice. After correcting the file or its permissions, open Saved with `4` and press `r` to reload it.

## Optional AI summaries

Open a story, press `M`, enter an OpenRouter API key, and press Enter. You can also supply the key through `OPENROUTER_API_KEY`. A key saved in settings takes precedence over the environment variable. Environment keys are not copied into the settings file unless you replace them through the form.

Summaries send extracted article text and sampled comment excerpts to OpenRouter using your credits. The app calls OpenRouter directly; no separate summary service is required.

Automatic summaries are on by default once a key is available. They start when you open a story, while you continue reading. In `M`, use Tab or the arrow keys to select a setting, Space or left/right to change it, Enter to apply, and Esc to cancel. Turn Automatic summaries off to generate them only when requested with `S`. Requests already submitted can still finish.

### Fast and Thorough

| Profile | Configured model | Discussion summary |
|---|---|---|
| Fast (default) | Claude Haiku 5.5, low reasoning | Up to three main insights, including disagreements when they affect the takeaway. |
| Thorough | Claude Sonnet 5.5, low reasoning | Broader coverage, with extra points when they preserve a useful reason or qualification. |

Both profiles also generate a compact article summary. Thorough works independently from the captured sources; it does not take the Fast summary as a draft. The model IDs are `anthropic/claude-haiku-5.5` and `anthropic/claude-sonnet-5.5`.

### Read and check a summary

The summary stays pinned above the comments. `S` hides or shows it without moving your comment position. When it is too long to fit, `J/K` scrolls the summary while `j/k` continues to navigate comments.

Discussion points include numbered citations. Press Tab or Shift-Tab to select one, then Enter to read the cited comment and parent context. The source viewer identifies the excerpt sent to the model and labels additional captured text below it. Esc returns to your discussion position.

Summaries show their capture time and source coverage. Article extraction can be partial, and discussion summaries use a sample of comments that includes replies. A citation links to captured evidence; it does not guarantee that the model interpreted it correctly.

### Revisit, change model, or refresh

Completed summaries and their captured inputs stay in memory for the current run. Returning to a story reuses its results without another model request, even if you changed the default profile elsewhere.

Applying a model choice in `M` uses that story's retained input and sets the default for future summaries. Only the latest result is retained, so explicitly switching back to a previous model can require another paid request. Saving unrelated settings does not replace an existing summary.

Choose **Refresh sources & summary** in `M` to fetch newer article and discussion content and generate new results. Press `r` to retry failed work using any successful capture already available. Existing summaries stay readable during replacement or failure, and a citation you are viewing keeps its captured text until you leave it.

Restarting hn begins a new reading session. Saved stories and summary settings persist; captured sources, summaries, and comment positions do not reload from disk.

## Optional Ghostty article pane

The companion pane currently supports macOS with Ghostty and terminal-browser. The hn terminal must be at least 110 columns wide when opening a new pane. Follow the [Ghostty setup instructions](scripts/ghostty/README.md) for the supported terminal-browser v0.13.4 setup, including the pane-specific Cmd+W binding and startup/exit patch.

Press `o` to open the article beside the discussion. For the same story, `o` focuses the existing browser tab and preserves its scroll position and any page you followed within that tab. Opening a different story replaces hn's article tab in the existing companion. Unrelated browser tabs and terminal panes are left alone.

With the documented setup, Cmd+W in the companion closes that pane without the terminal confirmation or a “Process exited” screen. Press `o` to reopen it. The setup requires a one-time Ghostty configuration edit; running hn does not change global configuration or other panes' close behavior.

Other terminals, narrower initial windows, or a missing terminal-browser executable use the default browser. If the companion fails to launch, `O` opens the story externally.

## Keyboard reference

Keys are case-sensitive: `s` saves a story, `S` controls its summary; `o` opens the companion when available, `O` always opens externally.

### Feed

| Key | Action |
|---|---|
| `j` / `↓` | Next story |
| `k` / `↑` | Previous story |
| `n` / `→` | Next page |
| `p` / `←` | Previous page |
| `1` / `2` / `3` | Top / New / Best stories |
| `4` | Saved stories |
| `s` | Save or unsave selected story |
| `Enter` / `o` | Open discussion and article |
| `O` | Open story in default browser |
| `c` | Open discussion |
| `r` | Refresh current page; reload saved file in Saved |
| `?` | Toggle help |
| `q` | Quit |

### Discussion and summary

| Key | Action |
|---|---|
| `j` / `↓`, `k` / `↑` | Next / previous comment |
| `]` / `[` | Next / previous sibling thread |
| `u` | Parent comment |
| `g` / `G` | Top / bottom |
| `Page Up` / `Page Down` | Previous / next screen of text |
| `Enter` / `l` / `→` | Expand or collapse replies |
| `c` | Collapse current thread |
| `o` | Open or focus article companion |
| `O` | Open story in default browser |
| `S` | Set up, generate, or show/hide summary |
| `M` | Summary key, automatic mode, profile, and refresh |
| `Tab` / `Shift-Tab` | Select next / previous summary citation |
| `Enter` on a citation | Read captured comment and parent context |
| `J` / `K` | Scroll an overflowing summary |
| `L` | Choose a link from the post or selected comment |
| `s` | Save or unsave current story |
| `r` | Retry failed summary or comment loading |
| `Esc` / `q` / `h` / `←` | Back from a source to the discussion, or from the discussion to the feed |
| `?` | Toggle help |

While viewing a cited source, `j/k`, Page Up/Down, and `g/G` scroll its text. In the link picker, use `j/k` to select, Enter to open, and Esc to close.

## Local files

| Data | Default location | Lifetime |
|---|---|---|
| Saved story details | `~/.local/share/hn/saved.json` | Until unsaved or deleted |
| Summary settings and any key entered in the form | `~/.config/hn/summary.json` | Across restarts |
| OpenRouter response diagnostics | `~/.local/state/hn/openrouter/` | Until manually deleted |
| Captured inputs, summaries, replies, and reading positions | Memory only | Current run |

`XDG_DATA_HOME`, `XDG_CONFIG_HOME`, and `XDG_STATE_HOME` override the corresponding directory roots. Settings and diagnostic files use owner-only permissions on Unix. A key entered through the form is stored in the settings file; it is not an OS keychain entry.

Each diagnostic JSON file records the timestamp, model, story title, HTTP status, generation ID, request ID when supplied, and raw response body. It can contain summary or error text. Request headers and full prompts/article/comment inputs are not logged, and an echoed active key is redacted. Logging failures do not interrupt reading or trigger another request.

Diagnostics are for investigation and are not used as a summary cache. Deleting the diagnostic directory is safe.

## Development

From a checkout, run:

```sh
./scripts/dev.sh
```

The runner builds and starts hn. It optionally reads an ignored `.env.local` containing `OPENROUTER_API_KEY=value`, with optional quotes, and adds `.tools/terminal-browser/bin` to PATH when present. An existing environment key takes precedence over the env file. The file is read as data, never executed. Pass another env-file path as the first argument if needed.

Run the deterministic checks:

```sh
cargo fmt --check
cargo test
cargo build
```

With an OpenRouter key available and `uv` installed, run the live terminal smoke test:

```sh
uv run --with pyte python scripts/smoke.py
```

This command calls live Hacker News and OpenRouter and makes paid model requests. It uses temporary settings, saved items, and diagnostic logs, and prints the paths to those files and its terminal captures. It checks Fast/Thorough, summary toggling without moving comments, retained results, citations, refresh, reply navigation, and clean exit.

Native Ghostty focus and pane closure need the manual checks in the [companion setup guide](scripts/ghostty/README.md). Summary accuracy needs the source-grounded review in [tests/quality/README.md](tests/quality/README.md); a successful smoke test does not establish factual accuracy.

### Code layout

The app uses ratatui and crossterm for terminal rendering and input, Tokio for asynchronous work, reqwest for HTTP, and Readability for article extraction.

| Area | Responsibility |
|---|---|
| `src/main.rs`, `src/reader_actions.rs` | Event loop and keyboard coordination |
| `src/app.rs`, `src/app/` | Feed, comment tree, and retained discussion position |
| `src/bookmarks.rs` | Saved-story persistence |
| `src/hn_client.rs`, `src/hn_cli_service.rs` | Hacker News requests and conversion to displayable content |
| `src/reader.rs`, `src/reader/` | Captured sources, summary jobs, settings, source viewer, and article companion |
| `src/reader/summary_prompts.rs` | Article, Fast discussion, and Thorough discussion instructions |
| `src/ui/` | Feed and comment rendering |
| `src/reader/tests/` | Reader navigation, summary lifecycle, and settings tests |

## Screenshot

<img width="835" height="1054" alt="HackerNews TUI" src="https://github.com/user-attachments/assets/66b94295-9673-4af6-ac1f-b5fb704e0462" />

## About

This project was created as a learning exercise to explore Rust and terminal UI development.
