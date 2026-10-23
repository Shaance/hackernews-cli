use super::{config::Config, text::append, Progress, Reader, Reading};
use hn_lib::app::App;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use std::time::{Duration, Instant};
use tachyonfx::{fx, Interpolation};

impl Reader {
    pub fn render(&mut self, frame: &mut Frame, app: &mut App, tick: usize) {
        let theme = self.theme;
        let area = frame.area();
        frame.buffer_mut().set_style(area, theme.fg);
        let Some(reading) = self.active.and_then(|id| self.readings.get_mut(&id)) else {
            self.render_settings(frame);
            return;
        };
        let saved = if app.is_bookmarked(reading.story.id) {
            "★ "
        } else {
            ""
        };
        let host = reqwest::Url::parse(&reading.story.url)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .unwrap_or_else(|| "news.ycombinator.com".into());
        let width = usize::from(area.width);
        let title_width = width.saturating_sub(2).max(1);
        let title_text = format!("{saved}{}", reading.story.title);
        let mut title_rows = textwrap::wrap(&title_text, title_width)
            .into_iter()
            .map(|row| row.into_owned())
            .collect::<Vec<_>>();
        if title_rows.len() > 2 {
            title_rows.truncate(2);
            while Span::raw(&title_rows[1]).width() >= title_width {
                title_rows[1].pop();
            }
            title_rows[1].push('…');
        }
        let areas = Layout::vertical([
            Constraint::Length(title_rows.len() as u16 + 2),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(area);
        let mut title = Vec::new();
        for (index, row) in title_rows.iter().enumerate() {
            let mut line = Line::from("  ");
            if index == 0 && !saved.is_empty() {
                line.push_span(Span::styled(saved, theme.accent));
            }
            line.push_span(Span::styled(
                if index == 0 {
                    row.strip_prefix(saved).unwrap_or(row)
                } else {
                    row
                }
                .to_owned(),
                theme.fg.add_modifier(Modifier::BOLD),
            ));
            title.push(line);
        }
        let chip = reading.status_chip(&self.config, self.auth_failed, tick);
        let chip_width = chip.width();
        let meta_width = width.saturating_sub(4 + chip_width + usize::from(chip_width > 0));
        let metadata = format!(
            "{} points · {} comments",
            reading.story.score,
            reading.story.comments.unwrap_or(0)
        );
        let mut metadata = if area.width < 60 {
            metadata
        } else {
            format!("{host} · {metadata}")
        };
        if Span::raw(&metadata).width() > meta_width {
            while Span::raw(&metadata).width() >= meta_width && !metadata.is_empty() {
                metadata.pop();
            }
            metadata.push('…');
        }
        title.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                if meta_width > 0 {
                    metadata
                } else {
                    String::new()
                },
                theme.dim,
            ),
        ]));
        frame.render_widget(
            Paragraph::new(title).block(
                Block::default()
                    .borders(Borders::BOTTOM)
                    .border_style(theme.line),
            ),
            areas[0],
        );
        if !saved.is_empty() {
            let star = Rect::new(area.x + 2, areas[0].y, 1, 1).intersection(areas[0]);
            app.render_save_flash(frame.buffer_mut(), star, reading.story.id);
        }
        let chip_x = area.width.saturating_sub(2 + chip_width as u16);
        frame.render_widget(
            Paragraph::new(chip),
            Rect::new(
                area.x + chip_x,
                areas[0].y + title_rows.len() as u16,
                area.width.saturating_sub(chip_x + 2),
                1,
            ),
        );
        reading.page_rows = usize::from(areas[1].height);
        if let Some(id) = reading.source {
            let lines = reading.source_lines(id, width);
            reading.source_scroll = reading
                .source_scroll
                .min(lines.len().saturating_sub(reading.page_rows));
            frame.render_widget(
                Paragraph::new(
                    lines
                        .into_iter()
                        .skip(reading.source_scroll)
                        .collect::<Vec<_>>(),
                ),
                areas[1],
            );
        } else {
            let comments_area = reading.render_summary(frame, areas[1]);
            let mut lines = vec![];
            if !reading.story.text.is_empty() {
                append(&mut lines, "POST", width.saturating_sub(2), theme.dim);
                append(
                    &mut lines,
                    &reading.story.text,
                    width.saturating_sub(2),
                    theme.fg,
                );
                for line in &mut lines {
                    line.spans.insert(0, Span::raw("  "));
                }
            }
            let mut heading = Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    format!("COMMENTS  {} ", app.visible_comment_count()),
                    theme.dim,
                ),
            ]);
            heading.push_span(Span::styled(
                "─".repeat(width.saturating_sub(heading.width())),
                theme.line,
            ));
            lines.push(heading);
            lines.push(Line::raw(""));
            let old = reading.prefix_height;
            if old > 0 && app.comment_scroll >= old {
                app.comment_scroll = app
                    .comment_scroll
                    .saturating_sub(old)
                    .saturating_add(lines.len());
            }
            reading.prefix_height = lines.len();
            hn_lib::ui::comments::render_reader_content(
                frame,
                comments_area,
                app,
                tick,
                lines,
                !reading.citation_focus,
            );
        }
        let hints = if reading.source.is_some() {
            vec![
                ("Tab", "next"),
                ("Shift-Tab", "previous"),
                ("PgUp/PgDn", "scroll"),
                ("Esc", "back"),
            ]
        } else if reading.citation_focus {
            vec![
                ("Tab", "next source"),
                ("Enter", "open"),
                ("S", "summary"),
                ("Esc", "feed"),
            ]
        } else {
            let mut hints = vec![("j/k", "comments")];
            if !reading.citations().is_empty() {
                hints.push(("Tab", "sources"));
            }
            hints.extend([
                ("S", "summary"),
                ("Esc", "feed"),
                ("?", "keys"),
                ("o", "article"),
                ("s", "save"),
                ("M", "settings"),
                ("L", "links"),
            ]);
            hints
        };
        // A rejected key is already reported by the status chip.
        let browser_visible = app.error().is_none()
            && app.bookmark_notice.is_none()
            && reading.staged.is_none()
            && !self.browser_notice.is_empty();
        let notice = app
            .error()
            .or(app.bookmark_notice.as_deref())
            .map(|s| (s, theme.error))
            .or_else(|| {
                reading
                    .staged
                    .as_ref()
                    .map(|_| ("Summary updated", theme.accent))
            })
            .or_else(|| {
                (!self.browser_notice.is_empty()).then_some((
                    self.browser_notice.as_str(),
                    if !theme.motion && !self.browser_busy {
                        theme.dim
                    } else {
                        theme.accent
                    },
                ))
            });
        frame.render_widget(
            Paragraph::new(hn_lib::ui::widgets::hint_line(
                theme, &hints, notice, area.width,
            ))
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(theme.line),
            ),
            areas[2],
        );
        if browser_visible && !self.browser_busy && theme.motion {
            if let Some(elapsed) = self
                .notice_started
                .map(|start| start.elapsed())
                .filter(|elapsed| *elapsed >= Duration::from_secs(2))
            {
                let cells = Rect::new(
                    area.x + 2,
                    areas[2].y + 1,
                    Span::raw(&self.browser_notice)
                        .width()
                        .min(usize::from(area.width.saturating_sub(2))) as u16,
                    1,
                )
                .intersection(areas[2]);
                fx::fade_to_fg(theme.dim.fg.unwrap(), (400, Interpolation::Linear)).process(
                    elapsed.saturating_sub(Duration::from_secs(2)).into(),
                    frame.buffer_mut(),
                    cells,
                );
            }
        }
        if app.show_help {
            hn_lib::ui::widgets::render_help(frame, theme, true, app.overlay_opened);
        }
        if app.show_links {
            hn_lib::ui::links::render(frame, app);
        }
        self.render_settings(frame);
    }
}

impl Reading {
    pub(super) fn has_active_motion(&self, now: Instant) -> bool {
        self.theme.motion
            && self.shown
            && self.source.is_none()
            && self.panel_motion.is_some_and(|(start, from, to)| {
                from < to && now.saturating_duration_since(start) < Duration::from_millis(250)
            })
    }

    pub(super) fn render_summary(&mut self, frame: &mut Frame, area: Rect) -> Rect {
        let theme = self.theme;
        let width = usize::from(area.width);
        let (summary, target) = self.summary_lines(width);
        // Show the full result when it fits; leave at least a third for comments.
        let desired =
            if self.shown && (self.article.result.is_some() || self.discussion.result.is_some()) {
                (summary.len() + 1).min(usize::from(area.height) * 2 / 3)
            } else {
                0
            };
        let height = if self.moved && desired > 0 {
            self.growth_cap
                .unwrap_or(desired)
                .min(usize::from(area.height) * 2 / 3)
        } else {
            desired
        };
        if !self.moved && self.growth_cap.is_some() {
            self.growth_cap = Some(height);
        }
        let target_height = height as u16;
        let now = Instant::now();
        let drawn_height = |(start, from, to): (Instant, u16, u16)| {
            let progress = (now.saturating_duration_since(start).as_secs_f32() / 0.180).min(1.0);
            from + (f32::from(to - from) * Interpolation::QuadOut.alpha(progress)) as u16
        };
        let previous = self.panel_motion.map_or(0, |(_, _, to)| to);
        if !theme.motion {
            self.panel_motion = None;
        } else if target_height != previous {
            let from = if target_height > previous {
                self.panel_motion.map_or(0, drawn_height)
            } else {
                target_height // Shrinking snaps, including during an unfinished opening.
            };
            self.panel_motion = Some((now, from, target_height));
        }
        let height = self.panel_motion.map_or(target_height, drawn_height);
        // Scroll/focus resolve against the final layout immediately, never animation rows.
        if target_height > 0 {
            if self.reveal_citation {
                self.summary_scroll = target.unwrap_or(0);
                self.reveal_citation = false;
            }
            self.summary_scroll = self.summary_scroll.min(
                summary
                    .len()
                    .saturating_sub(usize::from(target_height.saturating_sub(1))),
            );
        }
        let body = Layout::vertical([Constraint::Length(height), Constraint::Min(0)]).split(area);
        if height > 0 {
            let rows = usize::from(height.saturating_sub(1));
            let end = (self.summary_scroll + rows).min(summary.len());
            let footer = if summary.len() > rows {
                Line::from(vec![
                    Span::raw("  "),
                    Span::styled("│ ", theme.line),
                    Span::styled(
                        format!(
                            "J/K scroll · {}–{end} of {}",
                            self.summary_scroll + 1,
                            summary.len()
                        ),
                        theme.dim,
                    ),
                ])
            } else {
                Line::raw("")
            };
            let mut visible = summary
                .into_iter()
                .skip(self.summary_scroll)
                .take(rows)
                .collect::<Vec<_>>();
            visible.push(footer);
            let active_rows = visible
                .iter()
                .enumerate()
                .filter_map(|(i, line)| {
                    line.spans
                        .iter()
                        .any(|span| span.content == "┃ ")
                        .then_some(i as u16)
                })
                .collect::<Vec<_>>();
            frame.render_widget(Paragraph::new(visible), body[0]);
            for row in active_rows {
                frame.buffer_mut().set_style(
                    Rect::new(
                        body[0].x + 2,
                        body[0].y + row,
                        body[0].width.saturating_sub(2),
                        1,
                    ),
                    theme.sel,
                );
            }
        }
        if theme.motion {
            if let Some((start, _, _)) = self.panel_motion.filter(|(_, from, to)| from < to) {
                fx::fade_from_fg(theme.dim.fg.unwrap(), (250, Interpolation::QuadOut)).process(
                    start.elapsed().into(),
                    frame.buffer_mut(),
                    body[0],
                );
            }
        }
        body[1]
    }

    fn status_chip(&self, config: &Config, auth_failed: bool, tick: usize) -> Line<'static> {
        let theme = self.theme;
        let ready = self.article.result.is_some() as u8 + self.discussion.result.is_some() as u8;
        let loading = self.article.pending() || self.discussion.pending();
        if let Some(id) = self.source {
            let result = self.discussion.result.as_ref().unwrap();
            let author = &result
                .capture
                .input
                .sources
                .iter()
                .find(|s| s.id == id)
                .unwrap()
                .author;
            return Line::styled(
                format!(
                    "source {} of {} · {author} · {}",
                    self.citation + 1,
                    self.citations().len(),
                    result.profile.label()
                ),
                theme.dim,
            );
        }
        if config.key().is_empty() {
            return Line::default();
        }
        if auth_failed {
            return Line::styled(
                format!("! {} key rejected · M edit", config.provider.label()),
                theme.error,
            );
        }
        if self.article.failed() || self.discussion.failed() {
            return Line::styled("! summary incomplete · M retry", theme.error);
        }
        if loading {
            let profile = [&self.article.progress, &self.discussion.progress]
                .into_iter()
                .find_map(|p| match p {
                    Progress::Preparing(_, _, profile) => Some(*profile),
                    Progress::Queued(a) | Progress::Running(a) => Some(a.profile),
                    _ => None,
                })
                .unwrap();
            let state = match ready {
                2 => "updating",
                1 => "1/2 ready",
                _ => "summarizing",
            };
            return Line::styled(
                format!(
                    "{} {state} · {}",
                    hn_lib::ui::widgets::spinner_frame(tick),
                    profile.label()
                ),
                theme.dim,
            );
        }
        if ready == 0 && (!self.article.requested() || !self.discussion.requested()) {
            return Line::from(vec![
                Span::styled("S", theme.accent),
                Span::styled(" summarize", theme.dim),
            ]);
        }
        if ready != 2 {
            return Line::styled("! summary incomplete · M retry", theme.error);
        }
        let article = self.article.label();
        let discussion = self.discussion.label();
        let profiles = if article == discussion {
            article
        } else {
            format!("Article {article} · Discussion {discussion}")
        };
        let mut chip = Line::from(vec![
            Span::styled("✓", theme.accent),
            Span::styled(format!(" summary · {profiles}"), theme.dim),
        ]);
        if !self.shown || (self.moved && self.growth_cap == Some(0)) {
            chip.extend([
                Span::styled(" · ", theme.dim),
                Span::styled("S", theme.accent),
                Span::styled(" show", theme.dim),
            ]);
        }
        chip
    }
}
