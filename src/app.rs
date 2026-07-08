//! Application state management for HackerNews TUI

use crate::HNCLIItem;
use std::collections::HashMap;
use std::time::{Duration, Instant};

mod comment_view_state;
#[cfg(test)]
mod comment_view_state_tests;
pub use comment_view_state::{Comment, CommentState};

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
    /// Paths to comments visible in the flattened render/navigation order
    visible_comment_paths: Vec<Vec<usize>>,
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
            visible_comment_paths: Vec::new(),
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

    // === View Management ===

    /// Switch to comments view
    pub fn view_comments(&mut self, story_id: i32, story_title: String, story_url: String) {
        self.view = View::Comments {
            story_id,
            story_title,
            story_url,
        };
        self.comments.clear();
        self.visible_comment_paths.clear();
        self.comment_cursor = 0;
        self.set_loading(true);
    }

    /// Switch back to stories view
    pub fn view_stories(&mut self) {
        self.view = View::Stories;
        self.comments.clear();
        self.visible_comment_paths.clear();
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
}
