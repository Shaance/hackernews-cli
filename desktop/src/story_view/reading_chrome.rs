//! Reading transitions, loading skeletons, footer, and toolbar.

use rune::tok::{self, type_};
use rune::{Color, El, Spring, SpringPair, Transition, Tx, el, row, text};
use rune_kit::{Keybind, Kit};

use crate::app::App;
use crate::app::chrome::quiet_button;
use crate::story::Story;

/// A new reading surface arrives together; the old text must leave before it can overlap it.
pub(super) fn reading_transition(reduced: bool) -> Transition {
    if reduced {
        return Transition::none();
    }
    let mut transition = Transition::asymmetric(Transition::opacity(), Transition::none())
        .springs(SpringPair::new(Spring::new(0.15, 0.0), Spring::NONE));
    transition.insertion.opacity = 0.65;
    transition
}

/// A skeleton row: a bar whose width varies so the block reads as text, not as a table.
pub(super) fn skeleton_line(n: usize, t: tok::Theme) -> El {
    let width = [1.0, 0.86, 0.62, 0.94, 0.48][n % 5];
    el()
        .key(format!("skeleton-{n}"))
        .w_full()
        .h(9.0)
        .radius(4.0)
        .fill(t.control_off)
        .opacity(0.35 + 0.5 * width)
}

/// One footer stays in the same place when the reader enters or leaves a source.
pub fn story_footer(s: &Story, k: &Kit, t: tok::Theme, hints: &rune::Live<f32>, reduced: bool, contrast: bool) -> El {
    let in_source = s.source().is_some();
    let back = if in_source { "Back to discussion" } else { "Back to feed" };
    let h = |keys: &[&str], label: &str| crate::app::chrome::passive_hint(hint(k, t, keys, label), hints, reduced, contrast);
    let mut footer = row().key("story-footer").instant_changes().w_full().px(24.0).py(2.0).gap(18.0).flex_wrap().center_y()
        .child(row().key("story-back").role("button").label(back).focusable(true)
            .h(36.0).px(8.0).gap(6.0).center_y().radius(4.0)
            .hover(k.c.hover).press(k.c.pressed).cursor(rune::Cursor::Pointer)
            .on_press_tx(|s: &mut App, tx: &mut Tx| { s.story_back(tx); })
            .child(Keybind::new(&["Esc"]).build(k))
            .child(text(back).font(type_::LABEL).color(t.text)))
        .child(h(&["j", "k"], "move"))
        .child(h(&["Tab"], if in_source { "next source" } else { "citation" }));
    if !in_source {
        footer = footer.child(h(&["⇧", "S"], "summary"))
            .child(h(&["r"], "refresh comments"));
    }
    let kit = k.clone();
    footer.child(quiet_button("Open in browser")
        .label_with(move |font, ink| row().gap(6.0).center_y()
            .child(Keybind::new(&["⇧", "O"]).build(&kit))
            .child(text("browser ↗").font(font).color(ink)))
        .on_press(|s: &mut App| s.pending_external = true)
        .build(k).key("article-open-original").cursor(rune::Cursor::Pointer).shortcut_hint("⇧O"))
}

fn hint(k: &Kit, t: tok::Theme, keys: &[&str], label: &str) -> El {
    row().gap(6.0).center_y().child(Keybind::new(keys).build(k))
        .child(text(label).font(type_::LABEL).color(t.text_secondary))
}

/// The titlebar remains a drag region and may carry the saved indicator.
pub fn story_toolbar(t: tok::Theme, accent: Color, saved: bool) -> El {
    let mut bar = row().key("story-toolbar").w_full().center_y();
    if saved {
        bar = bar.child(row().key("story-saved").role("status").label("Saved").gap(6.0).center_y()
            .child(el().key("story-saved-dot").size(6.0, 6.0).radius(3.0).fill(accent))
            .child(text("Saved").font(type_::LABEL).color(t.text_secondary).a11y_hidden()));
    }
    bar
}

