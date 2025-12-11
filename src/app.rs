//! Application state management for HackerNews TUI

use crate::HNCLIItem;
use std::collections::HashMap;
use std::time::{Duration, Instant};

// Delay before showing loading indicators to avoid flicker
const LOADING_INDICATOR_DELAY_MS: u64 = 150;

/// Current view in the application
#[derive(Debug, Clone)]
pub enum View {
    /// Browsing stories list
    Stories,
    /// Viewing comments for a story
    Comments {
        story_id: i32,
        story_title: String,
        story_url: String,
    },
}

/// Type of stories to display
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StoryType {
    Best,
    New,
    Top,
}

impl StoryType {
    pub fn as_str(&self) -> &'static str {
        match self {
            StoryType::Best => "best",
            StoryType::New => "new",
            StoryType::Top => "top",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            StoryType::Best => "Best",
            StoryType::New => "New",
            StoryType::Top => "Top",
        }
    }
}

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

/// Main application state
pub struct App {
    /// Current view
    pub view: View,
    /// Current story type
    pub story_type: StoryType,
    /// List of stories
    pub stories: Vec<HNCLIItem>,
    /// Currently selected story index
    pub selected_index: usize,
    /// Scroll offset for stories view
    pub story_scroll: usize,
    /// Current page number
    pub current_page: u32,
    /// Loading state
    pub loading: bool,
    /// Error message, if any
    pub error: Option<String>,
    /// Comments for current story
    pub comments: Vec<Comment>,
    /// Flattened list of visible comments (for rendering/navigation)
    pub visible_comments: Vec<(Vec<usize>, Comment)>, // (path to comment, comment)
    /// Currently selected comment in visible list
    pub comment_cursor: usize,
    /// Scroll offset for comments view
    pub comment_scroll: usize,
    /// Should quit the application
    pub should_quit: bool,
    /// Show help overlay
    pub show_help: bool,
    /// Stories per page
    pub page_size: u8,
    /// Cache of fetched stories keyed by (StoryType, page)
    pub story_cache: HashMap<(StoryType, u32), Vec<HNCLIItem>>,
    /// Type/page that the currently displayed stories belong to (for stale detection)
    pub stories_for: Option<(StoryType, u32)>,
    /// When the current loading state started (for debouncing spinners)
    pub loading_since: Option<Instant>,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    /// Create a new application instance
    pub fn new() -> Self {
        let mut app = Self {
            view: View::Stories,
            story_type: StoryType::Best,
            stories: Vec::new(),
            selected_index: 0,
            story_scroll: 0,
            current_page: 1,
            loading: false,
            error: None,
            comments: Vec::new(),
            visible_comments: Vec::new(),
            comment_cursor: 0,
            comment_scroll: 0,
            should_quit: false,
            show_help: false,
            page_size: 20,
            story_cache: HashMap::new(),
            stories_for: None,
            loading_since: None,
        };

        app.set_loading(true);
        app
    }

    // === Story Navigation ===

    /// Move to next story
    pub fn next_story(&mut self) {
        if !self.stories.is_empty() && self.selected_index < self.stories.len() - 1 {
            self.selected_index += 1;
        }
    }

    /// Move to previous story
    pub fn prev_story(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    /// Update scroll offset based on selected index and viewport height
    pub fn update_story_scroll(&mut self, viewport_height: usize) {
        let visible_items = viewport_height.saturating_sub(1).max(1);

        // Ensure selected item is visible
        if self.selected_index < self.story_scroll {
            self.story_scroll = self.selected_index;
        } else if self.selected_index >= self.story_scroll + visible_items {
            self.story_scroll = self.selected_index.saturating_sub(visible_items - 1);
        }
    }

    /// Go to next page
    pub fn next_page(&mut self) {
        self.current_page += 1;
        self.selected_index = 0;
        self.story_scroll = 0;
    }

    /// Go to previous page
    pub fn prev_page(&mut self) {
        if self.current_page > 1 {
            self.current_page -= 1;
            self.selected_index = 0;
            self.story_scroll = 0;
        }
    }

    /// Switch story type
    pub fn set_story_type(&mut self, story_type: StoryType) {
        if self.story_type != story_type {
            self.story_type = story_type;
            self.current_page = 1;
            self.selected_index = 0;
            self.story_scroll = 0;
        }
    }

    /// Get currently selected story
    pub fn selected_story(&self) -> Option<&HNCLIItem> {
        self.stories.get(self.selected_index)
    }

    /// Get the story type/page that backs the currently displayed list (falls back to target)
    pub fn displayed_story_context(&self) -> (StoryType, u32) {
        self.stories_for
            .unwrap_or((self.story_type, self.current_page))
    }

    /// Whether the visible stories are from a different type/page than the selected target
    pub fn showing_stale_stories(&self) -> bool {
        match self.stories_for {
            Some((t, p)) => t != self.story_type || p != self.current_page,
            None => false,
        }
    }

    // === Comment Navigation ===

    /// Move to next comment
    pub fn next_comment(&mut self) {
        if !self.visible_comments.is_empty()
            && self.comment_cursor < self.visible_comments.len() - 1
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
        let Some((path, _)) = self.visible_comments.get(self.comment_cursor) else {
            return;
        };

        let Some((_, parent_path)) = path.split_last() else {
            return;
        };

        for (idx, (candidate_path, _)) in self
            .visible_comments
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
        let Some((path, _)) = self.visible_comments.get(self.comment_cursor) else {
            return;
        };

        let Some((_, parent_path)) = path.split_last() else {
            return;
        };

        let mut idx = self.comment_cursor;
        while idx > 0 {
            idx -= 1;
            let candidate_path = &self.visible_comments[idx].0;
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
        let Some((path, _)) = self.visible_comments.get(self.comment_cursor) else {
            return;
        };

        if path.len() < 2 {
            return; // Already at top level
        }

        let parent_path = &path[..path.len() - 1];

        if let Some((idx, _)) = self
            .visible_comments
            .iter()
            .enumerate()
            .find(|(_, (candidate_path, _))| candidate_path.as_slice() == parent_path)
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
        if !self.visible_comments.is_empty() {
            self.comment_cursor = self.visible_comments.len() - 1;
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
            .visible_comments
            .get(self.comment_cursor)
            .map(|(p, _)| p.clone())?;

        self.get_comment_mut_by_path(&path)
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
        let Some((path, _)) = self.visible_comments.get(self.comment_cursor).cloned() else {
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
                            .min(self.visible_comments.len().saturating_sub(1));
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
        self.visible_comments.clear();
        let comments = self.comments.clone();
        for (idx, comment) in comments.iter().enumerate() {
            Self::add_visible_comment_recursive(&mut self.visible_comments, vec![idx], comment);
        }
    }

    /// Recursively add comments to visible list (static method)
    fn add_visible_comment_recursive(
        visible_comments: &mut Vec<(Vec<usize>, Comment)>,
        path: Vec<usize>,
        comment: &Comment,
    ) {
        visible_comments.push((path.clone(), comment.clone()));

        if let CommentState::Expanded { children } = &comment.state {
            for (child_idx, child) in children.iter().enumerate() {
                let mut child_path = path.clone();
                child_path.push(child_idx);
                Self::add_visible_comment_recursive(visible_comments, child_path, child);
            }
        }
    }

    // === View Management ===

    /// Switch to comments view
    pub fn view_comments(&mut self, story_id: i32, story_title: String, story_url: String) {
        self.view = View::Comments {
            story_id,
            story_title,
            story_url,
        };
        self.comments.clear();
        self.visible_comments.clear();
        self.comment_cursor = 0;
        self.set_loading(true);
    }

    /// Switch back to stories view
    pub fn view_stories(&mut self) {
        self.view = View::Stories;
        self.comments.clear();
        self.visible_comments.clear();
        self.comment_cursor = 0;
    }

    /// Toggle help overlay
    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

    // === State Updates ===

    /// Set stories
    pub fn set_stories(&mut self, stories: Vec<HNCLIItem>) {
        self.stories = stories;
        self.loading = false;
        self.loading_since = None;
        self.error = None;

        // Ensure selected index is valid
        if self.selected_index >= self.stories.len() && !self.stories.is_empty() {
            self.selected_index = self.stories.len() - 1;
        }
    }

    /// Set stories and record their source type/page
    pub fn set_stories_for(&mut self, story_type: StoryType, page: u32, stories: Vec<HNCLIItem>) {
        self.stories_for = Some((story_type, page));
        self.set_stories(stories);
    }

    /// Apply loaded stories for a given page/type and cache them
    pub fn apply_stories_page(
        &mut self,
        story_type: StoryType,
        page: u32,
        stories: Vec<HNCLIItem>,
    ) {
        self.story_cache.insert((story_type, page), stories.clone());

        if self.story_type == story_type && self.current_page == page {
            self.set_stories_for(story_type, page, stories);
        }
    }

    /// Get cached stories for the current selection, if available
    pub fn cached_stories(&self) -> Option<Vec<HNCLIItem>> {
        self.story_cache
            .get(&(self.story_type, self.current_page))
            .cloned()
    }

    /// Set comments
    pub fn set_comments(&mut self, comments: Vec<Comment>) {
        self.comments = comments;
        self.rebuild_visible_comments();
        self.loading = false;
        self.loading_since = None;
        self.error = None;
    }

    /// Set error
    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
        self.loading = false;
        self.loading_since = None;
    }

    /// Clear error
    pub fn clear_error(&mut self) {
        self.error = None;
    }

    /// Set loading state
    pub fn set_loading(&mut self, loading: bool) {
        self.loading = loading;
        if loading {
            self.error = None;
            self.loading_since = Some(Instant::now());
        } else {
            self.loading_since = None;
        }
    }

    /// Whether loading indicator should be visible (debounced)
    pub fn should_show_loading(&self) -> bool {
        if !self.loading {
            return false;
        }

        match self.loading_since {
            Some(started) => started.elapsed() >= Duration::from_millis(LOADING_INDICATOR_DELAY_MS),
            None => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_story_type_conversion() {
        assert_eq!(StoryType::Best.as_str(), "best");
        assert_eq!(StoryType::New.as_str(), "new");
        assert_eq!(StoryType::Top.as_str(), "top");
    }

    #[test]
    fn test_app_navigation() {
        let mut app = App::new();
        app.stories = vec![
            HNCLIItem {
                id: 1,
                title: "Story 1".to_string(),
                url: "http://example.com".to_string(),
                author: "user1".to_string(),
                time: "2023-01-01".to_string(),
                time_ago: "1h ago".to_string(),
                score: 100,
                comments: Some(10),
            },
            HNCLIItem {
                id: 2,
                title: "Story 2".to_string(),
                url: "http://example.com".to_string(),
                author: "user2".to_string(),
                time: "2023-01-01".to_string(),
                time_ago: "2h ago".to_string(),
                score: 200,
                comments: Some(20),
            },
        ];

        assert_eq!(app.selected_index, 0);
        app.next_story();
        assert_eq!(app.selected_index, 1);
        app.next_story(); // Should not go beyond bounds
        assert_eq!(app.selected_index, 1);
        app.prev_story();
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_page_navigation() {
        let mut app = App::new();
        assert_eq!(app.current_page, 1);

        app.next_page();
        assert_eq!(app.current_page, 2);

        app.prev_page();
        assert_eq!(app.current_page, 1);

        app.prev_page(); // Should not go below 1
        assert_eq!(app.current_page, 1);
    }

    #[test]
    fn test_story_type_switch() {
        let mut app = App::new();
        app.selected_index = 5;
        app.current_page = 3;

        app.set_story_type(StoryType::New);
        assert_eq!(app.story_type, StoryType::New);
        assert_eq!(app.selected_index, 0); // Reset
        assert_eq!(app.current_page, 1); // Reset
    }

    #[test]
    fn test_next_comment_sibling_skips_thread() {
        let child_a = Comment {
            id: 2,
            author: "child_a".to_string(),
            text: "First child".to_string(),
            time_ago: "1m ago".to_string(),
            state: CommentState::Collapsed,
            depth: 1,
            deleted: false,
            child_ids: Vec::new(),
        };

        let child_b = Comment {
            id: 3,
            author: "child_b".to_string(),
            text: "Second child".to_string(),
            time_ago: "2m ago".to_string(),
            state: CommentState::Collapsed,
            depth: 1,
            deleted: false,
            child_ids: Vec::new(),
        };

        let top_level_a = Comment {
            id: 1,
            author: "parent".to_string(),
            text: "Parent".to_string(),
            time_ago: "now".to_string(),
            state: CommentState::Expanded {
                children: vec![child_a.clone(), child_b.clone()],
            },
            depth: 0,
            deleted: false,
            child_ids: vec![child_a.id, child_b.id],
        };

        let top_level_b = Comment {
            id: 4,
            author: "sibling".to_string(),
            text: "Top-level sibling".to_string(),
            time_ago: "5m ago".to_string(),
            state: CommentState::Collapsed,
            depth: 0,
            deleted: false,
            child_ids: Vec::new(),
        };

        let mut app = App::new();
        app.set_comments(vec![top_level_a, top_level_b.clone()]);

        // From the parent, jump to the next top-level sibling (skipping children)
        app.next_comment_sibling();
        assert_eq!(app.visible_comments[app.comment_cursor].1.id, top_level_b.id);

        // From the first child, jump to its next sibling
        app.comment_cursor = 1;
        app.next_comment_sibling();
        assert_eq!(app.visible_comments[app.comment_cursor].1.id, child_b.id);

        // When at the last sibling, cursor should stay in place
        app.comment_cursor = app.visible_comments.len() - 1;
        app.next_comment_sibling();
        assert_eq!(app.visible_comments[app.comment_cursor].1.id, top_level_b.id);
    }

    #[test]
    fn test_prev_comment_sibling_moves_up() {
        let child_a = Comment {
            id: 2,
            author: "child_a".to_string(),
            text: "First child".to_string(),
            time_ago: "1m ago".to_string(),
            state: CommentState::Collapsed,
            depth: 1,
            deleted: false,
            child_ids: Vec::new(),
        };

        let child_b = Comment {
            id: 3,
            author: "child_b".to_string(),
            text: "Second child".to_string(),
            time_ago: "2m ago".to_string(),
            state: CommentState::Collapsed,
            depth: 1,
            deleted: false,
            child_ids: Vec::new(),
        };

        let top_level = Comment {
            id: 1,
            author: "parent".to_string(),
            text: "Parent".to_string(),
            time_ago: "now".to_string(),
            state: CommentState::Expanded {
                children: vec![child_a.clone(), child_b.clone()],
            },
            depth: 0,
            deleted: false,
            child_ids: vec![child_a.id, child_b.id],
        };

        let mut app = App::new();
        app.set_comments(vec![top_level]);

        app.comment_cursor = 2; // child_b
        app.prev_comment_sibling();
        assert_eq!(app.visible_comments[app.comment_cursor].1.id, child_a.id);
    }

    #[test]
    fn test_parent_comment_navigates_up_tree() {
        let child = Comment {
            id: 2,
            author: "child".to_string(),
            text: "Child".to_string(),
            time_ago: "1m ago".to_string(),
            state: CommentState::Collapsed,
            depth: 1,
            deleted: false,
            child_ids: Vec::new(),
        };

        let parent = Comment {
            id: 1,
            author: "parent".to_string(),
            text: "Parent".to_string(),
            time_ago: "now".to_string(),
            state: CommentState::Expanded {
                children: vec![child.clone()],
            },
            depth: 0,
            deleted: false,
            child_ids: vec![child.id],
        };

        let mut app = App::new();
        app.set_comments(vec![parent]);

        // Move into child and then go to parent
        app.comment_cursor = 1;
        app.parent_comment();
        assert_eq!(app.visible_comments[app.comment_cursor].1.id, 1);

        // Top-level should no-op
        app.parent_comment();
        assert_eq!(app.visible_comments[app.comment_cursor].1.id, 1);
    }
}
