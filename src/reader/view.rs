use super::{text::append, Reader};
use hn_lib::app::App;
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

impl Reader {
    pub fn render(&mut self, frame: &mut Frame, app: &mut App, tick: usize) {
        if self.settings.is_some() {
            self.render_settings(frame);
            return;
        }
        let Some(reading) = self.active.and_then(|id| self.readings.get_mut(&id)) else {
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
        let title_width = usize::from(frame.area().width.saturating_sub(1)).max(1);
        let title_text = format!("{saved}{}", reading.story.title);
        let mut title_rows = textwrap::wrap(&title_text, title_width)
            .into_iter()
            .map(|row| row.into_owned())
            .collect::<Vec<_>>();
        if title_rows.len() > 2 {
            title_rows.truncate(2);
            title_rows[1].push('…');
        }
        let metadata = format!(
            "{} points · {} comments",
            reading.story.score,
            reading.story.comments.unwrap_or(0)
        );
        title_rows.push(if frame.area().width < 60 {
            metadata
        } else {
            format!("{host} · {metadata}")
        });
        let areas = Layout::vertical([
            Constraint::Length(title_rows.len() as u16 + 1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(frame.area());
        let title = title_rows.join("\n");
        frame.render_widget(
            Paragraph::new(title)
                .wrap(Wrap { trim: false })
                .block(Block::default().borders(Borders::BOTTOM)),
            areas[0],
        );
        reading.page_rows = usize::from(areas[2].height);
        let width = usize::from(areas[2].width);
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
                areas[2],
            );
        } else {
            let (summary, target) = reading.summary_lines(width);
            // Show the full result when it fits; leave at least a third for comments.
            let height = if reading.shown
                && (reading.article.requested() || reading.discussion.requested())
            {
                (summary.len() + 1).min(usize::from(areas[2].height) * 2 / 3) as u16
            } else {
                0
            };
            let body =
                Layout::vertical([Constraint::Length(height), Constraint::Min(0)]).split(areas[2]);
            if height > 0 {
                let rows = usize::from(height.saturating_sub(1));
                if reading.reveal_citation {
                    reading.summary_scroll = target.unwrap_or(0);
                    reading.reveal_citation = false;
                }
                reading.summary_scroll = reading
                    .summary_scroll
                    .min(summary.len().saturating_sub(rows));
                let end = (reading.summary_scroll + rows).min(summary.len());
                let footer = if summary.len() > rows {
                    format!(
                        "╰─ J/K Summary · {}–{end}/{}",
                        reading.summary_scroll + 1,
                        summary.len()
                    )
                } else {
                    format!("╰{}", "─".repeat(width.saturating_sub(1)))
                };
                let mut visible = summary
                    .into_iter()
                    .skip(reading.summary_scroll)
                    .take(rows)
                    .collect::<Vec<_>>();
                visible.push(Line::styled(footer, Style::default().fg(Color::Yellow)));
                frame.render_widget(Paragraph::new(visible), body[0]);
            }
            let mut lines = vec![];
            if !reading.story.text.is_empty() {
                append(
                    &mut lines,
                    "ORIGINAL POST",
                    width,
                    Style::default().fg(Color::Yellow),
                );
                append(&mut lines, &reading.story.text, width, Style::default());
            }
            lines.push(Line::styled(
                format!("DISCUSSION · {} visible", app.visible_comment_count()),
                Style::default().fg(Color::Cyan),
            ));
            lines.push(Line::raw(""));
            let old = reading.prefix_height;
            if old > 0 && app.comment_scroll >= old {
                app.comment_scroll = app
                    .comment_scroll
                    .saturating_sub(old)
                    .saturating_add(lines.len());
            }
            reading.prefix_height = lines.len();
            hn_lib::ui::comments::render_reader_content(frame, body[1], app, tick, lines);
        }
        let viewing = reading.shown;
        let action = if viewing { "S Hide" } else { "S View" };
        let ready =
            reading.article.result.is_some() as u8 + reading.discussion.result.is_some() as u8;
        let loading = reading.article.pending() || reading.discussion.pending();
        let (label, color) = if reading.source.is_some() {
            ("Summary source · Esc Back".to_owned(), Color::Cyan)
        } else if self.config.key().is_empty() {
            ("AI summary · S Set up summaries".to_owned(), Color::Yellow)
        } else if self.auth_failed {
            (
                "OpenRouter key rejected · M Edit key".to_owned(),
                Color::Yellow,
            )
        } else if !reading.article.requested() || !reading.discussion.requested() {
            ("AI summary · S Generate".to_owned(), Color::Yellow)
        } else if loading {
            let state = if ready == 2 {
                "Updating…"
            } else if ready > 0 {
                "1/2 ready"
            } else {
                "Generating…"
            };
            (
                format!(
                    "{} {state} · {action}",
                    hn_lib::ui::widgets::spinner_frame(tick)
                ),
                Color::Yellow,
            )
        } else if ready == 2 && !reading.article.failed() && !reading.discussion.failed() {
            (format!("✓ Summary ready · {action}"), Color::Green)
        } else {
            (
                format!("Summary incomplete · {action} · r Retry"),
                Color::Yellow,
            )
        };
        frame.render_widget(
            Paragraph::new(format!(
                "{label} · {} · Auto {} · M Settings",
                self.config.profile.label(),
                if self.config.automatic { "on" } else { "off" }
            ))
            .style(Style::default().fg(color)),
            areas[1],
        );
        let status = if reading.source.is_some() {
            if reading.staged.is_some() {
                "Summary updated · Esc Back".to_owned()
            } else {
                "Esc Back to summary · PgUp/Dn Scroll".to_owned()
            }
        } else if let Some(error) = app.error() {
            error.to_owned()
        } else if let Some(notice) = &app.bookmark_notice {
            notice.clone()
        } else if reading.citation_focus {
            "Tab Next source · Enter Open · S Hide".to_owned()
        } else if !self.browser_notice.is_empty() {
            self.browser_notice.clone()
        } else {
            "j/k Navigate · Tab Sources · ? Help".to_owned()
        };
        frame.render_widget(
            Paragraph::new(vec![
                Line::raw("Esc Feed · s Save · o Article · O Browser · M Settings"),
                Line::raw(status),
            ]),
            areas[3],
        );
        if app.show_help {
            hn_lib::ui::widgets::render_help(frame, frame.area(), true);
        }
        if app.show_links {
            hn_lib::ui::links::render(frame, app);
        }
    }
}
