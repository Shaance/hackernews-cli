//! Reusable UI widgets

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::app::App;

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

/// Render help overlay
pub fn render_help(f: &mut Frame, area: Rect, in_comments: bool) {
    let help_text = if in_comments {
        vec![
            Line::from("Comments"),
            Line::from("j/k/↑/↓  Select comment"),
            Line::from("PgUp/PgDn  Read text"),
            Line::from("Enter/l/→  Replies · c Collapse"),
            Line::from("[/] Siblings · u Parent"),
            Line::from("o Article · O Browser · L Links"),
            Line::from("S Summary · J/K Scroll · M Settings"),
            Line::from("Tab Sources · Enter Open · Esc Back"),
            Line::from("s Save/unsave · r Refresh comments"),
            Line::from("Esc/q/h/← Back · ? Close help"),
        ]
    } else {
        vec![
            Line::from("Stories"),
            Line::from("j/k/↑/↓  Select story"),
            Line::from("n/p/→/←  Page"),
            Line::from("1 Top · 2 New · 3 Best · 4 Saved"),
            Line::from("Enter/o Read · O Browser"),
            Line::from("c Comments · s Save/unsave"),
            Line::from("r Refresh/retry"),
            Line::from("q/Esc Quit · ? Close help"),
        ]
    };

    let block = Block::default().title(" Help ").borders(Borders::ALL);

    let height = (help_text.len() as u16 + 2).min(area.height);
    let width = 74.min(area.width);
    let paragraph = Paragraph::new(help_text)
        .block(block)
        .wrap(Wrap { trim: true });

    let dialog = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    f.render_widget(Clear, dialog);
    f.render_widget(paragraph, dialog);
}

/// Render status bar for stories view
pub fn render_stories_status(area: Rect, app: &App, _tick: usize) -> Paragraph<'static> {
    let theme = app.theme();
    let mut line = Line::from("  ");
    let notice = app
        .bookmark_notice
        .as_ref()
        .map(|notice| format!("Saved: 4 then r retry · {notice}"));
    if let Some(message) = notice.as_deref().or(app.error()) {
        line.push_span(Span::styled(message.to_owned(), theme.error));
    }
    for (key, label) in [
        ("j/k", "move"),
        ("Enter", "read"),
        ("1–4", "feeds"),
        ("s", "save"),
        ("?", "keys"),
        ("n/p", "page"),
        ("c", "discussion"),
        ("q", "quit"),
    ] {
        let separator = if line.width() > 2 { "   " } else { "" };
        let key = Span::styled(key, theme.accent);
        let label = Span::styled(format!(" {label}"), theme.dim);
        if line.width() + separator.len() + key.width() + label.width()
            > usize::from(area.width).saturating_sub(2)
        {
            break;
        }
        line.push_span(Span::raw(separator));
        line.push_span(key);
        line.push_span(label);
    }
    Paragraph::new(line).block(
        Block::default()
            .borders(Borders::TOP)
            .border_style(theme.line),
    )
}

/// Render status bar for comments view
pub fn render_comments_status(area: Rect, app: &App, tick: usize) -> Paragraph<'static> {
    if let Some(notice) = &app.bookmark_notice {
        return Paragraph::new(format!(" Saved: Esc, 4 then r retry · {notice}"))
            .style(app.theme().error)
            .block(Block::default().borders(Borders::TOP));
    }
    let mut segments = Vec::new();
    if let Some(error) = app.error() {
        segments.push(Span::styled(error.to_string(), app.theme().error));
        segments.push(Span::raw(" │ "));
    }

    segments.push(Span::raw(if area.width < 80 {
        " ? help · PgUp/Dn · L links · s save"
    } else {
        " j/k nav · PgUp/PgDn read · Enter replies · L links · s save · Esc back · ? help"
    }));

    if app.should_show_loading() {
        segments.push(Span::raw(" │ "));
        segments.push(Span::styled(
            format!("{} loading comments", spinner_frame(tick)),
            app.theme().dim,
        ));
    }

    Paragraph::new(Line::from(segments))
        .style(Style::default().add_modifier(Modifier::DIM))
        .block(Block::default().borders(Borders::TOP))
}
