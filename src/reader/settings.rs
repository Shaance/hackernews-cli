use super::{config::Profile, Progress, Reader, Request};
use crossterm::event::KeyCode;
use ratatui::{
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

pub struct Settings {
    key: String,
    automatic: bool,
    profile: Profile,
    cursor: usize,
    error: String,
}
impl Reader {
    pub(super) fn edit_settings(&mut self) {
        self.settings = Some(Settings {
            key: self.config.key().to_owned(),
            automatic: self.config.automatic,
            profile: self.config.profile,
            cursor: if self.config.key().is_empty() || self.auth_failed {
                0
            } else {
                1
            },
            error: self.settings_error.clone(),
        });
    }
    pub(super) fn settings_key(&mut self, key: KeyCode) {
        let Some(s) = self.settings.as_mut() else {
            return;
        };
        match key {
            KeyCode::Esc => {
                self.settings = None;
            }
            KeyCode::Tab | KeyCode::Down => s.cursor = (s.cursor + 1) % 4,
            KeyCode::BackTab | KeyCode::Up => s.cursor = (s.cursor + 3) % 4,
            KeyCode::Backspace if s.cursor == 0 => {
                s.key.pop();
            }
            KeyCode::Delete if s.cursor == 0 => s.key.clear(),
            KeyCode::Char(c) if s.cursor == 0 && !c.is_control() => s.key.push(c),
            KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right if s.cursor == 1 => {
                s.automatic = !s.automatic
            }
            KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right if s.cursor == 2 => {
                s.profile = if s.profile == Profile::Fast {
                    Profile::Thorough
                } else {
                    Profile::Fast
                }
            }
            KeyCode::Enter => {
                let refresh = s.cursor == 3;
                let generate = s.cursor == 2;
                self.apply_settings(refresh, generate);
            }
            _ => {}
        }
    }
    fn apply_settings(&mut self, refresh: bool, generate: bool) {
        let Some(s) = self.settings.as_ref() else {
            return;
        };
        let mut config = self.config.clone();
        let requested = self
            .active
            .and_then(|id| self.readings.get(&id))
            .is_some_and(|r| r.article.requested() || r.discussion.requested());
        let auto_disabled = self.config.automatic && !s.automatic;
        let profile_changed = s.profile != config.profile;
        let changed_key = s.key.trim() != config.key();
        if changed_key {
            config.api_key = s.key.trim().to_owned();
            config.environment_key.clear();
        }
        config.profile = s.profile;
        config.automatic = s.automatic;
        let result = self
            .config_path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Set HOME or XDG_CONFIG_HOME to save settings"))
            .and_then(|path| config.save(path));
        if let Err(error) = result {
            self.settings.as_mut().unwrap().error = error.to_string();
            return;
        }
        let repaired = changed_key || self.auth_failed;
        self.config = config;
        self.settings = None;
        self.settings_error.clear();
        if auto_disabled {
            for reading in self.readings.values_mut() {
                if matches!(reading.article.progress, Progress::Queued(a) | Progress::Preparing(a, _, _) if a.automatic)
                {
                    reading.article.cancel_preparation();
                }
                if matches!(reading.discussion.progress, Progress::Queued(a) | Progress::Preparing(a, _, _) if a.automatic)
                {
                    reading.discussion.cancel_preparation();
                }
            }
        }
        if repaired {
            self.auth_failed = false;
            for (id, reading) in self.readings.iter_mut() {
                if changed_key && reading.article.pending() {
                    reading.article.cancel_preparation();
                    reading.article.progress = Progress::Idle;
                    if self.active == Some(*id) {
                        reading.article.queue(
                            self.config.profile,
                            &mut self.next_attempt,
                            Request::Generate,
                        );
                    }
                }
                if changed_key && reading.discussion.pending() {
                    reading.discussion.cancel_preparation();
                    reading.discussion.progress = Progress::Idle;
                    if self.active == Some(*id) {
                        reading.discussion.queue(
                            self.config.profile,
                            &mut self.next_attempt,
                            Request::Generate,
                        );
                    }
                }
                if self.active == Some(*id)
                    && matches!(&reading.article.progress, Progress::Failed(_, e) if e.auth)
                {
                    reading.article.queue(
                        self.config.profile,
                        &mut self.next_attempt,
                        Request::Retry,
                    );
                }
                if self.active == Some(*id)
                    && matches!(&reading.discussion.progress, Progress::Failed(_, e) if e.auth)
                {
                    reading.discussion.queue(
                        self.config.profile,
                        &mut self.next_attempt,
                        Request::Retry,
                    );
                }
            }
        }
        if refresh || generate || self.config.automatic || (profile_changed && requested) {
            self.request(if refresh {
                Request::Refresh
            } else if generate || (profile_changed && requested) {
                Request::Generate
            } else {
                Request::Automatic
            });
        }
        self.start_queued();
    }
    pub(super) fn render_settings(&self, frame: &mut Frame) {
        let Some(s) = &self.settings else {
            return;
        };
        let values = [
            format!(
                "OpenRouter key: {}",
                if s.key.is_empty() {
                    "[paste key]".into()
                } else {
                    "•".repeat(s.key.chars().count().min(20))
                }
            ),
            format!(
                "Automatic summaries: {}",
                if s.automatic { "On" } else { "Off" }
            ),
            format!("Model preference: {}", s.profile.label()),
            "Refresh sources & summary".into(),
        ];
        let mut lines = vec![
            Line::raw(""),
            Line::raw("For this story and future summaries"),
            Line::raw(""),
        ];
        for (i, value) in values.iter().enumerate() {
            lines.push(Line::styled(
                format!("{} {value}", if i == s.cursor { ">" } else { " " }),
                Style::default().fg(if i == s.cursor {
                    Color::Yellow
                } else {
                    Color::Reset
                }),
            ));
        }
        lines.extend([
            Line::raw(""),
            Line::raw(match s.cursor {
                0 => "Type/paste key · Delete clears · input is masked",
                1 => "Off stops new automatic requests. Submitted work can finish.",
                2 => {
                    if s.profile == Profile::Fast {
                        "Fast: Haiku 5.5 · quick overview of the main insights"
                    } else {
                        "Thorough: Sonnet 5.5 · broader coverage and useful qualifications"
                    }
                }
                _ => "Fetch newer article and comments, then replace their summaries.",
            }),
            Line::raw(""),
            Line::raw("Tab/↑↓ Select · Space/←→ Change"),
            Line::raw("Enter Apply · Esc Cancel"),
        ]);
        if !s.error.is_empty() {
            lines.push(Line::styled(
                s.error.clone(),
                Style::default().fg(Color::Red),
            ));
        }
        frame.render_widget(
            Paragraph::new(lines).wrap(Wrap { trim: false }).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Summary settings"),
            ),
            frame.area(),
        );
    }
}
