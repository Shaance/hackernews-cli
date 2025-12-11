//! Stories list view rendering

use crate::app::App;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use super::widgets;

/// Render the stories view
pub fn render(f: &mut Frame, app: &mut App, tick: usize) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Title bar
            Constraint::Min(0),    // Stories list
            Constraint::Length(2), // Status bar
        ])
        .split(f.area());

    render_title(f, chunks[0], app, tick);

    if app.loading && app.stories.is_empty() {
        widgets::render_loading(f, chunks[1], "Loading stories...", tick);
    } else if let Some(error) = &app.error {
        widgets::render_error(f, chunks[1], error);
    } else if app.stories.is_empty() {
        widgets::render_error(f, chunks[1], "No stories found");
    } else {
        render_stories_list(f, chunks[1], app);
    }

    let status = widgets::render_stories_status(chunks[2], app, tick);
    f.render_widget(status, chunks[2]);

    // Render help overlay if shown
    if app.show_help {
        widgets::render_help(f, f.area(), false);
    }
}

/// Render title bar with current story type and page
fn render_title(f: &mut Frame, area: Rect, app: &App, tick: usize) {
    let (display_type, display_page) = app.displayed_story_context();
    let stale = app.showing_stale_stories();

    let mut spans = vec![
        Span::raw(" HN: "),
        Span::styled(
            format!("{} stories", display_type.display_name()),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(" │ Page {} ", display_page)),
    ];

    if stale {
        spans.push(Span::raw("│ "));
        spans.push(Span::styled(
            format!(
                "fetching {} p{}",
                app.story_type.display_name(),
                app.current_page
            ),
            Style::default().fg(Color::Cyan),
        ));
    }

    if app.should_show_loading() {
        spans.push(Span::raw(format!(
            " {} {}",
            widgets::spinner_frame(tick),
            if stale { "Updating…" } else { "Loading…" }
        )));
    }

    let title = Paragraph::new(Line::from(spans))
        .style(Style::default())
        .block(Block::default().borders(Borders::BOTTOM));

    f.render_widget(title, area);
}

/// Render the list of stories
fn render_stories_list(f: &mut Frame, area: Rect, app: &mut App) {
    // Keep selection in view
    app.update_story_scroll(area.height as usize);

    let (_, display_page) = app.displayed_story_context();
    let list_style = if app.loading {
        Style::default().fg(Color::Gray).add_modifier(Modifier::DIM)
    } else {
        Style::default()
    };

    let items: Vec<ListItem> = app
        .stories
        .iter()
        .enumerate()
        .map(|(idx, story)| {
            let is_selected = idx == app.selected_index;
            let global_idx = ((display_page - 1) as usize * app.page_size as usize) + idx + 1;

            // Build the story display
            let mut lines = vec![];

            // First line: selection indicator + number + title
            let indicator = if is_selected {
                Span::styled("▸ ", Style::default().fg(Color::Yellow))
            } else {
                Span::raw("  ")
            };
            let title_style = if is_selected {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            lines.push(Line::from(vec![
                indicator,
                Span::styled(format!("{}. ", global_idx), title_style),
                Span::styled(&story.title, title_style),
            ]));

            // Second line: metadata
            let comment_str = match story.comments {
                Some(n) if n == 1 => "1 comment".to_string(),
                Some(n) => format!("{} comments", n),
                None => "discuss".to_string(),
            };

            lines.push(Line::from(vec![
                Span::raw("     "),
                Span::styled("by ", Style::default().add_modifier(Modifier::DIM)),
                Span::styled(&story.author, Style::default().fg(Color::Cyan)),
                Span::raw(" │ "),
                Span::styled(
                    format!("{} points", story.score),
                    Style::default().fg(Color::Green),
                ),
                Span::raw(" │ "),
                Span::styled(comment_str, Style::default().fg(Color::Yellow)),
                Span::raw(" │ "),
                Span::styled(
                    &story.time_ago,
                    Style::default().add_modifier(Modifier::DIM),
                ),
            ]));

            // Add spacing between stories
            if idx < app.stories.len() - 1 {
                lines.push(Line::from(""));
            }

            ListItem::new(lines)
        })
        .collect();

    let list = List::new(items)
        .style(list_style)
        .block(Block::default().borders(Borders::NONE))
        .highlight_style(
            Style::default()
                .add_modifier(Modifier::BOLD | Modifier::REVERSED)
                .fg(Color::Reset),
        );

    let mut state = ListState::default()
        .with_selected(Some(app.selected_index))
        .with_offset(app.story_scroll);

    f.render_stateful_widget(list, area, &mut state);
}
