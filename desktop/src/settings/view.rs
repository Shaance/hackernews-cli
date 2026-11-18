//! Summary settings dialog layout and callbacks.

use super::*;
use rune::tok::{self, type_};
use rune::{Color, El, ScrollSpec, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Dialog, Kit, SegmentedControl, Switch, TextInput};

impl Settings {
    /// The dialog, in the overlay layer. `open` is the app's flag; `on_close` is what `Esc`,
    /// the close button and a click outside all run.
    pub fn view(&self, k: &Kit, t: tok::Theme, accent: Color, open: bool, has_story: bool, layer: &mut Vec<El>) {
        let mut kit = k.clone();
        kit.c.bg_accent = accent;
        kit.c.border_focus = accent;
        let k = &kit;
        let provider = self.draft.provider;
        let providers = SegmentedControl::new(
            &[Provider::OpenRouter.label(), Provider::Anthropic.label()],
            Some(if provider == Provider::OpenRouter { 0 } else { 1 }),
        )
        .label("Provider")
        .fit()
        .on_pick(|s: &mut crate::app::App, picked: usize, _tx: &mut rune::Tx| {
            if let Some(settings) = s.settings_mut() {
                settings.set_provider(if picked == 0 { Provider::OpenRouter } else { Provider::Anthropic });
            }
        })
        .build(k)
        .key("settings-providers");

        let key_label = format!("{} key", provider.label());
        let key = TextInput::new(&self.key)
            .secure()
            .label(key_label.clone())
            .placeholder("paste a key")
            .width(None)
            .on_edit(|s: &mut crate::app::App| &mut s.settings_mut().expect("the dialog is open").key)
            .build(k);

        let automatic = Switch::new(self.draft.automatic)
            .label("Automatic summaries")
            .hide_label()
            .on_toggle(|s: &mut crate::app::App| {
                if let Some(settings) = s.settings_mut() {
                    settings.toggle_automatic();
                }
            })
            .build(k);

        let profile = SegmentedControl::new(
            &[Profile::Fast.label(), Profile::Thorough.label()],
            Some(if self.draft.profile == Profile::Fast { 0 } else { 1 }),
        )
        .label("Summary quality")
        .fit()
        .on_pick(|s: &mut crate::app::App, picked: usize, _tx: &mut rune::Tx| {
            if let Some(settings) = s.settings_mut() {
                settings.set_profile(if picked == 0 {
                    Profile::Fast
                } else {
                    Profile::Thorough
                });
            }
        })
        .build(k)
        .key("settings-quality");

        let mut body = col()
            .key("settings-fields")
            .w_full()
            .gap(20.0)
            .child(
                col()
                    .key("settings-key")
                    .w_full()
                    .gap(8.0)
                    .child(text("Provider").font(type_::LABEL).color(t.text_secondary).a11y_hidden())
                    .child(row().child(providers))
                    .child(
                        text(key_label)
                            .font(type_::LABEL)
                            .color(t.text_secondary)
                            .a11y_hidden(),
                    )
                    .child(key),
            )
            .child(
                col()
                    .key("settings-automatic")
                    .w_full()
                    .gap(6.0)
                    .child(
                        row()
                            .w_full()
                            .center_y()
                            .justify_between()
                            .child(text("Automatic summaries").font(type_::BODY).color(t.text).a11y_hidden())
                            .child(automatic),
                    )
                    .child(
                        text("Summarize stories on first open.")
                            .font(type_::LABEL)
                            .color(t.text_secondary)
                            .wrap(),
                    ),
            )
            .child(
                col()
                    .key("settings-profile")
                    .w_full()
                    .gap(8.0)
                    .child(text("Summary quality").font(type_::LABEL).color(t.text_secondary).a11y_hidden())
                    .child(
                        row()
                            .w_full()
                            .gap(10.0)
                            .center_y()
                            .child(profile)
                            .child(
                                text(self.model())
                                    .key("settings-model")
                                    .font(type_::MONO_CAPTION)
                                    .color(t.text_secondary)
                                    .wrap_anywhere()
                                    .shrink(),
                            ),
                    ),
            );
        if self.reenter {
            body = body.child(
                text("The provider rejected the key. Enter a new one and apply.")
                    .key("settings-reenter")
                    .role("alert")
                    .font(type_::LABEL)
                    .color(accent)
                    .wrap(),
            );
        }
        if let Some(error) = &self.error {
            body = body.child(
                text(error.clone())
                    .key("settings-error")
                    .role("alert")
                    .font(type_::LABEL)
                    .color(accent)
                    .wrap_anywhere(),
            );
        }
        let actions =
            row()
                .key("settings-actions")
                .w_full()
                .gap(8.0)
                .center_y()
                .children(has_story.then(||
                    Button::new("Regenerate summary")
                        .variant(ButtonVariant::GhostMuted)
                        .on_press(|s: &mut crate::app::App| s.apply_settings(true))
                        .build(k)
                ))
                .child(el().grow(1.0))
                .child(
                    Button::new("Cancel")
                        .variant(ButtonVariant::Neutral)
                        .on_press(|s: &mut crate::app::App| s.close_settings())
                        .build(k),
                )
                .child(
                    Button::new("Apply")
                        .variant(ButtonVariant::Submit)
                        .on_press(|s: &mut crate::app::App| s.apply_settings(false))
                        .build(k),
                );

        let mut viewport = col()
            .key("settings-scroll")
            .w_full()
            .shrink()
            .min_h(0.0)
            .scroll(ScrollSpec { stick_bottom: false })
            .scroll_edge_fade(10.0)
            .scroll_margin(10.0)
            .child(body);
        if let Some([_, height]) = rune::memo::layout_size("window") {
            let header = rune::memo::layout_size("settings-dialog/header").map_or(50.0, |[_, h]| h);
            let actions = rune::memo::layout_size("settings-actions").map_or(24.0, |[_, h]| h);
            // The layer has 16 points on each edge; the body has a 16-point gap and bottom inset.
            viewport = viewport.max_h((height - 32.0 - header - actions - 32.0).max(0.0));
        }

        Dialog::new("settings", "Summary settings")
            .open(open)
            .width(460.0)
            .fit()
            .on_close(|s: &mut crate::app::App| s.close_settings())
            .child(col().key("settings-body").w_full().px(16.0).pb(16.0).gap(16.0).child(viewport).child(actions))
            .build(k, layer);
    }
}
