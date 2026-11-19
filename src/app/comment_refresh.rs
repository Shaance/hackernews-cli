use super::{App, Comment};
use crate::HackerNewsCliServiceImpl;
use std::collections::HashSet;

impl App {
    /// Fetch the story's new root order and the replies of every branch loaded at refresh start.
    pub(crate) async fn fetch_comment_refresh(
        service: &HackerNewsCliServiceImpl,
        story_id: i32,
        open: &HashSet<i32>,
    ) -> anyhow::Result<Vec<Comment>> {
        let roots = service.top_level_comment_ids(story_id).await?;
        let mut stack = service.fetch_comments_strict(&roots).await?;
        stack.reverse();
        let mut fresh = Vec::new();
        while let Some(comment) = stack.pop() {
            if open.contains(&comment.id) {
                let children = service.fetch_comments_strict(&comment.child_ids).await?;
                stack.extend(children.into_iter().rev());
            }
            fresh.push(comment);
        }
        Ok(fresh)
    }

    /// Apply the complete refresh against choices made while it was in flight.
    pub(super) fn apply_comment_refresh(&mut self, fresh: Vec<Comment>) {
        let ranges = self.comment_line_ranges.clone();
        self.update_comment_scroll(&ranges, self.comment_viewport_height);
        let anchor = self.comment_scroll_anchor();
        let selected = self.thread.cursor();
        let old_rows = self.thread.rows();
        let old_index = old_rows.iter().position(|row| Some(row.id) == selected);
        let mut candidates = Vec::new();
        if let Some((id, _)) = anchor {
            candidates.extend(std::iter::successors(Some(id), |id| {
                self.thread.parent_of(*id)
            }));
            if let Some(index) = old_rows.iter().position(|row| row.id == id) {
                for distance in 1..old_rows.len() {
                    if let Some(i) = index.checked_sub(distance) {
                        candidates.push(old_rows[i].id);
                    }
                    if let Some(row) = old_rows.get(index + distance) {
                        candidates.push(row.id);
                    }
                }
            }
        }

        self.thread.replace_comments(&fresh);
        let rows = self.thread.rows();
        let visible: HashSet<_> = rows.iter().map(|row| row.id).collect();
        self.pending_replies.retain(|id, request| {
            visible.contains(id) || request.for_source && self.thread.node(*id).is_some()
        });
        if !selected.is_some_and(|id| visible.contains(&id)) {
            if let Some(row) = rows.get(old_index.unwrap_or(0).min(rows.len().saturating_sub(1))) {
                self.thread.select(row.id);
            }
        }
        self.comment_anchor = candidates
            .into_iter()
            .find(|id| visible.contains(id))
            .map(|id| (id, anchor.filter(|a| a.0 == id).map_or(0, |a| a.1)));
        self.refreshing_comments = false;
        self.clear_comment_status();
        self.update_reply_loading();
    }

    /// A refresh failure keeps the previous tree, selection, and passage readable.
    pub fn apply_comment_error(&mut self, error: String) {
        self.refreshing_comments = false;
        self.set_comment_error(error);
        self.update_reply_loading();
    }
}
