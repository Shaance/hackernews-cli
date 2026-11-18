//! The summary settings dialog: the terminal reader's `M` overlay, as a `Dialog`.
//!
//! The draft is a copy of the reader's [`Config`]; nothing is written until `Apply`, and
//! `Cancel` or `Esc` throws the copy away. `Apply` goes through `hn_lib::reader::save_config`,
//! so the window and the terminal app share one file and one writer.
//!
//! The key field is a secure [`TextInput`](rune_kit::TextInput): Rune emits no `text` for a secure field and skips
//! its accessibility value, so the key cannot reach `--tree`, a trace or a log even by accident.
//! Nothing here formats the key into a string.

use std::path::PathBuf;

use hn_lib::reader::{self, Config, Profile, Provider};
use rune::TextEdit;

mod view;

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
}
