//! The keyboard sheet: `?` or `⌘?`, DESIGN.md's screen 07.
//!
//! The contents are [`keys::HELP`], which the tests in `keys.rs` check against the routing
//! tables — so the sheet cannot drift from the keys the window actually binds. This module is
//! only the layout: two columns of `Keybind` rows, grouped Feed / Discussion / Summary / Window.
//!
//! The `Dialog` is the modal: Rune traps focus inside it, `Esc` runs its dismiss closure, and
//! the background is inert while it is open.

use rune::tok::{self, type_};
use rune::{Color, El, col, row, text};
use rune_kit::{Dialog, Keybind, Kit};

use crate::keys::{self, HelpGroup};

/// The sheet's two columns, as DESIGN.md's mock shows. Which group goes where is
/// [`keys::HelpGroup::column`]'s, because the split is by height rather than by halving the list.
const COLUMNS: usize = 2;

/// The sheet, in the overlay layer. `open` is the app's flag.
pub fn view(k: &Kit, t: tok::Theme, accent: Color, open: bool, layer: &mut Vec<El>) {
    let mut columns = row().key("help-columns").w_full().gap(28.0).items_start();
    for column in 0..COLUMNS {
        let mut body = col().key(format!("help-column-{column}")).grow(1.0).gap(18.0);
        for group in keys::HELP.iter().filter(|group| group.column == column) {
            body = body.child(section(group, k, t, accent));
        }
        columns = columns.child(body);
    }
    let footer = row()
        .key("help-footer")
        .w_full()
        .gap(6.0)
        .center_y()
        .child(Keybind::new(&["Esc"]).build(k))
        .child(
            text("closes this sheet")
                .font(type_::LABEL)
                .color(t.text_secondary),
        )
        .child(el_grow())
        .child(
            text("The terminal app's keys, unchanged.")
                .font(type_::LABEL)
                .color(t.text_secondary),
        );
    Dialog::new("help", "Keyboard")
        .open(open)
        .width(720.0)
        .fit()
        .on_close(|s: &mut crate::app::App| s.help = false)
        .child(columns)
        .child(footer)
        .build(k, layer);
}

fn el_grow() -> El {
    rune::el().grow(1.0)
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
