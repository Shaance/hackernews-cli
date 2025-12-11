//! Comments view rendering

use crate::app::{App, CommentState, View};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use super::widgets;

/// Render the comments view
pub fn render(f: &mut Frame, app: &mut App, tick: usize) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Title bar
            Constraint::Min(0),    // Comments list
            Constraint::Length(2), // Status bar
        ])
        .split(f.area());

    render_title(f, chunks[0], app, tick);

    if app.loading && app.comments.is_empty() {
        widgets::render_loading(f, chunks[1], "Loading comments...", tick);
    } else if let Some(error) = &app.error {
        widgets::render_error(f, chunks[1], error);
    } else if app.comments.is_empty() {
        render_no_comments(f, chunks[1]);
    } else {
        render_comments_list(f, chunks[1], app, tick);
    }

    let status = widgets::render_comments_status(chunks[2], app, tick);
    f.render_widget(status, chunks[2]);

    // Render help overlay if shown
    if app.show_help {
        widgets::render_help(f, f.area(), true);
    }
}

/// Render title bar with story title
fn render_title(f: &mut Frame, area: Rect, app: &App, tick: usize) {
    let title_text = if let View::Comments { story_title, .. } = &app.view {
        let comment_count = app.visible_comments.len();
        vec![
            Line::from(vec![
                Span::styled(" Comments: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(story_title),
            ]),
            Line::from(vec![
                Span::raw(format!(" {} comments", comment_count)),
                if app.should_show_loading() {
                    Span::raw(format!(" {} Loading...", widgets::spinner_frame(tick)))
                } else {
                    Span::raw("")
                },
            ]),
        ]
    } else {
        vec![Line::from(" Comments")]
    };

    let title = Paragraph::new(title_text)
        .style(Style::default())
        .block(Block::default().borders(Borders::BOTTOM))
        .wrap(Wrap { trim: true });

    f.render_widget(title, area);
}

/// Render no comments message
fn render_no_comments(f: &mut Frame, area: Rect) {
    let text = "No comments yet";
    let paragraph = Paragraph::new(text)
        .alignment(ratatui::layout::Alignment::Center)
        .style(Style::default().add_modifier(Modifier::DIM));

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

/// Render the list of comments
fn render_comments_list(f: &mut Frame, area: Rect, app: &mut App, tick: usize) {
    // Keep selection in view
    app.update_comment_scroll(area.height as usize);

    let list_style = if app.loading {
        Style::default().fg(Color::Gray).add_modifier(Modifier::DIM)
    } else {
        Style::default()
    };

    let items: Vec<ListItem> = app
        .visible_comments
        .iter()
        .enumerate()
        .map(|(idx, (path, comment))| {
            let is_selected = idx == app.comment_cursor;
            render_comment(app, path, comment, is_selected, tick)
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
        .with_selected(Some(app.comment_cursor))
        .with_offset(app.comment_scroll);

    f.render_stateful_widget(list, area, &mut state);
}

/// Render a single comment
fn render_comment<'a>(
    app: &'a App,
    path: &'a [usize],
    comment: &'a crate::app::Comment,
    is_selected: bool,
    tick: usize,
) -> ListItem<'a> {
    let guides = branch_guides(app, path);
    let mut lines = vec![];

    let stem_prefix = guides_to_prefix(&guides, true);
    let text_prefix = guides_to_prefix(&guides, false);
    let guide_color = depth_color(path.len().saturating_sub(1));

    // Comment header with author and time
    let header_style = if is_selected {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    let (indicator_symbol, indicator_style) = match comment.state {
        CommentState::Collapsed => ("▸ ".to_string(), Style::default().fg(Color::Yellow)),
        CommentState::Loading => (
            format!("{} ", widgets::spinner_frame(tick)),
            Style::default().fg(Color::Blue),
        ),
        CommentState::Expanded { .. } => ("▾ ".to_string(), Style::default().fg(Color::Green)),
    };

    if comment.deleted {
        lines.push(Line::from(vec![
            Span::styled(stem_prefix.clone(), Style::default().fg(guide_color)),
            Span::styled(indicator_symbol.clone(), indicator_style),
            Span::styled(
                "[deleted]",
                Style::default().add_modifier(Modifier::DIM | Modifier::ITALIC),
            ),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::styled(stem_prefix.clone(), Style::default().fg(guide_color)),
            Span::styled(indicator_symbol, indicator_style),
            Span::styled(format!("{} ", comment.author), header_style.fg(Color::Cyan)),
            Span::styled(
                format!("• {}", comment.time_ago),
                Style::default().fg(Color::Gray).add_modifier(Modifier::DIM),
            ),
        ]));

        // Comment text
        let text_style = Style::default();

        // Split text into lines and add indent
        for line in comment.text.lines() {
            if !line.trim().is_empty() {
                lines.push(Line::from(vec![
                    Span::styled(text_prefix.clone(), Style::default().fg(guide_color)),
                    Span::styled(line.to_string(), text_style),
                ]));
            }
        }

        // Show collapse/expand indicator
        if comment.has_children() {
            let child_info = match &comment.state {
                CommentState::Collapsed => {
                    let child_count = comment.child_count();
                    format!(
                        "▸ {} {}",
                        child_count,
                        if child_count == 1 { "reply" } else { "replies" }
                    )
                }
                CommentState::Loading => {
                    format!("{} Loading replies...", widgets::spinner_frame(tick))
                }
                CommentState::Expanded { .. } => "▾ Collapse".to_string(),
            };

            let child_style = match comment.state {
                CommentState::Collapsed => Style::default().fg(Color::Yellow),
                CommentState::Loading => Style::default().fg(Color::Blue),
                CommentState::Expanded { .. } => Style::default().fg(Color::Green),
            }
            .add_modifier(Modifier::DIM);

            lines.push(Line::from(vec![
                Span::styled(text_prefix, Style::default().fg(guide_color)),
                Span::styled(child_info, child_style),
            ]));
        }
    }

    // Add spacing between comments
    lines.push(Line::from(""));

    ListItem::new(lines)
}

/// Build branch guides to know which ancestors have following siblings
fn branch_guides(app: &App, path: &[usize]) -> Vec<bool> {
    let mut guides = Vec::new();
    let mut current_level: &[crate::app::Comment] = &app.comments;

    for (depth, &idx) in path.iter().enumerate() {
        let is_last = idx + 1 >= current_level.len();
        guides.push(is_last);

        if depth + 1 == path.len() {
            break;
        }

        if let Some(node) = current_level.get(idx) {
            if let CommentState::Expanded { children } = &node.state {
                current_level = children;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    guides
}

/// Convert branch guides into a prefix string (with or without final elbow)
fn guides_to_prefix(guides: &[bool], include_elbow: bool) -> String {
    let mut prefix = String::new();

    if guides.is_empty() {
        return prefix;
    }

    for (i, &is_last) in guides.iter().enumerate() {
        let is_final = i == guides.len() - 1;
        if is_final {
            if include_elbow {
                prefix.push_str(if is_last { "└─" } else { "├─" });
            } else {
                prefix.push_str(if is_last { "  " } else { "│ " });
            }
        } else {
            prefix.push_str(if is_last { "  " } else { "│ " });
        }
    }

    prefix.push_str("  ");
    prefix
}

/// Pick a guide color based on depth (cycles through a palette)
fn depth_color(depth: usize) -> Color {
    // Keep to high-contrast, readable colors that vary with depth
    const PALETTE: [Color; 6] = [
        Color::Gray,
        Color::Cyan,
        Color::Green,
        Color::Yellow,
        Color::Magenta,
        Color::LightBlue,
    ];

    PALETTE[depth % PALETTE.len()]
}
