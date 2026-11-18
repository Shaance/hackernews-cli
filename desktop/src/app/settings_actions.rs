//! Summary settings and authentication failure routing.

use super::{App, Notice};
use hn_lib::reader::Config;
use crate::settings::Settings;
use crate::story::SummaryState;

impl App {
    /// The settings dialog's draft, for its own press handlers.
    pub(crate) fn settings_mut(&mut self) -> Option<&mut Settings> {
        self.settings.as_mut()
    }

    /// Whether the open story's summary failed because the provider rejected the key. That is
    /// the one failure a retry cannot fix, so `M` opens the dialog asking for a new key.
    fn auth_failed(&self) -> bool {
        self.story
            .as_ref()
            .is_some_and(|story| matches!(&story.summary, SummaryState::Failed { auth: true, .. })
                || matches!(&story.article_summary, crate::story::ArticleSummaryState::Failed { auth: true, .. }))
    }

    /// The summary card's settings button, and `M`: the dialog, asking for a key when the
    /// provider rejected the last one.
    pub(crate) fn open_summary_settings(&mut self) {
        self.open_settings(self.auth_failed());
    }

    /// `M` / `⌘,`: the summary settings dialog, on a copy of the reader's config.
    fn open_settings(&mut self, reenter: bool) {
        let path = if self.headless {
            None
        } else {
            Config::path().ok()
        };
        self.settings = Some(Settings::open(&self.config, path, reenter));
    }

    /// `Cancel`, `Esc` or the close button: the draft is dropped and nothing is written.
    pub(crate) fn close_settings(&mut self) {
        self.settings = None;
    }

    /// `Apply`, and `Regenerate summary`. A save failure keeps the dialog open with the
    /// message; a success closes it, adopts the config and says so.
    pub(crate) fn apply_settings(&mut self, refresh: bool) {
        let Some(settings) = self.settings.as_mut() else {
            return;
        };
        let Ok(config) = settings.apply() else {
            return;
        };
        self.config = config;
        self.settings = None;
        self.toast = Some(Notice {
            message: "Settings saved".to_string(),
            until: f64::INFINITY,
            show_dot: true,
        });
        if !self.configured() {
            for story in self.story.iter_mut().chain(self.readings.values_mut()) {
                story.stop_summaries();
            }
        }
        if refresh {
            if self.story.is_some() {
                self.retry_summary();
            } else {
                self.refresh();
            }
        }
    }
}
