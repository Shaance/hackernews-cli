use super::{App, Comment, CommentState};

impl App {
    /// Apply the complete refresh against choices made while it was in flight.
    pub(super) fn apply_comment_refresh(&mut self, mut fresh: Vec<Comment>) {
        let ranges = self.comment_line_ranges.clone();
        self.update_comment_scroll(&ranges, self.comment_viewport_height);
        let anchor = ranges
            .iter()
            .find(|r| r.1 <= self.comment_scroll && self.comment_scroll < r.2)
            .map(|r| (r.0, self.comment_scroll - r.1));
        let selected = self
            .visible_comment_at(self.comment_cursor)
            .map(|(_, c)| c.id);
        let old_ids: Vec<_> = (0..self.visible_comment_count())
            .filter_map(|i| self.visible_comment_at(i).map(|(_, c)| c.id))
            .collect();
        let mut candidates = Vec::new();
        if let Some((id, _)) = anchor {
            candidates.push(id);
            if let Some(index) = old_ids.iter().position(|old| *old == id) {
                let path = self.visible_comment_at(index).unwrap().0;
                for i in (0..index).rev() {
                    let (parent_path, parent) = self.visible_comment_at(i).unwrap();
                    if path.starts_with(parent_path) {
                        candidates.push(parent.id);
                    }
                }
                for distance in 1..old_ids.len() {
                    if let Some(i) = index.checked_sub(distance) {
                        candidates.push(old_ids[i]);
                    }
                    if let Some(id) = old_ids.get(index + distance) {
                        candidates.push(*id);
                    }
                }
            }
        }
        merge_expansion(&mut fresh, &mut self.comments);
        self.comments = fresh;
        self.rebuild_visible_comments();
        self.comment_cursor = (0..self.visible_comment_count())
            .find(|&i| self.visible_comment_at(i).map(|(_, c)| c.id) == selected)
            .unwrap_or(
                self.comment_cursor
                    .min(self.visible_comment_count().saturating_sub(1)),
            );
        self.comment_anchor = candidates
            .into_iter()
            .find(|id| {
                (0..self.visible_comment_count())
                    .any(|i| self.visible_comment_at(i).unwrap().1.id == *id)
            })
            .map(|id| (id, anchor.filter(|a| a.0 == id).map_or(0, |a| a.1)));
        self.refreshing_comments = false;
        self.clear_comment_status();
        self.recompute_comment_loading(self.has_loading_comments());
    }

    /// A refresh failure keeps the previous tree, selection, and passage readable.
    pub fn apply_comment_error(&mut self, error: String) {
        self.refreshing_comments = false;
        self.set_comment_error(error);
        self.recompute_comment_loading(self.has_loading_comments());
    }
}

fn merge_expansion(fresh: &mut [Comment], current: &mut [Comment]) {
    for comment in fresh {
        let Some(old) = current.iter_mut().find(|c| c.id == comment.id) else {
            continue;
        };
        match std::mem::replace(&mut old.state, CommentState::Collapsed) {
            CommentState::Expanded { mut children } => match &mut comment.state {
                CommentState::Expanded { children: fetched } => {
                    merge_expansion(fetched, &mut children)
                }
                _ => comment.state = CommentState::Expanded { children },
            },
            state => comment.state = state,
        }
    }
}
