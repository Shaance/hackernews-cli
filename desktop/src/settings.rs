//! The summary settings dialog: the terminal reader's `M` overlay, as a `Dialog`.
//!
//! The draft is a copy of the reader's [`Config`]; nothing is written until `Apply`, and
//! `Cancel` or `Esc` throws the copy away. `Apply` goes through `hn_lib::reader::save_config`,
//! so the window and the terminal app share one file and one writer.
//!
//! The key field is a secure [`TextInput`]: Rune emits no `text` for a secure field and skips
//! its accessibility value, so the key cannot reach `--tree`, a trace or a log even by accident.
//! Nothing here formats the key into a string.

use std::path::PathBuf;

use hn_lib::reader::{self, Config, Profile, Provider};
use rune::tok::{self, type_};
use rune::{Color, El, ScrollSpec, TextEdit, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Dialog, Kit, SegmentedControl, Switch, TextInput};

/// The dialog's draft state, and the path `Apply` writes.
pub struct Settings {
    /// The draft: a copy of the reader's config, edited in place and saved on `Apply`.
    draft: Config,
    /// The key field. Secure, so its text never reaches the tree or a log.
    key: TextEdit,
    /// Where `Apply` writes. `None` when neither `XDG_CONFIG_HOME` nor `HOME` is set.
    path: Option<PathBuf>,
    /// The last save failure, shown in the dialog rather than swallowed.
    error: Option<String>,
    /// The provider rejected the key: the dialog says so and puts the cursor in the field.
    reenter: bool,
}

impl Settings {
    /// Open the dialog on `config`. `reenter` is the auth-failure path: the provider refused
    /// the key, so the dialog asks for a new one instead of offering a retry.
    pub fn open(config: &Config, path: Option<PathBuf>, reenter: bool) -> Settings {
        let mut key = TextEdit::new(config.key_for(config.provider));
        // The field is what the reader came to change, so it starts focused.
        key.focused = reenter;
        Settings {
            draft: config.clone(),
            key,
            path,
            error: None,
            reenter,
        }
    }

    pub fn provider(&self) -> Provider {
        self.draft.provider
    }

    /// Switch provider. The field follows, because each provider has its own key.
    pub fn set_provider(&mut self, provider: Provider) {
        if provider == self.draft.provider {
            return;
        }
        self.draft.provider = provider;
        self.key.replace_text(self.draft.key_for(provider));
        self.reenter = false;
    }

    pub fn automatic(&self) -> bool {
        self.draft.automatic
    }

    pub fn toggle_automatic(&mut self) {
        self.draft.automatic = !self.draft.automatic;
    }

    pub fn profile(&self) -> Profile {
        self.draft.profile
    }

    pub fn set_profile(&mut self, profile: Profile) {
        self.draft.profile = profile;
    }

    /// The model the chosen profile resolves to on the chosen provider, as the TUI shows it.
    pub fn model(&self) -> &'static str {
        self.draft.profile.model(self.draft.provider)
    }

    /// The key the reader typed, trimmed. Never logged, never rendered.
    fn typed_key(&self) -> &str {
        self.key.text.trim()
    }

    /// Replace the key field's contents, as typing or pasting does. `replace_text` zeroes the
    /// old allocation, so a replaced key does not linger in the field's buffer.
    pub fn set_key(&mut self, key: &str) {
        self.key.replace_text(key);
    }

    /// Whether the dialog is asking for a new key because the provider rejected the last one.
    pub fn reenter(&self) -> bool {
        self.reenter
    }

    /// Write the draft through `hn_lib`'s writer and return the config the window should use.
    ///
    /// A key the reader changed replaces the saved one and drops the environment fallback, as
    /// the terminal app's `apply_settings` does: the file is the source of truth once it holds
    /// a key. A failure leaves the dialog open with the message.
    pub fn apply(&mut self) -> Result<Config, String> {
        let mut config = self.draft.clone();
        let typed = self.typed_key().to_owned();
        match config.provider {
            Provider::OpenRouter => {
                if typed != config.api_key {
                    config.api_key = typed;
                    config.environment_key.clear();
                }
            }
            Provider::Anthropic => {
                if typed != config.anthropic_api_key {
                    config.anthropic_api_key = typed;
                    config.anthropic_environment_key.clear();
                }
            }
        }
        let Some(path) = self.path.as_ref() else {
            let message = "Set HOME or XDG_CONFIG_HOME to save settings".to_string();
            self.error = Some(message.clone());
            return Err(message);
        };
        if let Err(error) = reader::save_config(&config, path) {
            let message = error.to_string();
            self.error = Some(message.clone());
            return Err(message);
        }
        self.draft = config.clone();
        self.error = None;
        self.reenter = false;
        Ok(config)
    }

    /// The dialog, in the overlay layer. `open` is the app's flag; `on_close` is what `Esc`,
    /// the close button and a click outside all run.
    pub fn view(&self, k: &Kit, t: tok::Theme, accent: Color, open: bool, layer: &mut Vec<El>) {
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
                .child(
                    Button::new("Regenerate summary")
                        .variant(ButtonVariant::GhostMuted)
                        .on_press(|s: &mut crate::app::App| s.apply_settings(true))
                        .build(k),
                )
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
