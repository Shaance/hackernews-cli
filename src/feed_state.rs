//! Shared feed contract: the pure feed state and the transitions both frontends use.
//!
//! `App` keeps field ownership. These functions take a `&FeedState` snapshot plus
//! explicit inputs and return a `FeedAction` describing the change, which the
//! caller applies to its own fields. No TUI types leak: only `StoryType`,
//! `HNCLIItem`, `FeedPosition` and `PageLanding`.

use crate::app::{FeedPosition, PageLanding, StoryType};
use crate::HNCLIItem;
use std::collections::HashMap;

/// One HN API page: 30 ranked ids.
pub const API_PAGE_SIZE: u32 = 30;

/// Request status only. `loading`, `error` and `stale` coexist (they mirror
/// `ViewStatus`); this is deliberately not an enum.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FeedStatus {
    pub loading: bool,
    pub error: Option<String>,
    pub stale: bool,
}

/// The feed fields a transition reads. Mirrors the matching `App` fields.
#[derive(Debug, Clone)]
pub struct FeedState {
    pub tab: StoryType,
    /// Stable ranked ids for the tab; a refresh replaces it explicitly.
    pub id_snapshot: Vec<i32>,
    /// Loaded items for the current window.
    pub stories: Vec<HNCLIItem>,
    pub story_cache: HashMap<i32, HNCLIItem>,
    pub page_size: u32,
    /// Rank offset of the first visible story (0-based).
    pub display_cursor: usize,
    /// Index into the visible window.
    pub selected: usize,
    pub status: FeedStatus,
    pub stories_for: Option<(StoryType, u32)>,
    pub feed_positions: HashMap<StoryType, FeedPosition>,
    pub page_landing: Option<PageLanding>,
    pub bookmark_notice: Option<String>,
}

/// A described change the caller applies to its own fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedAction {
    /// Nothing to do.
    None,
    /// Selection moved inside the visible window.
    Move { selected: usize },
    /// One page turn; `display_cursor` is the new first visible rank. A `None`
    /// landing leaves the caller's pending landing untouched.
    TurnPage {
        display_cursor: usize,
        landing: Option<PageLanding>,
    },
    /// Tab switch; `page` is the page to show for `tab`.
    SwitchTab { tab: StoryType, page: u32 },
    /// Save toggle for `id`; `saved` is the state after the toggle.
    ToggleSaved { id: i32, saved: bool },
    /// A page of items arrived for `tab`/`page`.
    ApplyPage {
        tab: StoryType,
        page: u32,
        from_rank: usize,
        count: usize,
    },
    /// Viewport changed; refill the grid from `from_rank`.
    Resize {
        from_rank: usize,
        count: usize,
        pages_to_fetch: u32,
    },
}

/// Ranks the grid must hold: `[display_cursor, display_cursor + visibleCells)`.
///
/// Returns `(from_rank, count, pages_to_fetch)`: the first rank to show, how many
/// ranks the window holds, and how many aligned `page_size` API pages intersect
/// that window. Shrinking never drops held pages; the caller keeps its cache.
pub fn fill_grid(
    visible_rows: usize,
    columns: usize,
    page_size: u32,
    display_cursor: usize,
) -> (usize, usize, u32) {
    let count = visible_rows.saturating_mul(columns);
    if count == 0 || page_size == 0 {
        return (display_cursor, 0, 0);
    }
    let page_size = page_size as usize;
    let first_page = display_cursor / page_size;
    let last_page = display_cursor.saturating_add(count - 1) / page_size;
    (display_cursor, count, (last_page - first_page + 1) as u32)
}

/// 1-based page number shown next to the feed: `floor(display_cursor / page_size) + 1`.
pub fn stepper(display_cursor: usize, page_size: u32) -> u32 {
    if page_size == 0 {
        return 1;
    }
    (display_cursor / page_size as usize) as u32 + 1
}

/// `step` is how many ranks one page turn advances: the visible cell count for a
/// cursor-aware grid, `page_size` for a page-numbered feed.
pub fn next_story(state: &FeedState, step: usize) -> FeedAction {
    if state.stories.is_empty() || state.status.stale {
        return FeedAction::None;
    }
    if state.selected + 1 < state.stories.len() {
        return FeedAction::Move {
            selected: state.selected + 1,
        };
    }
    if state.status.loading {
        return FeedAction::None;
    }
    turn_page(state, step, Some(PageLanding::Start))
}

/// `step` as in [`next_story`].
pub fn prev_story(state: &FeedState, step: usize) -> FeedAction {
    if state.status.stale {
        return FeedAction::None;
    }
    if state.selected > 0 {
        return FeedAction::Move {
            selected: state.selected - 1,
        };
    }
    if state.status.loading {
        return FeedAction::None;
    }
    turn_page_back(state, step, Some(PageLanding::End))
}

/// `step` as in [`next_story`].
pub fn next_page(state: &FeedState, step: usize) -> FeedAction {
    turn_page(state, step, None)
}

/// `step` as in [`next_story`].
pub fn prev_page(state: &FeedState, step: usize) -> FeedAction {
    turn_page_back(state, step, None)
}

/// Switch to `tab`, restoring the page that feed was last shown on.
pub fn switch_tab(state: &FeedState, tab: StoryType) -> FeedAction {
    if state.tab == tab {
        return FeedAction::None;
    }
    // The caller remembers the displayed feed's position before switching, so a
    // displayed-but-not-yet-remembered page wins over the stored entry.
    let page = match state.stories_for {
        Some((feed, page)) if feed == tab => page,
        _ => state.feed_positions.get(&tab).map_or(1, |p| p.page),
    };
    FeedAction::SwitchTab { tab, page }
}

/// The grid to fill after a viewport change, from the current cursor.
pub fn resize(state: &FeedState, visible_rows: usize, columns: usize) -> FeedAction {
    let (from_rank, count, pages_to_fetch) =
        fill_grid(visible_rows, columns, state.page_size, state.display_cursor);
    FeedAction::Resize {
        from_rank,
        count,
        pages_to_fetch,
    }
}

fn turn_page(state: &FeedState, step: usize, landing: Option<PageLanding>) -> FeedAction {
    if state.tab == StoryType::Saved {
        return FeedAction::None;
    }
    FeedAction::TurnPage {
        display_cursor: state.display_cursor.saturating_add(step),
        landing,
    }
}

fn turn_page_back(state: &FeedState, step: usize, landing: Option<PageLanding>) -> FeedAction {
    if state.tab == StoryType::Saved || state.display_cursor == 0 {
        return FeedAction::None;
    }
    FeedAction::TurnPage {
        display_cursor: state.display_cursor.saturating_sub(step),
        landing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(tab: StoryType, page_size: u32, display_cursor: usize, len: usize) -> FeedState {
        FeedState {
            tab,
            id_snapshot: (0..len as i32).collect(),
            stories: (0..len)
                .map(|id| HNCLIItem {
                    id: id as i32,
                    title: format!("Story {id}"),
                    url: "http://example.com".to_string(),
                    author: "user".to_string(),
                    time: "2023-01-01".to_string(),
                    time_ago: "1h ago".to_string(),
                    score: 100,
                    comments: Some(10),
                    text: String::new(),
                    links: Vec::new(),
                })
                .collect(),
            story_cache: HashMap::new(),
            page_size,
            display_cursor,
            selected: 0,
            status: FeedStatus::default(),
            stories_for: None,
            feed_positions: HashMap::new(),
            page_landing: None,
            bookmark_notice: None,
        }
    }

    #[test]
    fn fill_grid_eleven_cells_fetches_one_page() {
        assert_eq!(fill_grid(11, 1, API_PAGE_SIZE, 0), (0, 11, 1));
        assert_eq!(stepper(0, API_PAGE_SIZE), 1);
    }

    #[test]
    fn fill_grid_twenty_eight_cells_fetches_one_page() {
        assert_eq!(fill_grid(28, 1, API_PAGE_SIZE, 0), (0, 28, 1));
        assert_eq!(stepper(0, API_PAGE_SIZE), 1);
        // Advancing once at 28 cells lands on rank 28, still inside page 1.
        assert_eq!(fill_grid(28, 1, API_PAGE_SIZE, 28), (28, 28, 2));
        assert_eq!(stepper(28, API_PAGE_SIZE), 1);
        assert_eq!(stepper(30, API_PAGE_SIZE), 2);
    }

    #[test]
    fn fill_grid_thirty_eight_cells_fetches_two_pages() {
        assert_eq!(fill_grid(38, 1, API_PAGE_SIZE, 0), (0, 38, 2));
        assert_eq!(stepper(0, API_PAGE_SIZE), 1);
    }

    #[test]
    fn fill_grid_two_columns_doubles_the_window() {
        assert_eq!(fill_grid(19, 2, API_PAGE_SIZE, 0), (0, 38, 2));
        assert_eq!(fill_grid(14, 2, API_PAGE_SIZE, 0), (0, 28, 1));
    }

    #[test]
    fn fill_grid_resize_growth_keeps_the_cursor_and_adds_pages() {
        let narrow = fill_grid(11, 1, API_PAGE_SIZE, 0);
        let wide = fill_grid(38, 1, API_PAGE_SIZE, 0);
        assert_eq!(narrow, (0, 11, 1));
        assert_eq!(wide, (0, 38, 2));
        assert_eq!(narrow.0, wide.0, "resize keeps the first visible rank");
        // Shrinking back never asks for fewer held pages than it already has.
        assert_eq!(fill_grid(11, 1, API_PAGE_SIZE, 0).2, 1);
    }

    #[test]
    fn fill_grid_rank_38_to_39_is_contiguous() {
        let (from, count, pages) = fill_grid(38, 1, API_PAGE_SIZE, 0);
        assert_eq!((from, count, pages), (0, 38, 2));
        let (next_from, next_count, next_pages) = fill_grid(38, 1, API_PAGE_SIZE, from + count);
        assert_eq!((next_from, next_count, next_pages), (38, 38, 2));
        assert_eq!(from + count, next_from, "no rank is skipped or repeated");
        // Rank 38 lives in page 1 (ranks 30..=59), so the two windows share it.
        assert_eq!(38 / API_PAGE_SIZE as usize, 1);
    }

    #[test]
    fn fill_grid_empty_viewport_fetches_nothing() {
        assert_eq!(fill_grid(0, 1, API_PAGE_SIZE, 0), (0, 0, 0));
        assert_eq!(fill_grid(10, 0, API_PAGE_SIZE, 0), (0, 0, 0));
        assert_eq!(fill_grid(10, 1, 0, 0), (0, 0, 0));
    }

    #[test]
    fn next_story_moves_then_turns_the_page() {
        let mut feed = state(StoryType::Best, 20, 0, 3);
        assert_eq!(next_story(&feed, 20), FeedAction::Move { selected: 1 });
        feed.selected = 2;
        assert_eq!(
            next_story(&feed, 20),
            FeedAction::TurnPage {
                display_cursor: 20,
                landing: Some(PageLanding::Start)
            }
        );
        feed.status.loading = true;
        assert_eq!(next_story(&feed, 20), FeedAction::None);
    }

    #[test]
    fn prev_story_at_the_top_turns_back_to_the_previous_page() {
        let mut feed = state(StoryType::Best, 20, 40, 3);
        assert_eq!(
            prev_story(&feed, 20),
            FeedAction::TurnPage {
                display_cursor: 20,
                landing: Some(PageLanding::End)
            }
        );
        feed.display_cursor = 0;
        assert_eq!(prev_story(&feed, 20), FeedAction::None);
        feed.display_cursor = 40;
        feed.selected = 1;
        assert_eq!(prev_story(&feed, 20), FeedAction::Move { selected: 0 });
    }

    #[test]
    fn saved_has_no_pages() {
        let feed = state(StoryType::Saved, 20, 0, 3);
        assert_eq!(next_page(&feed, 20), FeedAction::None);
        assert_eq!(prev_page(&feed, 20), FeedAction::None);
    }

    #[test]
    fn switch_tab_prefers_the_displayed_page_over_the_stored_one() {
        let mut feed = state(StoryType::New, 20, 0, 3);
        feed.stories_for = Some((StoryType::Top, 3));
        feed.feed_positions.insert(
            StoryType::Top,
            FeedPosition {
                page: 5,
                selected_id: None,
                index: 0,
                scroll: 0,
            },
        );
        assert_eq!(
            switch_tab(&feed, StoryType::Top),
            FeedAction::SwitchTab {
                tab: StoryType::Top,
                page: 3
            }
        );
        feed.stories_for = Some((StoryType::New, 1));
        assert_eq!(
            switch_tab(&feed, StoryType::Top),
            FeedAction::SwitchTab {
                tab: StoryType::Top,
                page: 5
            }
        );
        assert_eq!(switch_tab(&feed, StoryType::New), FeedAction::None);
    }

    #[test]
    fn resize_reports_the_grid_to_fill() {
        let feed = state(StoryType::Best, API_PAGE_SIZE, 0, 3);
        assert_eq!(
            resize(&feed, 38, 1),
            FeedAction::Resize {
                from_rank: 0,
                count: 38,
                pages_to_fetch: 2
            }
        );
    }
}
