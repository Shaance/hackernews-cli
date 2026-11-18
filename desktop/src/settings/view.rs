//! Summary settings dialog layout and callbacks.

use super::*;
use rune::tok::{self, type_};
use rune::{Color, El, ScrollSpec, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Dialog, Kit, SegmentedControl, Switch, TextInput};

fn panel(t: tok::Theme) -> El {
    col().w_full().padding(16.0).radius(10.0).fill(t.card).border(1.0, t.card_border)
}

fn quality_tile(profile: Profile, provider: Provider, checked: bool, t: tok::Theme, accent: Color) -> El {
    let (key, description) = match profile {
        Profile::Fast => ("fast", "A quick overview."),
        Profile::Thorough => ("thorough", "Reads more of the discussion."),
    };
    let model = profile.model(provider);
    let mut tile = col()
        .key(key)
        .role("radio")
        .label(profile.label())
        .description(format!("{description} {model}"))
        .position_in_set(if profile == Profile::Fast { 1 } else { 2 }, 2)
        .focusable(true)
        .focus_selected(checked)
        .focus_ring(2.0, 1.0, accent)
        .w_full()
        .padding(12.0)
        .radius(8.0)
        .border(if checked { 1.5 } else { 1.0 }, if checked { accent } else { t.card_border })
        .cursor(rune::Cursor::Pointer)
        .child(
            col().w_full().gap(4.0).a11y_hidden()
                .child(text(profile.label()).font(type_::TITLE).color(t.text))
                .child(text(description).font(type_::LABEL).color(t.text_secondary).wrap())
                .child(text(model).key(format!("{key}-model")).font(type_::MONO_CAPTION).color(t.text_secondary).wrap_anywhere()),
        )
        .on_key(move |s: &mut crate::app::App, ev: &rune::KeyEvent, _tx: &mut rune::Tx| {
            if ev.key != rune::Key::Enter || ev.mods != rune::Mods::default() {
                return false;
            }
            if let Some(settings) = s.settings_mut() {
                settings.set_profile(profile);
            }
            true
        })
        .on_press(move |s: &mut crate::app::App| {
            if let Some(settings) = s.settings_mut() {
                settings.set_profile(profile);
            }
        });
    if checked {
        tile = tile.fill(accent.alpha(0.10));
    } else {
        tile = tile.hover(t.hover);
    }
    tile
}

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
        .width(None)
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

        let mut key_fields = col().key("settings-key").w_full().gap(6.0)
            .child(text(key_label).font(type_::LABEL).color(t.text_secondary).a11y_hidden())
            .child(key)
            .child(text("Saved to ~/.config/hn/summary.json by default when you apply.")
                .font(type_::LABEL).color(t.text_secondary).wrap());
        if self.reenter {
            key_fields = key_fields.child(
                text("The provider rejected the key. Enter a new one and apply.")
                    .key("settings-reenter").role("alert").font(type_::LABEL).color(accent).wrap(),
            );
        }
        let provider_panel = panel(t).key("settings-provider-panel").gap(16.0)
            .child(providers)
            .child(key_fields);

        let automatic = Switch::new(self.draft.automatic)
            .label("Automatic summaries")
            .hide_label()
            .on_toggle(|s: &mut crate::app::App| {
                if let Some(settings) = s.settings_mut() {
                    settings.toggle_automatic();
                }
            })
            .build(k);
        let summaries_panel = panel(t).key("settings-summaries-panel").gap(16.0)
            .child(
                row().key("settings-automatic").w_full().gap(16.0).center_y()
                    .child(col().grow(1.0).shrink().gap(4.0)
                        .child(text("Automatic summaries").font(type_::BODY).color(t.text).a11y_hidden())
                        .child(text("Summarize a story the first time you open it.")
                            .font(type_::LABEL).color(t.text_secondary).wrap()))
                    .child(automatic),
            )
            .child(el().key("settings-summaries-separator").w_full().h(1.0).fill(t.separator))
            .child(
                col().key("settings-profile").w_full().gap(8.0)
                    .child(text("Quality").font(type_::LABEL).color(t.text_secondary).a11y_hidden())
                    .child(
                        row().key("settings-quality").role("radiogroup").label("Summary quality")
                            .focus_group().w_full().equal_columns(2).gap(10.0)
                            .child(quality_tile(Profile::Fast, provider, self.draft.profile == Profile::Fast, t, k.c.border_focus))
                            .child(quality_tile(Profile::Thorough, provider, self.draft.profile == Profile::Thorough, t, k.c.border_focus)),
                    ),
            );

        let mut body = col().key("settings-fields").w_full().pb(20.0).gap(24.0)
            .child(col().key("settings-provider-group").w_full().gap(8.0)
                .child(text("Provider").role("heading").level(2).font(type_::SECTION).color(t.text_secondary))
                .child(provider_panel))
            .child(col().key("settings-summaries-group").w_full().gap(8.0)
                .child(text("Summaries").role("heading").level(2).font(type_::SECTION).color(t.text_secondary))
                .child(summaries_panel));
        if let Some(error) = &self.error {
            body = body.child(
                text(error.clone()).key("settings-error").role("alert")
                    .font(type_::LABEL).color(accent).wrap_anywhere(),
            );
        }
        let actions = row().key("settings-actions").w_full().px(16.0).py(16.0).gap(8.0).center_y()
            .children(has_story.then(||
                Button::new("Regenerate summary")
                    .variant(ButtonVariant::GhostMuted)
                    .on_press(|s: &mut crate::app::App| s.apply_settings(true))
                    .build(k)
            ))
            .child(el().grow(1.0))
            .child(Button::new("Cancel").variant(ButtonVariant::Neutral)
                .on_press(|s: &mut crate::app::App| s.close_settings()).build(k))
            .child(Button::new("Apply").variant(ButtonVariant::Submit)
                .on_press(|s: &mut crate::app::App| s.apply_settings(false)).build(k));
        let footer = col().key("settings-footer").w_full()
            .child(el().key("settings-footer-separator").w_full().h(1.0).fill(t.separator))
            .child(actions);

        let mut viewport = col().key("settings-scroll").w_full().shrink().min_h(0.0)
            .scroll(ScrollSpec { stick_bottom: false }).scroll_edge_fade(10.0).scroll_margin(10.0)
            .child(body);
        if let Some([_, height]) = rune::memo::layout_size("window") {
            let header = rune::memo::layout_size("settings-dialog/header").map_or(50.0, |[_, h]| h);
            let footer = rune::memo::layout_size("settings-footer").map_or(57.0, |[_, h]| h);
            // Layer: 16 pt top + bottom; header: 50 pt; footer: 1 pt rule + 16 pt
            // top/bottom + 24 pt controls = 57 pt. Body padding scrolls inside the viewport.
            viewport = viewport.max_h((height - 32.0 - header - footer).max(0.0));
        }

        Dialog::new("settings", "Summary settings")
            .open(open).width(520.0).fit()
            .on_close(|s: &mut crate::app::App| s.close_settings())
            .child(col().key("settings-body").w_full()
                .child(col().w_full().px(16.0).shrink().min_h(0.0).child(viewport))
                .child(footer))
            .build(k, layer);
    }
}
