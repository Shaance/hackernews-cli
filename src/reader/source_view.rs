use super::Reader;
use crate::{
    app::{
        comment_view_state::{CitationLookup, CommentRequest},
        App,
    },
    comments::{BranchStatus, Navigation},
    ui::comments::{render_comment, trail_line, CommentRows},
};
use crossterm::event::KeyCode;
use ratatui::{layout::Rect, text::Line, widgets::Paragraph, Frame};

pub(super) struct SourceView {
    pub(super) id: i32,
    nav: Navigation,
    pub(super) scroll: usize,
    page_rows: usize,
    levels: u32,
    follow: bool,
}

impl SourceView {
    pub(super) fn new(id: i32) -> Self {
        Self {
            id,
            nav: Navigation::rooted(id),
            scroll: 0,
            page_rows: 1,
            levels: 6,
            follow: false,
        }
    }

    pub(super) fn scroll_key(&mut self, key: KeyCode) {
        let step = self.page_rows.saturating_sub(1).max(1);
        match key {
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(step),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(step),
            KeyCode::Char('g') => self.scroll = 0,
            KeyCode::Char('G') => self.scroll = usize::MAX,
            _ => {}
        }
    }

    pub(super) fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        app: &mut App,
        tick: usize,
        mut lines: Vec<Line<'static>>,
    ) {
        self.page_rows = usize::from(area.height).max(1);
        self.levels = App::visible_levels_at(area.width);
        let id = self.id;
        if self
            .nav
            .root
            .is_some_and(|root| root != id && !app.thread.is_descendant(root, id))
        {
            self.nav.root = Some(id);
        }
        let theme = app.theme();
        let pending = app.pending_reply_ids();
        let status = if app.thread.node(id).is_some() {
            None
        } else if app.waiting_for_comments() {
            Some("Waiting for discussion…")
        } else if matches!(app.citation_lookup(id), Some(CitationLookup::Loading(_))) {
            Some("Loading cited comment…")
        } else {
            Some("Cited comment unavailable · Enter retry")
        };
        lines.push(Line::raw(""));
        let selected = app
            .thread
            .with_source_navigation(&mut self.nav, self.levels, |thread| {
                if let Some(cursor) = thread.cursor() {
                    // A resize can hide the cursor under the cap: keep the nearest visible ancestor.
                    let mut at = Some(cursor);
                    while let Some(id) = at.filter(|&id| !thread.select(id)) {
                        at = thread.parent_of(id);
                    }
                    if at.is_none() {
                        thread.first();
                    }
                }
                let trail = thread.trail();
                let start = trail
                    .iter()
                    .position(|entry| *entry == id)
                    .unwrap_or(trail.len());
                if trail.len() - start > 1 {
                    lines.push(trail_line(thread, &trail[start..], area.width).style(theme.dim));
                }
                let renderer = CommentRows {
                    thread,
                    theme,
                    tick,
                    width: area.width,
                };
                let mut selected = None;
                for row in thread.rows() {
                    let active = thread.cursor() == Some(row.id);
                    let start = lines.len();
                    lines.extend(render_comment(
                        &renderer,
                        row,
                        active,
                        pending.contains(&row.id),
                        row.id == id,
                    ));
                    let failure = !pending.contains(&row.id)
                        && thread
                            .node(row.id)
                            .is_some_and(|node| matches!(node.branch, BranchStatus::Failed { .. }));
                    if let Some(message) = status
                        .filter(|_| row.id == id)
                        .or_else(|| failure.then_some("Replies unavailable · Enter retry"))
                    {
                        for line in textwrap::wrap(
                            message,
                            usize::from(area.width).saturating_sub(2).max(1),
                        ) {
                            lines.push(Line::styled(format!("  {line}"), theme.dim));
                        }
                    }
                    if active {
                        selected = Some((start, lines.len()));
                    }
                }
                selected
            });
        if self.follow {
            if let Some((start, end)) = selected {
                self.scroll = if start < self.scroll || end - start > self.page_rows {
                    start
                } else {
                    self.scroll.max(end.saturating_sub(self.page_rows))
                };
            }
            self.follow = false;
        }
        self.scroll = self.scroll.min(lines.len().saturating_sub(self.page_rows));
        frame.render_widget(Paragraph::new(lines.split_off(self.scroll)), area);
        if let Some((start, end)) = selected {
            let height = usize::from(area.height);
            let top = start.saturating_sub(self.scroll).min(height);
            let bottom = end.saturating_sub(self.scroll).min(height);
            frame.buffer_mut().set_style(
                Rect::new(
                    area.x,
                    area.y + top as u16,
                    area.width,
                    (bottom - top) as u16,
                ),
                theme.sel,
            );
        }
    }
}

impl Reader {
    pub(crate) fn source_key(&mut self, app: &mut App, key: KeyCode) -> Option<CommentRequest> {
        let source = self.readings.get_mut(&self.active?)?.source.as_mut()?;
        let id = source.nav.cursor?;
        let pending = app.comment_replies_pending(id);
        let mut load = false;
        let follow = app
            .thread
            .with_source_navigation(&mut source.nav, source.levels, |thread| match key {
                KeyCode::Char('j') | KeyCode::Down => {
                    thread.next();
                    true
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    thread.prev();
                    true
                }
                KeyCode::Char('f') => {
                    thread.toggle_focus();
                    false
                }
                KeyCode::Backspace => id != source.id && thread.pop_trail(),
                KeyCode::Char('h') => thread.fold(),
                KeyCode::Char('c') if pending => thread.fold(),
                KeyCode::Char('c') => thread.collapse(),
                KeyCode::Enter | KeyCode::Char('o') => {
                    if thread.capped(id) {
                        load = true;
                        thread.activate()
                    } else if thread.unfold() {
                        load = !pending;
                        true
                    } else if pending {
                        thread.fold()
                    } else {
                        load = !thread.replies_loaded(id) || thread.reply_batch(id, true).is_some();
                        !load && thread.activate()
                    }
                }
                _ => false,
            });
        source.follow |= follow;
        if !load {
            return None;
        }
        if id == source.id {
            app.request_citation(id, true)
        } else {
            app.request_replies(id, true, true)
        }
    }
}

#[cfg(test)]
#[path = "tests/source_replies.rs"]
mod tests;
