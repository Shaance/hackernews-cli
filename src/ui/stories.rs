//! Stories list view rendering

use crate::app::{App, StoryType};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
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
            Constraint::Length(2), // Title and divider
            Constraint::Min(0),    // Stories list
            Constraint::Length(2), // Status bar
        ])
        .split(f.area());

    render_title(f, chunks[0], app, tick);

    if !app.stories.is_empty() {
        render_stories_list(f, chunks[1], app);
    } else {
        let theme = app.theme();
        let mut lines = if app.is_loading() {
            vec![Line::styled(
                format!("{} Loading stories…", widgets::spinner_frame(tick)),
                theme.dim,
            )]
        } else if let Some(error) = app.error() {
            let message = format!("! {error}");
            let mut rows: Vec<_> = textwrap::wrap(&message, usize::from(chunks[1].width).max(1))
                .into_iter()
                .enumerate()
                .map(|(index, row)| {
                    if index == 0 {
                        Line::from(vec![
                            Span::styled("! ", theme.error),
                            Span::styled(row.strip_prefix("! ").unwrap_or("").to_owned(), theme.fg),
                        ])
                    } else {
                        Line::styled(row.into_owned(), theme.fg)
                    }
                })
                .collect();
            rows.push(Line::from(vec![
                Span::styled("r", theme.accent),
                Span::styled(" retry", theme.dim),
            ]));
            rows
        } else if app.story_type == StoryType::Saved {
            vec![
                Line::styled("No saved stories yet", theme.fg),
                Line::styled("Press s on any story to save it", theme.dim),
            ]
        } else {
            vec![
                Line::styled("No stories found", theme.fg),
                Line::from(vec![
                    Span::styled("r", theme.accent),
                    Span::styled(" refresh", theme.dim),
                ]),
            ]
        };
        let height = (lines.len() as u16).min(chunks[1].height);
        lines.truncate(usize::from(height));
        let area = Rect::new(
            chunks[1].x,
            chunks[1].y + (chunks[1].height - height) / 2,
            chunks[1].width,
            height,
        );
        f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), area);
    }

    let status = widgets::render_stories_status(chunks[2], app, tick);
    f.render_widget(status, chunks[2]);

    // Render help overlay if shown
    if app.show_help {
        widgets::render_help(f, app.theme(), false);
    }
}

/// Render title bar with current story type and page
fn render_title(f: &mut Frame, area: Rect, app: &App, tick: usize) {
    let (display_type, display_page) = app.displayed_story_context();
    let theme = app.theme();
    let mut tabs = vec![
        Span::raw(" "),
        Span::styled(" Y ", theme.brand),
        Span::raw("  "),
    ];
    for (idx, story_type) in [
        StoryType::Top,
        StoryType::New,
        StoryType::Best,
        StoryType::Saved,
    ]
    .iter()
    .enumerate()
    {
        if idx > 0 {
            tabs.push(Span::raw("   "));
        }
        let active = *story_type == display_type;
        tabs.push(Span::styled(
            format!("{} ", idx + 1),
            if active { theme.accent } else { theme.dim },
        ));
        tabs.push(Span::styled(
            story_type.display_name(),
            if active {
                theme
                    .accent
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else {
                theme.dim
            },
        ));
    }
    let stale = app.showing_stale_stories();
    let mut status = Vec::new();
    if app.should_show_loading() {
        status.push(Span::styled(
            format!(
                "{} {}  ",
                widgets::spinner_frame(tick),
                if stale { "updating" } else { "loading" }
            ),
            theme.dim,
        ));
    }
    if stale {
        let failed = app.error().is_some() && !app.is_loading();
        status.push(Span::styled(
            format!(
                "{} {} p{}  ",
                if failed { "failed" } else { "fetching" },
                app.story_type.display_name(),
                app.current_page
            ),
            if failed { theme.error } else { theme.dim },
        ));
    }
    status.push(Span::styled(format!("Page {display_page}"), theme.dim));
    let status = Line::from(status);
    let right_width = (status.width() as u16).min(area.width.saturating_sub(2));
    let right_x = area.right().saturating_sub(2 + right_width);
    f.render_widget(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(theme.line),
        area,
    );
    if area.height > 1 {
        f.render_widget(
            Line::from(tabs),
            Rect::new(area.x, area.y, right_x.saturating_sub(area.x), 1),
        );
        f.render_widget(
            status.right_aligned(),
            Rect::new(right_x, area.y, right_width, 1),
        );
    }
}

/// Render the list of stories
fn render_stories_list(f: &mut Frame, area: Rect, app: &mut App) {
    // Keep selection in view
    app.update_story_scroll(area.height as usize);

    let (_, display_page) = app.displayed_story_context();
    let list_style = if app.is_loading() {
        app.theme().dim.add_modifier(Modifier::DIM)
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

            let theme = app.theme();
            let indicator = Span::styled(if is_selected { "▎ " } else { "  " }, theme.accent);
            let bookmark = if app.is_bookmarked(story.id) {
                " ★"
            } else {
                ""
            };
            let rank = format!("{global_idx:>3}  ");
            let title_width = usize::from(area.width)
                .saturating_sub(2 + rank.len() + Span::raw(bookmark).width());
            let mut title = story.title.clone();
            if Span::raw(&title).width() > title_width {
                title.clear();
                if title_width > 0 {
                    for ch in story.title.chars() {
                        title.push(ch);
                        if Span::raw(&title).width() + 1 > title_width {
                            title.pop();
                            break;
                        }
                    }
                    title.push('…');
                }
            }
            let mut lines = vec![Line::from(vec![
                indicator.clone(),
                Span::styled(rank, theme.dim),
                Span::styled(
                    title,
                    if is_selected {
                        theme.fg.add_modifier(Modifier::BOLD)
                    } else {
                        theme.fg
                    },
                ),
                Span::styled(bookmark, theme.accent),
            ])];

            // Second line: metadata
            let comment_str = match story.comments {
                Some(1) => "1 comment".to_string(),
                Some(n) => format!("{n} comments"),
                None => "discuss".to_string(),
            };

            let host = reqwest::Url::parse(&story.url)
                .ok()
                .and_then(|url| url.host_str().map(str::to_owned))
                .filter(|host| host != "news.ycombinator.com")
                .unwrap_or_else(|| "HN discussion".into());
            lines.push(Line::from(vec![
                indicator,
                Span::raw("     "),
                Span::styled(
                    format!("{host} · {} · {} points · ", story.author, story.score),
                    theme.dim,
                ),
                Span::styled(comment_str, theme.fg),
                Span::styled(format!(" · {}", story.time_ago), theme.dim),
            ]));

            if f.area().height >= 30 && idx < app.stories.len() - 1 {
                lines.push(Line::from(""));
            }

            ListItem::new(lines)
        })
        .collect();

    let list = List::new(items).style(list_style);

    let mut state = ListState::default()
        .with_selected(Some(app.selected_index))
        .with_offset(app.story_scroll);

    f.render_stateful_widget(list, area, &mut state);
    app.story_scroll = state.offset();
    // Apply after text rendering, which resets continuation cells of wide characters.
    let stride = if f.area().height >= 30 { 3 } else { 2 };
    let y = area.y + ((app.selected_index - state.offset()) * stride) as u16;
    let selected = Rect::new(area.x, y, area.width, 2).intersection(area);
    f.buffer_mut().set_style(selected, app.theme().sel);
}
