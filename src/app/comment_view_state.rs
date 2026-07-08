use super::App;

/// State of a comment's children
#[derive(Debug, Clone)]
pub enum CommentState {
    /// Children not yet fetched
    Collapsed,
    /// Currently fetching children
    Loading,
    /// Children fetched and available
    Expanded { children: Vec<Comment> },
}

/// A HackerNews comment
#[derive(Debug, Clone)]
pub struct Comment {
    pub id: i32,
    pub author: String,
    pub text: String,
    pub time_ago: String,
    pub state: CommentState,
    pub depth: usize,
    pub deleted: bool,
    /// Child comment IDs (preserved across expand/collapse)
    pub child_ids: Vec<i32>,
}

impl Comment {
    /// Check if comment has children
    pub fn has_children(&self) -> bool {
        !self.child_ids.is_empty()
    }

    /// Get child count
    pub fn child_count(&self) -> usize {
        self.child_ids.len()
    }

    /// Check if expanded
    pub fn is_expanded(&self) -> bool {
        matches!(self.state, CommentState::Expanded { .. })
    }

    /// Check if loading
    pub fn is_loading(&self) -> bool {
        matches!(self.state, CommentState::Loading)
    }
}

impl App {
    /// Move to next comment
    pub fn next_comment(&mut self) {
        if !self.visible_comment_paths.is_empty()
            && self.comment_cursor < self.visible_comment_paths.len() - 1
        {
            self.comment_cursor += 1;
        }
    }

    /// Move to previous comment
    pub fn prev_comment(&mut self) {
        if self.comment_cursor > 0 {
            self.comment_cursor -= 1;
        }
    }

    /// Jump to the next sibling comment (skips over the current thread)
    pub fn next_comment_sibling(&mut self) {
        let Some(path) = self.visible_comment_paths.get(self.comment_cursor) else {
            return;
        };

        let Some((_, parent_path)) = path.split_last() else {
            return;
        };

        for (idx, candidate_path) in self
            .visible_comment_paths
            .iter()
            .enumerate()
            .skip(self.comment_cursor + 1)
        {
            if candidate_path.len() != path.len() {
                continue;
            }

            if candidate_path.starts_with(parent_path) {
                self.comment_cursor = idx;
                return;
            }
        }
    }

    /// Jump to the previous sibling comment
    pub fn prev_comment_sibling(&mut self) {
        let Some(path) = self.visible_comment_paths.get(self.comment_cursor) else {
            return;
        };

        let Some((_, parent_path)) = path.split_last() else {
            return;
        };

        let mut idx = self.comment_cursor;
        while idx > 0 {
            idx -= 1;
            let candidate_path = &self.visible_comment_paths[idx];
            if candidate_path.len() != path.len() {
                continue;
            }

            if candidate_path.starts_with(parent_path) {
                self.comment_cursor = idx;
                return;
            }
        }
    }

    /// Jump to the parent comment of the current selection
    pub fn parent_comment(&mut self) {
        let Some(path) = self.visible_comment_paths.get(self.comment_cursor) else {
            return;
        };

        if path.len() < 2 {
            return; // Already at top level
        }

        let parent_path = &path[..path.len() - 1];

        if let Some((idx, _)) = self
            .visible_comment_paths
            .iter()
            .enumerate()
            .find(|(_, candidate_path)| candidate_path.as_slice() == parent_path)
        {
            self.comment_cursor = idx;
        }
    }

    /// Go to top comment
    pub fn first_comment(&mut self) {
        self.comment_cursor = 0;
    }

    /// Go to bottom comment
    pub fn last_comment(&mut self) {
        if !self.visible_comment_paths.is_empty() {
            self.comment_cursor = self.visible_comment_paths.len() - 1;
        }
    }

    /// Update scroll offset (in lines) based on selected comment and viewport height
    pub fn update_comment_scroll(
        &mut self,
        line_ranges: &[(usize, usize)],
        viewport_height: usize,
    ) {
        if line_ranges.is_empty() || self.comment_cursor >= line_ranges.len() {
            self.comment_scroll = 0;
            return;
        }

        let view_height = viewport_height.max(1);
        let (start, end) = line_ranges[self.comment_cursor];
        let mut new_scroll = self.comment_scroll;

        // Keep selected comment fully visible
        if start < new_scroll {
            new_scroll = start;
        } else if end > new_scroll + view_height {
            new_scroll = end.saturating_sub(view_height);
        }

        // Clamp to available content
        let total_lines = line_ranges.last().map(|(_, end)| *end).unwrap_or(0);
        let max_scroll = total_lines.saturating_sub(view_height);
        if new_scroll > max_scroll {
            new_scroll = max_scroll;
        }

        self.comment_scroll = new_scroll;
    }

    /// Get currently selected comment (mutable)
    pub fn selected_comment_mut(&mut self) -> Option<&mut Comment> {
        // Clone the path to avoid borrow conflicts
        let path = self
            .visible_comment_paths
            .get(self.comment_cursor)
            .cloned()?;

        self.get_comment_mut_by_path(&path)
    }

    /// Get a visible comment and its path by flattened index
    pub fn visible_comment_at(&self, index: usize) -> Option<(&[usize], &Comment)> {
        let path = self.visible_comment_paths.get(index)?;
        let comment = self.get_comment_by_path(path)?;
        Some((path.as_slice(), comment))
    }

    /// Count comments visible in the flattened render/navigation order
    pub fn visible_comment_count(&self) -> usize {
        self.visible_comment_paths.len()
    }

    /// Get an immutable reference to a comment by path
    fn get_comment_by_path(&self, path: &[usize]) -> Option<&Comment> {
        if path.is_empty() {
            return None;
        }

        let mut current = self.comments.get(path[0])?;

        for &child_idx in &path[1..] {
            if let CommentState::Expanded { children } = &current.state {
                current = children.get(child_idx)?;
            } else {
                return None;
            }
        }

        Some(current)
    }

    /// Get a mutable reference to a comment by path
    fn get_comment_mut_by_path(&mut self, path: &[usize]) -> Option<&mut Comment> {
        if path.is_empty() {
            return None;
        }

        // Start with the top-level comment
        let mut current = self.comments.get_mut(path[0])?;

        // Navigate down the path
        for &child_idx in &path[1..] {
            if let CommentState::Expanded { children } = &mut current.state {
                current = children.get_mut(child_idx)?;
            } else {
                return None;
            }
        }

        Some(current)
    }

    /// Collapse the nearest expanded ancestor (or current comment) in the thread
    pub fn collapse_current_thread(&mut self) {
        let Some(path) = self.visible_comment_paths.get(self.comment_cursor).cloned() else {
            return;
        };

        for depth in (0..path.len()).rev() {
            if let Some(comment) = self.get_comment_mut_by_path(&path[..=depth]) {
                match comment.state {
                    CommentState::Expanded { .. } | CommentState::Loading => {
                        comment.state = CommentState::Collapsed;
                        self.rebuild_visible_comments();
                        self.comment_cursor = self
                            .comment_cursor
                            .min(self.visible_comment_paths.len().saturating_sub(1));
                        return;
                    }
                    _ => {}
                }
            }
        }
    }

    /// Find and update a comment by ID (recursively searches all levels)
    pub fn update_comment_by_id<F>(&mut self, comment_id: i32, updater: F) -> bool
    where
        F: Fn(&mut Comment),
    {
        for comment in &mut self.comments {
            if Self::update_comment_recursive(comment, comment_id, &updater) {
                return true;
            }
        }
        false
    }

    /// Recursively search and update a comment
    fn update_comment_recursive<F>(comment: &mut Comment, target_id: i32, updater: &F) -> bool
    where
        F: Fn(&mut Comment),
    {
        if comment.id == target_id {
            updater(comment);
            return true;
        }

        if let CommentState::Expanded { children } = &mut comment.state {
            for child in children {
                if Self::update_comment_recursive(child, target_id, updater) {
                    return true;
                }
            }
        }

        false
    }

    /// Rebuild the flattened visible comments list
    pub fn rebuild_visible_comments(&mut self) {
        self.visible_comment_paths.clear();
        for (idx, comment) in self.comments.iter().enumerate() {
            let mut path = vec![idx];
            Self::add_visible_comment_path_recursive(
                &mut self.visible_comment_paths,
                &mut path,
                comment,
            );
        }
    }

    /// Recursively add comment paths to the visible list
    fn add_visible_comment_path_recursive(
        visible_comment_paths: &mut Vec<Vec<usize>>,
        path: &mut Vec<usize>,
        comment: &Comment,
    ) {
        visible_comment_paths.push(path.clone());

        if let CommentState::Expanded { children } = &comment.state {
            for (child_idx, child) in children.iter().enumerate() {
                path.push(child_idx);
                Self::add_visible_comment_path_recursive(visible_comment_paths, path, child);
                path.pop();
            }
        }
    }
}
