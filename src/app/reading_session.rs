use super::{App, StoryType, Thread, View};
use crate::HNCLIItem;

/// Owns the loaded discussion while its reader is closed.
pub(super) struct ReadingSession {
    thread: Thread,
    anchor: Option<(i32, usize)>,
    scroll: usize,
}

impl App {
    pub(super) fn store_reading_session(&mut self) {
        let View::Comments { story_id, .. } = self.view else {
            return;
        };
        // An interrupted initial fetch must be retried on re-entry.
        if self.thread.is_empty() && (self.is_loading() || self.error().is_some()) {
            return;
        }
        let anchor = self.comment_anchor.or_else(|| self.comment_scroll_anchor());
        self.reading_sessions.insert(
            story_id,
            ReadingSession {
                thread: std::mem::replace(&mut self.thread, Self::empty_comment_thread()),
                anchor,
                scroll: self.comment_scroll,
            },
        );
    }

    pub(super) fn restore_reading_session(&mut self, story_id: i32) -> bool {
        let Some(session) = self.reading_sessions.remove(&story_id) else {
            return false;
        };
        self.thread = session.thread;
        self.comment_anchor = session.anchor;
        self.comment_scroll = session.scroll;
        self.clear_comment_status();
        true
    }

    /// Restore a reader position without following the selected comment on redraw.
    #[cfg(test)]
    pub(crate) fn set_comment_position(&mut self, id: i32, scroll: usize) {
        assert!(self.thread.select(id));
        self.comment_scroll = scroll;
        self.follow_comment_selection = false;
        self.pending_comment_pages = 0;
    }

    pub fn scroll_comment_page(&mut self, down: bool) {
        self.follow_comment_selection = false;
        self.pending_comment_pages += if down { 1 } else { -1 };
    }

    pub(crate) fn begin_comment_refresh(&mut self) -> (u64, std::collections::HashSet<i32>) {
        self.pending_replies.clear();
        self.comment_view_generation = self.comment_view_generation.wrapping_add(1);
        self.refreshing_comments = true;
        self.set_comment_loading(true);
        // Folded branches keep their replies, so they refresh too; reopening must not show stale ones.
        (self.comment_view_generation, self.thread.loaded_branches())
    }

    pub fn current_links(&self) -> Vec<&str> {
        let mut links = Vec::new();
        if let Some(story) = &self.discussion_story {
            links.extend(story.links.iter().map(String::as_str));
        }
        if let Some(comment) = self.thread.cursor().and_then(|id| self.thread.node(id)) {
            links.extend(comment.links.iter().map(String::as_str));
        }
        links
    }

    pub fn bookmark_story(&self) -> Option<&HNCLIItem> {
        match self.view {
            View::Stories => self.selected_story(),
            View::Comments { .. } => self.discussion_story.as_ref(),
        }
    }

    pub fn is_bookmarked(&self, id: i32) -> bool {
        self.bookmarks.iter().any(|story| story.id == id)
    }

    pub fn show_saved_stories(&mut self) {
        self.next_story_request_generation();
        self.set_stories_for(StoryType::Saved, 1, self.bookmarks.clone());
    }
}
