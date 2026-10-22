use super::{App, Comment, CommentState, StoryType, View};
use crate::HNCLIItem;

/// Owns the loaded discussion while its reader is closed.
pub(super) struct ReadingSession {
    comments: Vec<Comment>,
    cursor: usize,
    scroll: usize,
}

impl App {
    pub(super) fn store_reading_session(&mut self) {
        let View::Comments { story_id, .. } = self.view else {
            return;
        };
        // An interrupted initial fetch must be retried on re-entry.
        if self.comments.is_empty() && (self.is_loading() || self.error().is_some()) {
            return;
        }
        let mut comments = std::mem::take(&mut self.comments);
        reset_pending_replies(&mut comments);
        self.reading_sessions.insert(
            story_id,
            ReadingSession {
                comments,
                cursor: self.comment_cursor,
                scroll: self.comment_scroll,
            },
        );
    }

    pub(super) fn restore_reading_session(&mut self, story_id: i32) -> bool {
        let Some(session) = self.reading_sessions.remove(&story_id) else {
            return false;
        };
        self.comments = session.comments;
        self.comment_cursor = session.cursor;
        self.comment_scroll = session.scroll;
        self.rebuild_visible_comments();
        self.clear_comment_status();
        true
    }

    /// Restore a reader position without following the selected comment on redraw.
    pub fn set_comment_position(&mut self, cursor: usize, scroll: usize) {
        self.comment_cursor = cursor;
        self.comment_scroll = scroll;
        self.follow_comment_selection = false;
        self.pending_comment_pages = 0;
    }

    pub fn scroll_comment_page(&mut self, down: bool) {
        self.follow_comment_selection = false;
        self.pending_comment_pages += if down { 1 } else { -1 };
    }

    pub fn begin_comment_refresh(&mut self) -> u64 {
        reset_pending_replies(&mut self.comments);
        self.comment_view_generation = self.comment_view_generation.wrapping_add(1);
        self.set_comment_loading(true);
        self.comment_view_generation
    }

    pub fn current_links(&self) -> Vec<&str> {
        let mut links = Vec::new();
        if let Some(story) = &self.discussion_story {
            links.extend(story.links.iter().map(String::as_str));
        }
        if let Some((_, comment)) = self.visible_comment_at(self.comment_cursor) {
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

fn reset_pending_replies(comments: &mut [Comment]) {
    for comment in comments {
        match &mut comment.state {
            CommentState::Loading { .. } => comment.state = CommentState::Collapsed,
            CommentState::Expanded { children } => reset_pending_replies(children),
            CommentState::Collapsed => {}
        }
    }
}
