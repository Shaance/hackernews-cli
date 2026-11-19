use super::{App, Thread};

pub(crate) const COMMENT_PREFIX: u16 = 2;
pub(crate) const COMMENT_INDENT: u16 = 2;

/// A normalized Hacker News comment, with reply IDs in HN order.
#[derive(Debug, Clone)]
pub struct Comment {
    pub id: i32,
    pub author: String,
    pub text: String,
    pub links: Vec<String>,
    pub time_ago: String,
    pub deleted: bool,
    pub child_ids: Vec<i32>,
}

impl App {
    pub(super) fn empty_comment_thread() -> Thread {
        let mut thread = Thread::empty(None, usize::MAX);
        thread.toggle_focus();
        thread
    }

    pub fn next_comment(&mut self) {
        self.follow_comment_selection = true;
        self.thread.next();
    }

    pub fn prev_comment(&mut self) {
        self.follow_comment_selection = true;
        self.thread.prev();
    }

    pub fn next_comment_sibling(&mut self) {
        self.follow_comment_selection = true;
        if self.thread.root().is_none() || self.thread.cursor() != self.thread.root() {
            self.thread.next_sibling();
        }
    }

    pub fn prev_comment_sibling(&mut self) {
        self.follow_comment_selection = true;
        if self.thread.root().is_none() || self.thread.cursor() != self.thread.root() {
            self.thread.prev_sibling();
        }
    }

    pub fn parent_comment(&mut self) {
        self.follow_comment_selection = true;
        self.thread.to_parent();
    }

    pub fn first_comment(&mut self) {
        self.thread.first();
        self.comment_scroll = 0;
        self.comment_anchor = None;
        self.follow_comment_selection = false;
    }

    pub fn last_comment(&mut self) {
        self.follow_comment_selection = true;
        self.thread.last();
    }

    pub fn visible_comment_count(&self) -> usize {
        self.thread.rows().len()
    }

    pub(crate) fn comment_scroll_anchor(&self) -> Option<(i32, usize)> {
        self.comment_line_ranges
            .iter()
            .find(|r| r.1 <= self.comment_scroll && self.comment_scroll < r.2)
            .map(|r| (r.0, self.comment_scroll - r.1))
    }

    pub(crate) fn prepare_comment_layout(&mut self, width: u16) {
        if self.comment_layout_width != width {
            self.comment_anchor = self.comment_anchor.or_else(|| self.comment_scroll_anchor());
            self.comment_layout_width = width;
            let selected = self.thread.cursor();
            self.thread
                .set_visible_levels(self.comment_visible_levels());
            self.follow_comment_selection |= self.thread.cursor() != selected;
        }
    }

    pub(super) fn comment_visible_levels(&self) -> u32 {
        if self.comment_layout_width == 0 {
            return 6;
        }
        let text_width = self.comment_layout_width.saturating_sub(COMMENT_PREFIX);
        u32::from((text_width.saturating_sub(60) / COMMENT_INDENT + 1).clamp(2, 6))
    }

    pub(crate) fn pop_comment_trail(&mut self) {
        if self.thread.root().is_some() && self.thread.pop_trail() {
            self.follow_comment_selection = true;
        }
    }

    pub(crate) fn can_continue_thread(&self) -> bool {
        self.thread
            .cursor()
            .is_some_and(|id| self.thread.capped(id))
    }

    /// Update scroll offset (in lines) based on selected comment and viewport height
    pub fn update_comment_scroll(
        &mut self,
        line_ranges: &[(i32, usize, usize)],
        viewport_height: usize,
    ) {
        let view_height = viewport_height.max(1);
        if let Some((id, offset)) = self.comment_anchor.take() {
            if let Some(&(_, start, end)) = line_ranges.iter().find(|r| r.0 == id) {
                self.comment_scroll = start + offset.min(end.saturating_sub(start + 1));
            }
        }
        let mut new_scroll = self.comment_scroll;
        if self.follow_comment_selection {
            let selected = self.thread.cursor();
            if let Some(&(_, start, end)) = line_ranges.iter().find(|r| Some(r.0) == selected) {
                if start < new_scroll || end - start > view_height {
                    new_scroll = start;
                } else if end > new_scroll + view_height {
                    new_scroll = end.saturating_sub(view_height);
                }
            }
            self.follow_comment_selection = false;
        }
        let page_delta =
            self.pending_comment_pages as isize * view_height.saturating_sub(1).max(1) as isize;
        new_scroll = new_scroll.saturating_add_signed(page_delta);
        self.pending_comment_pages = 0;
        let total_lines = line_ranges.last().map(|(_, _, end)| *end).unwrap_or(0);
        // Growing the viewport (for example, hiding the pinned summary) must
        // not move the reader backward just to fill the newly available space.
        let max_scroll = total_lines
            .saturating_sub(view_height)
            .max(self.comment_scroll.min(total_lines.saturating_sub(1)));
        if new_scroll > max_scroll {
            new_scroll = max_scroll;
        }

        self.comment_scroll = new_scroll;
        self.comment_line_ranges = line_ranges.to_vec();
        self.comment_viewport_height = view_height;
    }

    pub fn collapse_current_thread(&mut self) {
        self.follow_comment_selection = true;
        let mut current = self.thread.cursor();
        while let Some(id) = current {
            if !self.thread.capped(id)
                && (self.comment_replies_pending(id) || self.comment_replies_open(id))
                && self.thread.select(id)
            {
                self.fold_comment(id);
                return;
            }
            if Some(id) == self.thread.root() {
                break;
            }
            current = self.thread.parent_of(id);
        }
    }

    fn fold_comment(&mut self, id: i32) {
        if self.comment_replies_open(id) {
            self.thread.fold();
        }
        self.pending_replies
            .retain(|parent, _| *parent != id && !self.thread.is_descendant(*parent, id));
        self.recompute_comment_loading(!self.pending_replies.is_empty());
    }

    pub(crate) fn comment_replies_pending(&self, id: i32) -> bool {
        self.pending_replies.contains_key(&id)
    }

    pub(crate) fn comment_replies_open(&self, id: i32) -> bool {
        self.thread.replies_loaded(id) && !self.thread.is_folded(id)
    }

    /// Toggle cached replies, or return the IDs and token for a new request.
    pub(crate) fn toggle_comment_replies(&mut self) -> Option<(i32, u64, Vec<i32>)> {
        let id = self.thread.cursor()?;
        let continuing = self.thread.capped(id);
        if continuing {
            self.thread.activate();
            self.follow_comment_selection = true;
        }
        if self.comment_replies_pending(id) {
            return None;
        }
        if self.comment_replies_open(id) {
            if !continuing {
                self.fold_comment(id);
            }
            return None;
        }
        if self.thread.is_folded(id) {
            self.thread.unfold();
            return None;
        }
        let batch = self
            .thread
            .reply_batch(id, false)
            .or_else(|| self.thread.reply_batch(id, true))?;
        self.comment_child_load_generation = self.comment_child_load_generation.wrapping_add(1);
        let generation = self.comment_child_load_generation;
        let ids = batch.ids().to_vec();
        self.pending_replies.insert(id, (generation, batch));
        self.set_comment_loading(true);
        Some((id, generation, ids))
    }

    pub(crate) fn apply_comment_replies(
        &mut self,
        id: i32,
        generation: u64,
        result: anyhow::Result<Vec<Comment>>,
    ) {
        if !self
            .pending_replies
            .get(&id)
            .is_some_and(|(pending, _)| *pending == generation)
        {
            return;
        }
        let (_, batch) = self.pending_replies.remove(&id).unwrap();
        let result = result.map_err(|error| {
            let message = format!("Failed to load comment children: {error}");
            self.set_comment_error(message.clone());
            message
        });
        self.thread.merge_replies(&batch, result);
        self.recompute_comment_loading(!self.pending_replies.is_empty());
    }
}
