//! Shared feed and reading window affordances.

use rune::tok;
use rune::{el, row, text, Color, El, Spring, TextStyle};
use rune_kit::shell::PaneMotion;
use rune_kit::{Button, ButtonVariant, ControlSize};

pub(super) fn brand(t: tok::Theme) -> El {
    row().key("brand").role("group").label("Hacker News").gap(6.0).center_y()
        .child(row().size(24.0, 24.0).center().radius(4.0)
            .fill(rune::color::hex("#ff6600").expect("the brand color is valid"))
            .child(text("HN").font(TextStyle::new(10.0, 700.0)).color(Color::WHITE).a11y_hidden()))
        .child(text("Hacker News").font(TextStyle::new(13.0, 500.0)).color(t.text_secondary).a11y_hidden())
}

pub(crate) fn quiet_button(label: impl Into<String>) -> Button {
    Button::new(label).variant(ButtonVariant::GhostMuted).control_size(ControlSize::Small)
}

/// The split's motion: `MORPH`'s spring, scaled by the pane's width so a narrow window does not
/// feel twitchy, with the tail fade DESIGN.md asks for.
pub(super) fn pane_motion() -> PaneMotion {
    PaneMotion::new()
        .spring(Spring::new(0.30, 0.16))
        .scale_with_width(480.0, 0.15, 0.3)
        .fade_tail(0.15)
}

/// The saved state, everywhere it appears: a 6 pt amber dot. The word lives in the accessible
/// label, not in the layout.
pub(super) fn saved_dot(key: impl Into<String>, accent: Color) -> El {
    el()
        .key(key)
        .size(6.0, 6.0)
        .radius(3.0)
        .fill(accent)
        .role("status")
        .label("saved")
}

pub(crate) fn passive_hint(hint: El, hints: &rune::Live<f32>, reduced: bool, contrast: bool) -> El {
    if contrast { hint } else {
        hint.live_opacity(hints.smooth(if reduced { Spring::NONE } else { Spring::new(0.20, 0.0) }))
    }
}
