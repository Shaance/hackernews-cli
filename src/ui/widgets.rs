//! Reusable UI widgets

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::app::App;

/// ASCII spinner frames
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Get spinner frame for current tick
pub fn spinner_frame(tick: usize) -> &'static str {
    SPINNER_FRAMES[tick % SPINNER_FRAMES.len()]
}

/// Render a loading spinner with message
pub fn render_loading(f: &mut Frame, area: Rect, message: &str, tick: usize) {
    let spinner = spinner_frame(tick);
    let text = format!("{} {}", spinner, message);

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
    let text = format!("Error: {}", error);

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
            Line::from(vec![Span::styled(
                "Comments View",
                Style::default().add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from(vec![
                Span::styled("j/↓", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw("       Next comment"),
            ]),
            Line::from(vec![
                Span::styled("k/↑", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw("       Previous comment"),
            ]),
            Line::from(vec![
                Span::styled("g", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw("         Go to top"),
            ]),
            Line::from(vec![
                Span::styled("G", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw("         Go to bottom"),
            ]),
            Line::from(vec![
                Span::styled("Enter/l", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" Expand/collapse replies"),
            ]),
            Line::from(vec![
                Span::styled("c", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw("         Collapse thread"),
            ]),
            Line::from(vec![
                Span::styled("o", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw("         Open story URL"),
            ]),
            Line::from(vec![
                Span::styled("Esc/q", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw("     Back to stories"),
            ]),
            Line::from(vec![
                Span::styled("?", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw("         Toggle this help"),
            ]),
        ]
    } else {
        vec![
            Line::from(vec![Span::styled(
                "Stories View",
                Style::default().add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from(vec![
                Span::styled("j/↓", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw("       Next story"),
            ]),
            Line::from(vec![
                Span::styled("k/↑", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw("       Previous story"),
            ]),
            Line::from(vec![
                Span::styled("n/→", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw("       Next page"),
            ]),
            Line::from(vec![
                Span::styled("p/←", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw("       Previous page"),
            ]),
            Line::from(vec![
                Span::styled("1/2/3", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw("     1:Top  2:New  3:Best"),
            ]),
            Line::from(vec![
                Span::styled("Enter/o", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw("   Open story URL"),
            ]),
            Line::from(vec![
                Span::styled("c", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw("         View comments"),
            ]),
            Line::from(vec![
                Span::styled("r", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw("         Refresh"),
            ]),
            Line::from(vec![
                Span::styled("q", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw("         Quit"),
            ]),
            Line::from(vec![
                Span::styled("?", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw("         Toggle this help"),
            ]),
        ]
    };

    let block = Block::default().title(" Help ").borders(Borders::ALL);

    let paragraph = Paragraph::new(help_text)
        .block(block)
        .wrap(Wrap { trim: true });

    // Center the help dialog
    let vertical_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Min(15),
            Constraint::Percentage(20),
        ])
        .split(area);

    let horizontal_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Min(50),
            Constraint::Percentage(20),
        ])
        .split(vertical_layout[1]);

    // Clear the area underneath so text isn't visible through the help overlay
    f.render_widget(Clear, horizontal_layout[1]);
    f.render_widget(paragraph, horizontal_layout[1]);
}

/// Render status bar for stories view
pub fn render_stories_status(_area: Rect, app: &App, tick: usize) -> Paragraph<'static> {
    let (display_type, display_page) = app.displayed_story_context();
    let stale = app.showing_stale_stories();

    let mut segments = vec![
        Span::raw(" j/k navigate "),
        Span::raw("│ "),
        Span::raw("n/p page "),
        Span::raw("│ "),
        Span::raw("1:Top 2:New 3:Best "),
        Span::raw("│ "),
        Span::raw("o open "),
        Span::raw("│ "),
        Span::raw("c comments "),
        Span::raw("│ "),
        Span::raw("?:help "),
        Span::raw("│ "),
        Span::raw("q quit "),
        Span::raw("│ "),
        Span::styled(
            format!(
                "showing {} · p{}",
                display_type.display_name(),
                display_page
            ),
            Style::default().fg(Color::Yellow),
        ),
    ];

    if stale {
        segments.push(Span::raw(" → "));
        segments.push(Span::styled(
            format!(
                "fetching {} · p{}",
                app.story_type.display_name(),
                app.current_page
            ),
            Style::default().fg(Color::Cyan),
        ));
    }

    if app.should_show_loading() {
        segments.push(Span::raw(" │ "));
        segments.push(Span::styled(
            format!(
                "{} {}",
                spinner_frame(tick),
                if stale { "updating" } else { "loading" }
            ),
            Style::default().fg(Color::Blue),
        ));
    }

    Paragraph::new(Line::from(segments))
        .style(Style::default().add_modifier(Modifier::DIM))
        .block(Block::default().borders(Borders::TOP))
}

/// Render status bar for comments view
pub fn render_comments_status(_area: Rect, app: &App, tick: usize) -> Paragraph<'static> {
    let mut segments = vec![
        Span::raw(" j/k scroll "),
        Span::raw("│ "),
        Span::raw("Enter/l expand "),
        Span::raw("│ "),
        Span::raw("c collapse thread "),
        Span::raw("│ "),
        Span::raw("o open "),
        Span::raw("│ "),
        Span::raw("Esc back "),
        Span::raw("│ "),
        Span::raw("?:help"),
    ];

    if app.should_show_loading() {
        segments.push(Span::raw(" │ "));
        segments.push(Span::styled(
            format!("{} loading comments", spinner_frame(tick)),
            Style::default().fg(Color::Blue),
        ));
    }

    Paragraph::new(Line::from(segments))
        .style(Style::default().add_modifier(Modifier::DIM))
        .block(Block::default().borders(Borders::TOP))
}
