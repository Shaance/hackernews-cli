//! Comments view rendering

use crate::{
    app::{
        comment_view_state::{COMMENT_INDENT, COMMENT_PREFIX},
        App, View,
    },
    comments::{Row, Thread},
    theme::Theme,
};
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

    app.prepare_comment_layout(chunks[1].width);
    render_title(f, chunks[0], app, tick);
    let has_post = app
        .discussion_story
        .as_ref()
        .is_some_and(|story| !story.text.is_empty());

    if app.is_loading() && app.thread.is_empty() && !has_post {
        widgets::render_loading(f, chunks[1], "Loading comments...", tick);
    } else if app.thread.is_empty() && !has_post {
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
    if app.thread.is_empty() {
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
    mut area: Rect,
    app: &mut App,
    tick: usize,
    prefix: Option<Vec<Line<'static>>>,
    show_selection: bool,
) {
    if app.thread.root().is_some() && !area.is_empty() {
        let trail = trail_line(&app.thread, &app.thread.trail(), area.width);
        f.render_widget(
            Paragraph::new(trail).style(app.theme().dim),
            Rect { height: 1, ..area },
        );
        area.y += 1;
        area.height -= 1;
    }

    let list_style = if app.is_loading() {
        app.theme().dim.add_modifier(Modifier::DIM)
    } else {
        Style::default()
    };

    // Pre-render all comments to line buffers and track their line ranges
    let rows = app.thread.rows();
    let mut rendered = Vec::with_capacity(rows.len());
    let mut line_ranges = Vec::with_capacity(rows.len());
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

    let selected = app.thread.cursor().filter(|_| show_selection);
    let renderer = CommentRows {
        thread: &app.thread,
        theme: app.theme(),
        tick,
        width: area.width,
    };
    for row in rows {
        let prepared = render_comment(
            &renderer,
            row,
            Some(row.id) == selected,
            app.comment_replies_pending(row.id),
            false,
        );
        let start = line_cursor;
        line_cursor += prepared.len();
        let end = line_cursor;
        line_ranges.push((row.id, start, end));
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
        lines.extend(item);
    }

    let paragraph = Paragraph::new(lines)
        .style(list_style)
        .block(Block::default().borders(Borders::NONE))
        .scroll((scroll as u16, 0));

    f.render_widget(paragraph, area);
    if let Some((_, start, end)) = line_ranges.iter().find(|(id, _, _)| Some(*id) == selected) {
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

pub(crate) fn trail_line(thread: &Thread, ids: &[i32], width: u16) -> Line<'static> {
    let trail = ids
        .iter()
        .filter_map(|id| thread.node(*id))
        .map(|node| {
            if node.readable {
                node.author.as_str()
            } else {
                "Replies"
            }
        })
        .collect::<Vec<_>>()
        .join(" › ");
    let mut start = 0;
    while start < trail.len()
        && Span::raw(&trail[start..]).width() + usize::from(start > 0) > usize::from(width)
    {
        start += trail[start..].chars().next().unwrap().len_utf8();
    }
    Line::from(format!(
        "{}{}",
        if start > 0 { "…" } else { "" },
        &trail[start..]
    ))
}

pub(crate) struct CommentRows<'a> {
    pub thread: &'a Thread,
    pub theme: Theme,
    pub tick: usize,
    pub width: u16,
}

pub(crate) fn render_comment(
    renderer: &CommentRows<'_>,
    row: Row,
    is_selected: bool,
    pending: bool,
    is_citation: bool,
) -> Vec<Line<'static>> {
    let CommentRows {
        thread,
        theme,
        tick,
        width,
    } = *renderer;
    let comment = thread.node(row.id);
    let depth = row.depth as usize;
    let text_column = usize::from(COMMENT_PREFIX) + usize::from(COMMENT_INDENT) * depth;
    let dimmed = thread.focus() && !thread.in_chain_from(row.id, thread.cursor());
    let ink = if dimmed { theme.dim } else { theme.fg };
    let prefix = vec![
        Span::styled(if is_selected { "▎ " } else { "  " }, theme.accent),
        Span::styled("│ ".repeat(depth), theme.line),
    ];
    let mut header = prefix.clone();
    if is_citation {
        header.push(Span::styled("Replies to cited comment", ink));
    } else if let Some(comment) = comment.filter(|comment| comment.readable) {
        header.push(Span::styled(
            comment.author.clone(),
            if is_selected {
                theme.fg.add_modifier(Modifier::BOLD)
            } else {
                ink
            },
        ));
        header.push(Span::styled(format!("  {}", comment.time_ago), theme.dim));
    } else {
        header.push(Span::styled(
            "[deleted]",
            theme.dim.add_modifier(Modifier::ITALIC),
        ));
    }
    let mut lines = vec![Line::from(header)];
    if let Some(comment) = comment.filter(|comment| comment.readable && !is_citation) {
        for body in comment.text.lines() {
            let quote = body.trim_start().strip_prefix('>');
            let text = quote
                .map(|text| text.strip_prefix(' ').unwrap_or(text))
                .unwrap_or(body);
            let width = usize::from(width)
                .saturating_sub(text_column + if quote.is_some() { 2 } else { 0 });
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
                        if dimmed || quote.is_some() && !is_link {
                            theme.dim
                        } else if is_link {
                            theme.link
                        } else {
                            ink
                        },
                    ));
                    consumed += token.len();
                }
                lines.push(Line::from(spans));
            }
        }
    }
    let child_count = thread.child_count(row.id);
    let child_info = if thread.capped(row.id) {
        let behind = thread.reply_count(row.id).max(child_count);
        Some(format!(
            "continue thread → {behind} {}",
            if behind == 1 { "reply" } else { "replies" }
        ))
    } else if pending {
        Some(format!("{} loading replies", widgets::spinner_frame(tick)))
    } else if child_count > 0 && (!thread.replies_loaded(row.id) || thread.is_folded(row.id)) {
        let child_count = thread.hidden_count(row.id).max(child_count);
        let label = if child_count == 1 { "reply" } else { "replies" };
        Some(format!("+ {child_count} {label}"))
    } else {
        None
    };
    if let Some(info) = child_info {
        let mut spans = prefix;
        spans.push(Span::styled(info, theme.dim));
        lines.push(Line::from(spans));
    }
    lines.push(Line::from(Span::styled(
        if is_selected { "▎" } else { "" },
        theme.accent,
    )));
    lines
}

/// Wrap a line of comment text to the available width
fn wrap_with_indent(line: &str, width: usize) -> Vec<String> {
    let options = Options::new(width.max(1)).break_words(true);

    wrap(line, options)
        .into_iter()
        .map(|segment| segment.into_owned())
        .collect()
}
