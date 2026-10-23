//! Application state management for HackerNews TUI

use crate::{theme::Theme, HNCLIItem};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

#[cfg(test)]
mod app_tests;
mod comment_refresh;
mod comment_view_state;
#[cfg(test)]
mod comment_view_state_tests;
mod reading_session;
mod view_status;
pub use comment_view_state::{Comment, CommentState};
use reading_session::ReadingSession;
use view_status::ViewStatus;

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
    Saved,
}

impl StoryType {
    pub fn as_str(&self) -> &'static str {
        match self {
            StoryType::Best => "best",
            StoryType::New => "new",
            StoryType::Top => "top",
            StoryType::Saved => "saved",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            StoryType::Best => "Best",
            StoryType::New => "New",
            StoryType::Top => "Top",
            StoryType::Saved => "Saved",
        }
    }
}

/// Last successfully displayed location in one feed for this session.
struct FeedPosition {
    page: u32,
    selected_id: Option<i32>,
    index: usize,
    scroll: usize,
}

/// Main application state
pub struct App {
    theme: Theme,
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
    /// Comments for current story
    pub comments: Vec<Comment>,
    /// Paths to comments visible in the flattened render/navigation order
    visible_comment_paths: Vec<Vec<usize>>,
    /// Currently selected comment in visible list
    pub comment_cursor: usize,
    /// Scroll offset for comments view
    pub comment_scroll: usize,
    pub discussion_story: Option<HNCLIItem>,
    pub show_links: bool,
    pub overlay_opened: Option<Instant>,
    pub save_flash: Option<(i32, Instant)>,
    pub link_cursor: usize,
    pub bookmarks: Vec<HNCLIItem>,
    pub bookmark_notice: Option<String>,
    reading_sessions: HashMap<i32, ReadingSession>,
    feed_positions: HashMap<StoryType, FeedPosition>,
    comment_line_ranges: Vec<(i32, usize, usize)>,
    comment_viewport_height: usize,
    comment_anchor: Option<(i32, usize)>,
    refreshing_comments: bool,
    follow_comment_selection: bool,
    pending_comment_pages: i32,
    /// Monotonic token used to ignore stale async comment responses
    comment_view_generation: u64,
    /// Monotonic token used to ignore stale async story responses
    story_request_generation: u64,
    /// Monotonic token used to ignore stale child-comment responses
    comment_child_load_generation: u64,
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
    story_status: ViewStatus,
    comment_status: ViewStatus,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    /// Create a new application instance
    pub fn new() -> Self {
        Self::with_theme(Theme::default())
    }

    pub fn theme(&self) -> Theme {
        self.theme
    }

    pub fn with_theme(theme: Theme) -> Self {
        let mut app = Self {
            theme,
            view: View::Stories,
            story_type: StoryType::Best,
            stories: Vec::new(),
            selected_index: 0,
            story_scroll: 0,
            current_page: 1,
            comments: Vec::new(),
            visible_comment_paths: Vec::new(),
            comment_cursor: 0,
            comment_scroll: 0,
            discussion_story: None,
            show_links: false,
            overlay_opened: None,
            save_flash: None,
            link_cursor: 0,
            bookmarks: Vec::new(),
            bookmark_notice: None,
            reading_sessions: HashMap::new(),
            feed_positions: HashMap::new(),
            comment_line_ranges: Vec::new(),
            comment_viewport_height: 0,
            comment_anchor: None,
            refreshing_comments: false,
            follow_comment_selection: false,
            pending_comment_pages: 0,
            comment_view_generation: 0,
            story_request_generation: 0,
            comment_child_load_generation: 0,
            should_quit: false,
            show_help: false,
            page_size: 20,
            story_cache: HashMap::new(),
            stories_for: None,
            story_status: ViewStatus::default(),
            comment_status: ViewStatus::default(),
        };

        app.set_story_loading(true);
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
        if self.story_type == StoryType::Saved {
            return;
        }
        self.current_page += 1;
    }

    /// Go to previous page
    pub fn prev_page(&mut self) {
        if self.story_type == StoryType::Saved {
            return;
        }
        if self.current_page > 1 {
            self.current_page -= 1;
        }
    }

    /// Switch story type
    pub fn set_story_type(&mut self, story_type: StoryType) {
        if self.story_type != story_type {
            self.remember_feed_position();
            self.story_type = story_type;
            self.current_page = self.feed_positions.get(&story_type).map_or(1, |p| p.page);
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

    /// Start a new story request and return the token async responses must echo.
    pub fn next_story_request_generation(&mut self) -> u64 {
        self.story_request_generation = self.story_request_generation.wrapping_add(1);
        self.story_request_generation
    }

    /// Whether a story response still matches the latest request for the current page/type.
    pub fn is_current_story_request(
        &self,
        story_type: StoryType,
        page: u32,
        request_generation: u64,
    ) -> bool {
        self.story_type == story_type
            && self.current_page == page
            && self.story_request_generation == request_generation
    }

    // === View Management ===

    /// Switch to comments view
    pub fn view_comments(&mut self, story_id: i32, story_title: String, story_url: String) {
        self.reset_comment_view_state();
        self.view = View::Comments {
            story_id,
            story_title,
            story_url,
        };
        self.discussion_story = self
            .stories
            .iter()
            .find(|story| story.id == story_id)
            .cloned();
        if self.restore_reading_session(story_id) {
            return;
        }
        self.set_comment_loading(true);
    }

    /// Switch back to stories view
    pub fn view_stories(&mut self) {
        self.store_reading_session();
        self.reset_comment_view_state();
        self.clear_comment_status();
        self.view = View::Stories;
    }

    fn reset_comment_view_state(&mut self) {
        self.comments.clear();
        self.comment_line_ranges.clear();
        self.comment_anchor = None;
        self.refreshing_comments = false;
        self.visible_comment_paths.clear();
        self.comment_cursor = 0;
        self.comment_scroll = 0;
        self.discussion_story = None;
        self.show_links = false;
        self.follow_comment_selection = false;
        self.pending_comment_pages = 0;
        self.comment_view_generation = self.comment_view_generation.wrapping_add(1);
    }

    /// Token for the current comments view. Async responses must echo this to be applied.
    pub fn comment_view_generation(&self) -> u64 {
        self.comment_view_generation
    }

    /// Toggle help overlay
    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
        if self.show_help {
            self.overlay_opened = self.theme.motion.then(Instant::now);
        }
    }

    pub fn has_active_motion(&self, now: Instant) -> bool {
        self.theme.motion
            && (((self.show_help || self.show_links)
                && self.overlay_opened.is_some_and(|start| {
                    now.saturating_duration_since(start) < Duration::from_millis(120)
                }))
                || self.save_flash.is_some_and(|(_, start)| {
                    now.saturating_duration_since(start) < Duration::from_millis(300)
                }))
    }

    /// Apply only to the saved star cell identified by the renderer.
    pub fn render_save_flash(
        &self,
        buf: &mut ratatui::buffer::Buffer,
        area: ratatui::layout::Rect,
        id: i32,
    ) {
        if let Some((_, start)) = self
            .save_flash
            .filter(|(saved, _)| self.theme.motion && *saved == id && self.is_bookmarked(*saved))
        {
            tachyonfx::fx::fade_from_fg(
                ratatui::style::Color::Rgb(0xff, 0xf6, 0xea),
                (300, tachyonfx::Interpolation::QuadOut),
            )
            .with_color_space(tachyonfx::ColorSpace::Rgb)
            .process(start.elapsed().into(), buf, area);
        }
    }

    // === State Updates ===

    /// Set stories
    pub fn set_stories(&mut self, stories: Vec<HNCLIItem>) {
        self.stories = stories;
        self.clear_story_status();

        // Ensure selected index is valid
        if self.selected_index >= self.stories.len() && !self.stories.is_empty() {
            self.selected_index = self.stories.len() - 1;
        }
    }

    /// Set stories and record their source type/page
    pub fn set_stories_for(&mut self, story_type: StoryType, page: u32, stories: Vec<HNCLIItem>) {
        self.remember_feed_position();
        let position = self
            .feed_positions
            .get(&story_type)
            .filter(|p| p.page == page);
        self.selected_index = position.map_or(0, |p| {
            p.selected_id
                .and_then(|id| stories.iter().position(|story| story.id == id))
                .unwrap_or(p.index)
                .min(stories.len().saturating_sub(1))
        });
        self.story_scroll = position.map_or(0, |p| p.scroll.min(stories.len().saturating_sub(1)));
        self.stories_for = Some((story_type, page));
        self.set_stories(stories);
    }

    fn remember_feed_position(&mut self) {
        if let Some((feed, page)) = self.stories_for {
            self.feed_positions.insert(
                feed,
                FeedPosition {
                    page,
                    selected_id: self.selected_story().map(|s| s.id),
                    index: self.selected_index,
                    scroll: self.story_scroll,
                },
            );
        }
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

    /// Apply a story page load failure without discarding currently usable stories.
    pub fn apply_stories_error(&mut self, error: String) {
        self.set_story_error(error);
    }

    /// Get cached stories for the current selection, if available
    pub fn cached_stories(&self) -> Option<Vec<HNCLIItem>> {
        self.story_cache
            .get(&(self.story_type, self.current_page))
            .cloned()
    }

    /// Set comments
    pub fn set_comments(&mut self, comments: Vec<Comment>) {
        if self.refreshing_comments {
            self.apply_comment_refresh(comments);
            return;
        }
        self.comments = comments;
        self.rebuild_visible_comments();
        self.comment_cursor = self
            .comment_cursor
            .min(self.visible_comment_count().saturating_sub(1));
        self.clear_comment_status();
        self.recompute_comment_loading(self.has_loading_comments());
    }
}
