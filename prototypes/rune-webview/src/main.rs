//! rune-webview — spike: host a wry/WKWebView child view inside a Rune window on macOS.
//!
//! The Rune side is a normal `App`: a header row with a label and a "w" toggle, and a child
//! region whose laid-out rect the webview is pinned to. The webview is a plain wry child of the
//! window's content view, created from the raw handle Rune hands out in `App::on_window`.
//!
//! ```sh
//! cargo run --release                                   # local marker HTML
//! cargo run --release -- https://news.ycombinator.com/item?id=1   # "u" loads this
//! cargo run --release -- --snap out.png --tree tree.json --size 960x680 --scale 2
//! ```
//!
//! Headless (`--snap` / `--tree`) renders the Rune chrome only: a child webview needs a real
//! NSWindow, so it is absent there by construction.

mod app;

#[cfg(target_os = "macos")]
mod webview;

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();

    // The first bare argument that looks like a URL is the page "u" loads.
    let remote_url = argv
        .iter()
        .find(|a| a.starts_with("http://") || a.starts_with("https://"))
        .cloned();

    #[cfg(not(target_os = "macos"))]
    {
        if !rune::tooling::Args::parse_from(argv.clone()).headless() {
            println!("webview spike is macOS-only");
            return;
        }
        // Headless still renders the Rune-only parts, so `--snap` / `--tree` work anywhere.
        app::run(argv, None);
    }

    #[cfg(target_os = "macos")]
    app::run(argv, remote_url);
}
