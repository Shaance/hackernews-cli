//! Reusable UI widgets

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Padding, Paragraph, Wrap},
    Frame,
};

use crate::{app::App, theme::Theme};

/// ASCII spinner frames
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Get the spinner frame for elapsed 80 ms intervals supplied by the frame clock.
pub fn spinner_frame(tick: usize) -> &'static str {
    SPINNER_FRAMES[tick % SPINNER_FRAMES.len()]
}

/// Render a loading spinner with message
pub fn render_loading(f: &mut Frame, area: Rect, message: &str, tick: usize) {
    let spinner = spinner_frame(tick);
    let text = format!("{spinner} {message}");

    let paragraph = Paragraph::new(text)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::NONE));

    // Center the loading message
    let vertical_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(45),
            Constraint::Length(1),
            Constraint::Percentage(45),
        ])
        .split(area);

    f.render_widget(paragraph, vertical_layout[1]);
}

/// Render an error message
pub fn render_error(f: &mut Frame, area: Rect, error: &str) {
    let text = format!("Error: {error}");

    let paragraph = Paragraph::new(text)
        .style(Style::default().add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });

    // Center the error message
    let vertical_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(40),
            Constraint::Length(3),
            Constraint::Percentage(40),
        ])
        .split(area);

    f.render_widget(paragraph, vertical_layout[1]);
}

/// Dim the frame and render a padded, content-sized modal with an optional selected row.
/// The body receives the available content width and height, excluding padding and borders.
pub fn render_modal(
    f: &mut Frame,
    theme: Theme,
    title: &str,
    body: impl FnOnce(u16, u16) -> (Vec<Line<'static>>, Option<usize>),
) {
    for cell in &mut f.buffer_mut().content {
        cell.set_style(Style::reset().patch(theme.scrim).bg(Color::Reset));
    }
    let area = f.area().inner(Margin::new(2, 1));
    let (mut lines, selected) = body(area.width.saturating_sub(6), area.height.saturating_sub(4));
    lines.truncate(usize::from(area.height.saturating_sub(4)));
    let width = lines
        .iter()
        .map(Line::width)
        .max()
        .unwrap_or(0)
        .min(usize::from(area.width.saturating_sub(6)));
    let selected = selected.filter(|index| *index < lines.len());
    let height = lines.len() + 2;
    let body = tui_popup::KnownSizeWrapper::new(
        Paragraph::new(lines).block(Block::default().padding(Padding::new(2, 2, 1, 1))),
        width + 4,
        height,
    );
    let popup = tui_popup::Popup::new(body)
        .style(theme.fg)
        .border_set(ratatui::symbols::border::ROUNDED)
        .border_style(theme.line)
        .title(Line::styled(
            format!(" {title} "),
            theme.accent.add_modifier(Modifier::BOLD),
        ));
    let mut state = tui_popup::PopupState::default();
    f.render_stateful_widget(popup, area, &mut state);
    if let (Some(index), Some(dialog)) = (selected, state.area()) {
        let row = Rect::new(
            dialog.x + 1,
            dialog.y + 2 + index as u16,
            dialog.width.saturating_sub(2),
            1,
        );
        let body = dialog.inner(Margin::new(1, 1));
        let row = row.intersection(body);
        f.buffer_mut().set_style(row, theme.sel);
        if !row.is_empty() {
            f.buffer_mut()[(row.x, row.y)]
                .set_symbol("▎")
                .set_style(theme.accent);
        }
    }
}

/// Render the key groups shared by the comments and reader views.
pub fn render_help(f: &mut Frame, theme: Theme, in_comments: bool) {
    let groups: &[(&str, &[(&str, &str)])] = if in_comments {
        &[
            (
                "READING",
                &[
                    ("j/k/↑/↓", "Select comment"),
                    ("PgUp/PgDn", "Read text"),
                    ("Enter/l/→", "Replies"),
                    ("c", "Collapse"),
                    ("[/]", "Siblings"),
                    ("u", "Parent"),
                    ("g/G", "First/last comment"),
                ],
            ),
            (
                "SUMMARY",
                &[
                    ("S", "Summary"),
                    ("J/K", "Scroll"),
                    ("M", "Settings"),
                    ("Tab/Shift-Tab", "Sources"),
                    ("Enter", "Open"),
                    ("Esc", "Back"),
                ],
            ),
            (
                "STORY",
                &[
                    ("o", "Article"),
                    ("O", "Browser"),
                    ("L", "Links"),
                    ("s", "Save/unsave"),
                    ("r", "Refresh comments"),
                    ("Esc/q/h/←", "Back"),
                    ("?", "Close"),
                ],
            ),
        ]
    } else {
        &[
            (
                "STORIES",
                &[
                    ("j/k/↑/↓", "Select story"),
                    ("n/p/→/←", "Page"),
                    ("1", "Top"),
                    ("2", "New"),
                    ("3", "Best"),
                    ("4", "Saved"),
                    ("Enter/o", "Read"),
                    ("O", "Browser"),
                    ("c", "Comments"),
                    ("s", "Save/unsave"),
                    ("r", "Refresh/retry"),
                ],
            ),
            ("ANYWHERE", &[("q/Esc", "Quit"), ("?", "Close")]),
        ]
    };
    render_modal(f, theme, "Keys", |width, _| {
        // The body includes the two padding columns on each side.
        let columns = if width + 4 >= 72 { 2 } else { 1 };
        let key_width = groups
            .iter()
            .flat_map(|(_, entries)| entries.iter())
            .map(|(key, _)| Span::raw(*key).width())
            .max()
            .unwrap_or(0);
        let entry_width = groups
            .iter()
            .flat_map(|(_, entries)| entries.iter())
            .map(|(_, action)| key_width + 2 + Span::raw(*action).width())
            .max()
            .unwrap_or(0);
        let mut lines = Vec::new();
        for (heading, entries) in groups {
            if !lines.is_empty() {
                lines.push(Line::raw(""));
            }
            lines.push(Line::styled(*heading, theme.dim));
            for pair in entries.chunks(columns) {
                let mut line = Line::default();
                for (index, (key, action)) in pair.iter().enumerate() {
                    if index > 0 {
                        line.push_span(Span::raw(" ".repeat(entry_width + 2 - line.width())));
                    }
                    line.push_span(Span::raw(" ".repeat(key_width - Span::raw(*key).width())));
                    line.push_span(Span::styled(*key, theme.accent));
                    line.push_span(Span::raw("  "));
                    line.push_span(Span::styled(*action, theme.fg));
                }
                lines.push(line);
            }
        }
        (lines, None)
    });
}

/// Fit complete key hints in priority order, leaving two columns at either edge.
pub fn hint_line(
    theme: crate::theme::Theme,
    hints: &[(&str, &str)],
    notice: Option<(&str, Style)>,
    width: u16,
) -> Line<'static> {
    let mut line = Line::from("  ");
    if let Some((message, style)) = notice {
        line.push_span(Span::styled(message.to_owned(), style));
    }
    for (key, label) in hints {
        let separator = if line.width() > 2 { "   " } else { "" };
        let key = Span::styled((*key).to_owned(), theme.accent);
        let label = Span::styled(format!(" {label}"), theme.dim);
        if line.width() + separator.len() + key.width() + label.width()
            > usize::from(width).saturating_sub(2)
        {
            break;
        }
        line.push_span(Span::raw(separator));
        line.push_span(key);
        line.push_span(label);
    }
    line
}

/// Render status bar for stories view
pub fn render_stories_status(area: Rect, app: &App, _tick: usize) -> Paragraph<'static> {
    let theme = app.theme();
    let notice = app
        .bookmark_notice
        .as_ref()
        .map(|notice| format!("Saved: 4 then r retry · {notice}"));
    let line = hint_line(
        theme,
        &[
            ("j/k", "move"),
            ("Enter", "read"),
            ("1–4", "feeds"),
            ("s", "save"),
            ("?", "keys"),
            ("n/p", "page"),
            ("c", "discussion"),
            ("q", "quit"),
        ],
        notice.as_deref().or(app.error()).map(|s| (s, theme.error)),
        area.width,
    );
    Paragraph::new(line).block(
        Block::default()
            .borders(Borders::TOP)
            .border_style(theme.line),
    )
}

/// Render status bar for comments view
pub fn render_comments_status(area: Rect, app: &App, tick: usize) -> Paragraph<'static> {
    let theme = app.theme();
    let notice = app
        .bookmark_notice
        .as_ref()
        .map(|notice| format!("Saved: Esc, 4 then r retry · {notice}"));
    let loading = app
        .should_show_loading()
        .then(|| format!("{} loading comments", spinner_frame(tick)));
    let notice = notice
        .as_deref()
        .or(app.error())
        .map(|s| (s, theme.error))
        .or_else(|| loading.as_deref().map(|s| (s, theme.dim)));
    Paragraph::new(hint_line(
        theme,
        &[
            ("?", "help"),
            ("j/k", "nav"),
            ("PgUp/PgDn", "read"),
            ("Enter", "replies"),
            ("L", "links"),
            ("s", "save"),
            ("Esc", "back"),
        ],
        notice,
        area.width,
    ))
    .block(
        Block::default()
            .borders(Borders::TOP)
            .border_style(theme.line),
    )
}
