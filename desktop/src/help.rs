//! The keyboard sheet: `?` or `⌘?`, DESIGN.md's screen 07.
//!
//! The contents are [`crate::keyboard_help::HELP`], which the tests in `keys.rs` check against the routing
//! tables — so the sheet cannot drift from the keys the window actually binds. This module is
//! only the layout: two columns of `Keybind` rows, grouped Feed / Discussion / Summary / Window.
//!
//! The `Dialog` is the modal: Rune traps focus inside it, `Esc` runs its dismiss closure, and
//! the background is inert while it is open.

use rune::tok::{self, type_};
use rune::{Color, El, ScrollSpec, col, row, text};
use rune_kit::{Dialog, Keybind, Kit};

use crate::keyboard_help::{HelpGroup, HELP};

/// The sheet's two columns, as DESIGN.md's mock shows. Which group goes where is
/// [`crate::keyboard_help::HelpGroup::column`]'s, because the split is by height rather than by halving the list.
const COLUMNS: usize = 2;

/// The sheet, in the overlay layer. `open` is the app's flag.
pub fn view(k: &Kit, t: tok::Theme, accent: Color, open: bool, layer: &mut Vec<El>) {
    let mut columns = row().key("help-columns").w_full().gap(28.0).items_start();
    for column in 0..COLUMNS {
        let mut body = col().key(format!("help-column-{column}")).grow(1.0).gap(18.0);
        for group in HELP.iter().filter(|group| group.column == column) {
            body = body.child(section(group, k, t, accent));
        }
        columns = columns.child(body);
    }
    let mut viewport = col()
        .key("help-scroll")
        .w_full()
        .shrink()
        .min_h(0.0)
        .scroll(ScrollSpec { stick_bottom: false })
        .scroll_edge_fade(10.0)
        .child(columns);
    if let Some([_, height]) = rune::memo::layout_size("window") {
        let header = rune::memo::layout_size("help-dialog/header").map_or(50.0, |[_, h]| h);
        // The layer has 16 points on each edge, and the body has a 16-point bottom inset.
        viewport = viewport.max_h((height - 32.0 - header - 16.0).max(0.0));
    }
    Dialog::new("help", "Keyboard")
        .open(open)
        .width(720.0)
        .fit()
        .on_close(|s: &mut crate::app::App| s.help = false)
        .child(col().w_full().px(16.0).pb(16.0).child(viewport))
        .build(k, layer);
}

/// One group: its title, then a row per binding.
fn section(group: &HelpGroup, k: &Kit, t: tok::Theme, accent: Color) -> El {
    let mut body = col()
        .key(format!("help-group-{}", slug(group.title)))
        .w_full()
        .gap(6.0)
        .child(
            text(group.title.to_string())
                .key(format!("help-title-{}", slug(group.title)))
                .font(type_::SECTION)
                .color(accent),
        );
    for row_ in group.rows {
        body = body.child(
            row()
                .key(format!("help-row-{}", slug(row_.label)))
                .w_full()
                .gap(10.0)
                .center_y()
                .child(
                    row()
                        .w(152.0)
                        .gap(3.0)
                        .justify_end()
                        .child(Keybind::new(row_.keys).build(k)),
                )
                .child(
                    text(row_.label.to_string())
                        .font(type_::LABEL)
                        .color(t.text)
                        .truncate(),
                ),
        );
    }
    body
}

/// A stable key segment for a label: lowercase ASCII words joined by dashes.
fn slug(label: &str) -> String {
    label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}
