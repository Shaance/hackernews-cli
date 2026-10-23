//! Comments view rendering

use crate::app::{App, CommentState, View};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use textwrap::{wrap, Options};

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
    let has_post = app
        .discussion_story
        .as_ref()
        .is_some_and(|story| !story.text.is_empty());

    if app.is_loading() && app.comments.is_empty() && !has_post {
        widgets::render_loading(f, chunks[1], "Loading comments...", tick);
    } else if app.comments.is_empty() && !has_post {
        if let Some(error) = app.error() {
            widgets::render_error(f, chunks[1], error);
        } else {
            render_no_comments(f, chunks[1]);
        }
    } else {
        render_comments_list(f, chunks[1], app, tick, None, true);
    }

    let status = widgets::render_comments_status(chunks[2], app, tick);
    f.render_widget(status, chunks[2]);

    // Render help overlay if shown
    if app.show_help {
        widgets::render_help(f, app.theme(), true, app.overlay_opened);
    }
    if app.show_links {
        super::links::render(f, app);
    }
}

/// Render the existing discussion tree inside the prototype reader.
pub fn render_reader_content(
    f: &mut Frame,
    area: Rect,
    app: &mut App,
    tick: usize,
    mut prefix: Vec<Line<'static>>,
    show_selection: bool,
) {
    if app.comments.is_empty() {
        let message = if app.is_loading() {
            "Loading comments..."
        } else {
            app.error().unwrap_or("No comments yet")
        };
        for line in textwrap::wrap(message, usize::from(area.width).max(1)) {
            prefix.push(Line::raw(line.into_owned()));
        }
    }
    render_comments_list(f, area, app, tick, Some(prefix), show_selection);
}

/// Render title bar with story title
fn render_title(f: &mut Frame, area: Rect, app: &App, tick: usize) {
    let title_text = if let View::Comments { story_title, .. } = &app.view {
        let comment_count = app.visible_comment_count();
        vec![
            Line::from(vec![
                Span::styled(" Comments: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(story_title),
                Span::raw(
                    if app
                        .bookmark_story()
                        .is_some_and(|story| app.is_bookmarked(story.id))
                    {
                        " ★ Saved"
                    } else {
                        ""
                    },
                ),
            ]),
            Line::from(vec![
                Span::raw(format!(" {comment_count} loaded comments")),
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
        .block(Block::default().borders(Borders::BOTTOM));

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
fn render_comments_list(
    f: &mut Frame,
    area: Rect,
    app: &mut App,
    tick: usize,
    prefix: Option<Vec<Line<'static>>>,
    show_selection: bool,
) {
    let list_style = if app.is_loading() {
        app.theme().dim.add_modifier(Modifier::DIM)
    } else {
        Style::default()
    };

    // Pre-render all comments to line buffers and track their line ranges
    let mut rendered = Vec::with_capacity(app.visible_comment_count());
    let mut line_ranges = Vec::with_capacity(app.visible_comment_count());
    let mut post_lines = Vec::new();
    if let Some(prefix) = prefix {
        post_lines = prefix;
    } else if let Some(story) = &app.discussion_story {
        if !story.text.is_empty() {
            post_lines.push(Line::styled(
                "Original post",
                Style::default().add_modifier(Modifier::BOLD),
            ));
            for line in story.text.lines() {
                for wrapped in wrap_with_indent(line, area.width as usize) {
                    post_lines.push(Line::raw(wrapped));
                }
            }
            post_lines.push(Line::raw(""));
        }
    }
    let mut line_cursor = post_lines.len();

    for idx in 0..app.visible_comment_count() {
        let Some((path, comment)) = app.visible_comment_at(idx) else {
            continue;
        };
        let is_selected = show_selection && idx == app.comment_cursor;
        let prepared = render_comment(app, path, comment, is_selected, tick, area.width as usize);
        let start = line_cursor;
        line_cursor += prepared.height;
        let end = line_cursor;
        line_ranges.push((comment.id, start, end));
        rendered.push(prepared);
    }

    // Adjust scroll to keep the selection visible, even when a long comment would otherwise leave blank space
    if line_ranges.is_empty() {
        app.update_comment_scroll(&[(0, 0, post_lines.len())], area.height as usize);
    } else {
        app.update_comment_scroll(&line_ranges, area.height as usize);
    }

    let total_lines = line_cursor;
    let scroll = app.comment_scroll.min(total_lines.saturating_sub(1));

    // Flatten all lines
    let mut lines = post_lines;
    for item in rendered {
        lines.extend(item.lines);
    }

    let paragraph = Paragraph::new(lines)
        .style(list_style)
        .block(Block::default().borders(Borders::NONE))
        .scroll((scroll as u16, 0));

    f.render_widget(paragraph, area);
    if let Some((_, start, end)) = line_ranges
        .get(app.comment_cursor)
        .filter(|_| show_selection)
    {
        let top = start.saturating_sub(scroll).min(usize::from(area.height));
        let bottom = end.saturating_sub(scroll).min(usize::from(area.height));
        let selected = Rect::new(
            area.x,
            area.y + top as u16,
            area.width,
            (bottom - top) as u16,
        );
        f.buffer_mut().set_style(selected, app.theme().sel);
    }
}

/// A rendered comment ready for display
struct RenderedComment {
    lines: Vec<Line<'static>>,
    height: usize,
}

/// Render a single comment into lines
fn render_comment(
    app: &App,
    path: &[usize],
    comment: &crate::app::Comment,
    is_selected: bool,
    tick: usize,
    max_width: usize,
) -> RenderedComment {
    let theme = app.theme();
    let depth = path.len().saturating_sub(1);
    let text_column = 2 + 2 * depth;
    let prefix = vec![
        Span::styled(if is_selected { "▎ " } else { "  " }, theme.accent),
        Span::styled("│ ".repeat(depth), theme.line),
    ];
    let mut header = prefix.clone();
    if comment.deleted {
        header.push(Span::styled(
            "[deleted]",
            theme.dim.add_modifier(Modifier::ITALIC),
        ));
    } else {
        header.push(Span::styled(
            comment.author.clone(),
            if is_selected {
                theme.fg.add_modifier(Modifier::BOLD)
            } else {
                theme.fg
            },
        ));
        header.push(Span::styled(format!("  {}", comment.time_ago), theme.dim));
    }
    let mut lines = vec![Line::from(header)];
    if !comment.deleted {
        for body in comment.text.lines() {
            let quote = body.trim_start().strip_prefix('>');
            let text = quote
                .map(|text| text.strip_prefix(' ').unwrap_or(text))
                .unwrap_or(body);
            let width = max_width.saturating_sub(text_column + if quote.is_some() { 2 } else { 0 });
            // Track source offsets so a URL keeps its colour when wrapped across rows.
            let mut consumed = 0;
            for wrapped in wrap_with_indent(text, width) {
                consumed += text[consumed..]
                    .find(&wrapped)
                    .expect("wrapped text is a source slice");
                let mut spans = prefix.clone();
                if quote.is_some() {
                    spans.push(Span::styled("▏ ", theme.line));
                }
                for token in wrapped.split_inclusive(char::is_whitespace) {
                    let preceding = text[..consumed]
                        .rsplit(char::is_whitespace)
                        .next()
                        .unwrap_or("");
                    let start = consumed - preceding.len();
                    let is_link = text[start..].starts_with("https://")
                        || text[start..].starts_with("http://");
                    spans.push(Span::styled(
                        token.to_owned(),
                        if is_link {
                            theme.link
                        } else if quote.is_some() {
                            theme.dim
                        } else {
                            theme.fg
                        },
                    ));
                    consumed += token.len();
                }
                lines.push(Line::from(spans));
            }
        }
    }
    if comment.has_children() {
        let child_info = match comment.state {
            CommentState::Collapsed => Some(format!(
                "+ {} {}",
                comment.child_count(),
                if comment.child_count() == 1 {
                    "reply"
                } else {
                    "replies"
                }
            )),
            CommentState::Loading { .. } => {
                Some(format!("{} loading replies", widgets::spinner_frame(tick)))
            }
            CommentState::Expanded { .. } => None,
        };
        if let Some(info) = child_info {
            let mut spans = prefix;
            spans.push(Span::styled(info, theme.dim));
            lines.push(Line::from(spans));
        }
    }
    lines.push(Line::from(Span::styled(
        if is_selected { "▎" } else { "" },
        theme.accent,
    )));
    let height = lines.len();
    RenderedComment { lines, height }
}

/// Wrap a line of comment text to the available width
fn wrap_with_indent(line: &str, width: usize) -> Vec<String> {
    let options = Options::new(width.max(1)).break_words(true);

    wrap(line, options)
        .into_iter()
        .map(|segment| segment.into_owned())
        .collect()
}
