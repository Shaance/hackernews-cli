use super::{
    config::{Profile, Provider},
    Progress, Reader, Request,
};
use crossterm::event::KeyCode;
use hn_lib::ui::widgets::render_modal;
use ratatui::{
    style::Modifier,
    text::{Line, Span},
    Frame,
};

pub struct Settings {
    pub(super) opened: Option<std::time::Instant>,
    keys: [String; 2],
    provider: Provider,
    automatic: bool,
    profile: Profile,
    cursor: usize,
    error: String,
    retry: bool,
}
impl Reader {
    pub(super) fn edit_settings(&mut self) {
        let retry = self
            .active
            .and_then(|id| self.readings.get(&id))
            .is_some_and(|r| r.article.failed() || r.discussion.failed());
        self.settings = Some(Settings {
            opened: self.theme.motion.then(std::time::Instant::now),
            retry,
            keys: [
                self.config.key_for(Provider::OpenRouter).to_owned(),
                self.config.key_for(Provider::Anthropic).to_owned(),
            ],
            provider: self.config.provider,
            automatic: self.config.automatic,
            profile: self.config.profile,
            cursor: if self.config.key().is_empty() || self.auth_failed {
                1
            } else if retry {
                5
            } else {
                2
            },
            error: self.settings_error.clone(),
        });
    }
    pub(super) fn settings_key(&mut self, key: KeyCode) {
        let Some(s) = self.settings.as_mut() else {
            return;
        };
        let rows = if s.retry { 6 } else { 5 };
        match key {
            KeyCode::Esc => {
                self.settings = None;
            }
            KeyCode::Tab | KeyCode::Down => s.cursor = (s.cursor + 1) % rows,
            KeyCode::BackTab | KeyCode::Up => s.cursor = (s.cursor + rows - 1) % rows,
            KeyCode::Backspace if s.cursor == 1 => {
                s.keys[s.provider as usize].pop();
            }
            KeyCode::Delete if s.cursor == 1 => s.keys[s.provider as usize].clear(),
            KeyCode::Char(c) if s.cursor == 1 && !c.is_control() => {
                s.keys[s.provider as usize].push(c)
            }
            KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right if s.cursor == 2 => {
                s.automatic = !s.automatic
            }
            KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right if s.cursor == 3 => {
                s.profile = if s.profile == Profile::Fast {
                    Profile::Thorough
                } else {
                    Profile::Fast
                }
            }
            KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right if s.cursor == 0 => {
                s.provider = if s.provider == Provider::OpenRouter {
                    Provider::Anthropic
                } else {
                    Provider::OpenRouter
                };
            }
            KeyCode::Enter => {
                self.apply_settings();
            }
            _ => {}
        }
    }
    fn apply_settings(&mut self) {
        let Some(s) = self.settings.as_ref() else {
            return;
        };
        let refresh = s.cursor == 4;
        let generate = s.cursor == 3;
        let retry = s.cursor == 5;
        let repair = s.cursor == 1;
        let mut config = self.config.clone();
        let requested = self
            .active
            .and_then(|id| self.readings.get(&id))
            .is_some_and(|r| r.article.requested() || r.discussion.requested());
        let auto_disabled = self.config.automatic && !s.automatic;
        let profile_changed = s.profile != config.profile;
        let changed_key = s.provider != config.provider
            || s.keys[s.provider as usize].trim() != config.key_for(s.provider);
        for provider in [Provider::OpenRouter, Provider::Anthropic] {
            let key = s.keys[provider as usize].trim();
            if key != config.key_for(provider) {
                match provider {
                    Provider::OpenRouter => {
                        config.api_key = key.to_owned();
                        config.environment_key.clear();
                    }
                    Provider::Anthropic => {
                        config.anthropic_api_key = key.to_owned();
                        config.anthropic_environment_key.clear();
                    }
                }
            }
        }
        config.provider = s.provider;
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
        let repaired = changed_key || (repair && self.auth_failed);
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
        if retry || refresh || generate || self.config.automatic || (profile_changed && requested) {
            self.request(if retry {
                Request::Retry
            } else if refresh {
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
        if let Some(settings) = &self.settings {
            settings.render(frame, self.theme);
        }
    }
}

impl Settings {
    fn render(&self, frame: &mut Frame, theme: hn_lib::theme::Theme) {
        let s = self;
        render_modal(frame, theme, self.opened, "Summary settings", |width, _| {
            let width = width.min(64);
            let key = &s.keys[s.provider as usize];
            let values = [
                ("Provider".to_owned(), s.provider.label().to_owned()),
                (
                    format!("{} key", s.provider.label()),
                    if key.is_empty() {
                        "paste a key".into()
                    } else {
                        "•".repeat(key.chars().count().min(20))
                    },
                ),
                (
                    "Automatic summaries".into(),
                    if s.automatic { "On" } else { "Off" }.into(),
                ),
                (
                    "Model".into(),
                    match s.profile {
                        Profile::Fast => "Fast · Haiku 5.5",
                        Profile::Thorough => "Thorough · Sonnet 5.5",
                    }
                    .into(),
                ),
            ];
            let mut lines = vec![
                Line::styled("Applies to this story and future summaries.", theme.dim),
                Line::raw(""),
            ];
            for (index, (label, value)) in values.into_iter().enumerate() {
                let value = if index == s.cursor && index != 1 {
                    format!("‹ {value} ›")
                } else {
                    value
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("{label:22}"), theme.dim),
                    Span::styled(
                        value,
                        if index == 1 && key.is_empty() {
                            theme.dim.add_modifier(Modifier::ITALIC)
                        } else {
                            theme.fg
                        },
                    ),
                ]));
            }
            lines.push(Line::raw(""));
            lines.push(Line::styled("Refresh sources & summary", theme.fg));
            if s.retry {
                lines.push(Line::styled("Retry failed summary", theme.fg));
            }
            lines.push(Line::raw(""));
            let description = match s.cursor {
                1 => "Type/paste key · Delete clears · input is masked",
                2 => "Off stops new automatic requests. Submitted work can finish.",
                3 => {
                    if s.profile == Profile::Fast {
                        "Fast: Haiku 5.5 · quick overview of the main insights"
                    } else {
                        "Thorough: Sonnet 5.5 · broader coverage and useful qualifications"
                    }
                }
                4 => "Fetch newer article and comments, then replace their summaries.",
                5 => "Retry failed sections using available captures; keep successful summaries.",
                _ => "Provider for new requests. Completed summaries stay available.",
            };
            lines.extend(
                textwrap::wrap(description, usize::from(width).max(1))
                    .into_iter()
                    .map(|line| Line::styled(line.into_owned(), theme.dim)),
            );
            lines.push(Line::raw(""));
            let mut hints = hn_lib::ui::widgets::hint_line(
                theme,
                &[
                    ("↑↓", "select"),
                    ("←→", "change"),
                    ("Enter", "apply"),
                    ("Esc", "cancel"),
                ],
                None,
                width + 4,
            );
            hints.spans.remove(0); // The modal supplies the two-column padding.
            lines.push(hints);
            if !s.error.is_empty() {
                lines.push(Line::styled(s.error.clone(), theme.error));
            }
            (lines, Some(2 + s.cursor + usize::from(s.cursor >= 4)))
        });
    }
}

#[cfg(test)]
#[test]
fn settings_modal_styles_focus_padding_and_scrim() {
    use ratatui::{
        backend::TestBackend,
        style::{Color, Modifier},
        Terminal,
    };
    let theme = hn_lib::theme::Theme::TRUE_DARK;
    let settings = Settings {
        opened: None,
        keys: Default::default(),
        provider: Provider::OpenRouter,
        automatic: true,
        profile: Profile::Fast,
        cursor: 1,
        error: String::new(),
        retry: false,
    };
    let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
    let frame = terminal
        .draw(|f| {
            let area = f.area();
            f.buffer_mut().set_style(area, theme.sel.patch(theme.link));
            f.buffer_mut()[(0, 0)].set_symbol("▎");
            settings.render(f, theme);
        })
        .unwrap();
    let buffer = frame.buffer;
    let corner = buffer
        .content
        .iter()
        .position(|c| c.symbol() == "╭")
        .unwrap();
    let (left, top) = (corner as u16 % 100, corner as u16 / 100);
    assert!(left >= 2 && top >= 1);
    assert_eq!(buffer[(left, top)].fg, theme.line.fg.unwrap());
    let right = (left..100)
        .find(|x| buffer[(*x, top)].symbol() == "╮")
        .unwrap();
    let bottom = (top + 1..40)
        .find(|y| buffer[(left, *y)].symbol() == "╰")
        .unwrap();
    for pos in [(right, top), (left, bottom), (right, bottom)] {
        assert_eq!(buffer[pos].fg, theme.line.fg.unwrap());
    }
    assert_eq!(buffer[(right, bottom)].symbol(), "╯");
    assert_eq!(buffer[(left + 2, top)].fg, theme.accent.fg.unwrap());
    assert!(buffer[(left + 2, top)].modifier.contains(Modifier::BOLD));
    let y = top + 5; // top padding, subtitle, blank, Provider, key
    assert!((left + 1..right).all(|x| buffer[(x, y)].bg == theme.sel.bg.unwrap()));
    assert_eq!(buffer[(left + 1, y)].symbol(), "▎");
    assert_eq!(buffer[(left + 1, y)].fg, theme.accent.fg.unwrap());
    let x = left + 3 + 22;
    let placeholder: String = (x..x + 11).map(|x| buffer[(x, y)].symbol()).collect();
    assert_eq!(placeholder, "paste a key");
    assert_eq!(buffer[(x, y)].fg, theme.dim.fg.unwrap());
    assert!(buffer[(x, y)].modifier.contains(Modifier::ITALIC));
    assert_eq!(
        buffer[(0, 0)].fg,
        theme.scrim.fg.unwrap(),
        "backdrop must use scrim foreground"
    );
    assert_eq!(
        buffer[(0, 0)].bg,
        Color::Reset,
        "backdrop must clear selection tint"
    );
    assert!(
        buffer
            .content
            .iter()
            .any(|c| c.symbol() == "▎" && c.fg == theme.scrim.fg.unwrap()),
        "selected comment glyphs must survive behind scrim"
    );
}
