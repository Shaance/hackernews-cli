//! Stories list view rendering

use crate::app::{App, StoryType, SELECTION_GLIDE};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};
use tachyonfx::{fx, ColorSpace, Interpolation};

use super::widgets;
use std::time::{Duration, Instant};

/// Feed switch motion: 110ms dip out + 160ms dip in on the list, 160ms tab color ease.
const FEED_DIP_OUT: Duration = Duration::from_millis(110);
const FEED_DIP_IN: Duration = Duration::from_millis(160);
const FEED_TAB_EASE: Duration = Duration::from_millis(160);

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
            let mut rows = vec![Line::styled(
                format!("Couldn't load {} stories", app.story_type.display_name()),
                theme.fg,
            )];
            rows.extend(
                textwrap::wrap(error, usize::from(chunks[1].width).max(1))
                    .into_iter()
                    .map(|row| Line::styled(row.into_owned(), theme.dim)),
            );
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

    // Feed switch: dip the outgoing list through the chrome color, then back in.
    let now = Instant::now();
    if let Some((start, _)) = app.feed_switched {
        let theme = app.theme();
        let elapsed = now.saturating_duration_since(start);
        if theme.motion {
            let scrim = theme.scrim.fg.unwrap();
            if elapsed < FEED_DIP_OUT {
                fx::fade_to_fg(scrim, (110, Interpolation::QuadIn))
                    .with_color_space(ColorSpace::Rgb)
                    .process(elapsed.into(), f.buffer_mut(), chunks[1]);
            } else if elapsed < FEED_DIP_OUT + FEED_DIP_IN {
                fx::fade_from_fg(scrim, (160, Interpolation::QuadOut))
                    .with_color_space(ColorSpace::Rgb)
                    .process((elapsed - FEED_DIP_OUT).into(), f.buffer_mut(), chunks[1]);
            }
        }
        if elapsed >= FEED_DIP_OUT + FEED_DIP_IN {
            app.feed_switched = None;
        }
    }

    let status = widgets::render_stories_status(chunks[2], app, tick);
    f.render_widget(status, chunks[2]);

    // Render help overlay if shown
    if app.show_help {
        widgets::render_help(f, app.theme(), false, app.overlay_opened);
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
    // Feed switch: the newly active tab eases dim → accent while the previous
    // one eases accent → dim, so tab and list tell one story.
    let tab_ease = app
        .feed_switched
        .filter(|_| app.theme().motion)
        .map(|(start, prev)| {
            let elapsed = Instant::now().saturating_duration_since(start);
            (
                elapsed,
                prev,
                Interpolation::QuadOut.alpha((elapsed.as_secs_f32() / FEED_TAB_EASE.as_secs_f32()).min(1.0)),
            )
        })
        .filter(|(elapsed, _, _)| *elapsed < FEED_TAB_EASE);
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
        let mut number_style = if active { theme.accent } else { theme.dim };
        let mut name_style = if active {
            theme
                .accent
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            theme.dim
        };
        if let Some((_, prev, alpha)) = tab_ease {
            if active {
                number_style = Style::default()
                    .fg(lerp_color(theme.dim.fg.unwrap(), theme.accent.fg.unwrap(), alpha));
                name_style = number_style.add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
            } else if *story_type == prev {
                number_style = Style::default()
                    .fg(lerp_color(theme.accent.fg.unwrap(), theme.dim.fg.unwrap(), alpha));
                name_style = number_style;
            }
        }
        tabs.push(Span::styled(format!("{} ", idx + 1), number_style));
        tabs.push(Span::styled(story_type.display_name(), name_style));
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
    if app.snapshot.is_none() {
        status.push(Span::styled(format!("Page {display_page}"), theme.dim));
    }
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

    let (display_type, display_page) = app.displayed_story_context();
    let list_style = if app.is_loading() {
        app.theme().dim.add_modifier(Modifier::DIM)
    } else {
        Style::default()
    };

    let now = Instant::now();
    let gliding = app.theme().motion
        && app.selection_glide.is_some_and(|(start, _, to)| {
            to == app.selected_index && now.saturating_duration_since(start) < SELECTION_GLIDE
        });
    let mut flash_cell = None;
    let items: Vec<ListItem> = app
        .stories
        .iter()
        .enumerate()
        .map(|(idx, story)| {
            let is_selected = idx == app.selected_index;
            let global_idx = ((display_page - 1) as usize * app.page_size as usize) + idx + 1;

            let theme = app.theme();
            // While gliding, the bar is drawn at the eased row after the list render.
            let indicator = Span::styled(
                if is_selected && !gliding { "▎ " } else { "  " },
                theme.accent,
            );
            let bookmark = if display_type != StoryType::Saved && app.is_bookmarked(story.id) {
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
            if !bookmark.is_empty() && app.save_flash.is_some_and(|(id, _)| id == story.id) {
                let x = area.x + 2 + rank.len() as u16 + Span::raw(&title).width() as u16 + 1;
                flash_cell = Some((idx, x, story.id));
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
    let position = if gliding {
        app.selection_glide_position(now)
    } else {
        app.selection_glide = None;
        app.selected_index as f32
    };
    let row = ((position - state.offset() as f32) * stride as f32)
        .round()
        .max(0.0) as u16;
    let y = area.y + row;
    let selected = Rect::new(area.x, y, area.width, 2).intersection(area);
    f.buffer_mut().set_style(selected, app.theme().sel);
    if gliding {
        f.buffer_mut().set_string(area.x, y, "▎", app.theme().accent);
    }
    if let Some((index, x, id)) = flash_cell.filter(|(index, _, _)| *index >= state.offset()) {
        let y = area.y + ((index - state.offset()) * stride) as u16;
        let cell = Rect::new(x, y, 1, 1).intersection(area);
        app.render_save_flash(f.buffer_mut(), cell, id);
    }
}

/// Interpolate two colors; non-RGB colors fall back to the target.
fn lerp_color(from: Color, to: Color, t: f32) -> Color {
    match (from, to) {
        (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) => {
            let lerp = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t) as u8;
            Color::Rgb(lerp(r1, r2), lerp(g1, g2), lerp(b1, b2))
        }
        _ => to,
    }
}
