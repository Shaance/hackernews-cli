//! `hn-desktop` — the native window over `hn_lib`.
//!
//! ```sh
//! cargo run --release                                    # the window
//! cargo run -- --scene feed-wide --snap out.png --tree out.json \
//!     --size 1600x900 --scale 2 --theme dark             # a headless golden
//! ```
//!
//! The window is a `SplitView` shell with the feed tabs in its toolbar, a responsive
//! column-major feed, and the Saved view. Everything the feed knows — the ranked window, the
//! page stepper, the selection — is `hn_lib::feed_state`'s arithmetic; the items come from
//! `hn_lib`'s paging seam. Nothing here decodes HTML or formats a timestamp.

fn main() {
    rune::run(hn_desktop::app::spec());
}
