//! The feed's state machine: `hn_lib::feed_state`'s pure transitions, applied to owned fields.
//!
//! `FeedModel` mirrors `FeedState` field for field, so every transition is the shared one:
//! [`FeedModel::state`] hands the frozen contract a snapshot, the contract returns a
//! [`FeedAction`], and [`FeedModel::apply`] writes it back. Paging arithmetic — the visible
//! window, the aligned API pages it needs, the stepper — is `feed_state::fill_grid` and
//! `feed_state::stepper`, never a second copy.

use std::collections::{HashMap, HashSet};

use hn_lib::app::{FeedPosition, PageLanding, StoryType};
use hn_lib::feed_state::{self, FeedAction, FeedState, FeedStatus};
use hn_lib::HNCLIItem;

/// The title a rank whose item failed to load renders with. The rank keeps its slot, so the
/// list never shrinks silently.
pub const UNAVAILABLE: &str = "Story unavailable";

/// One aggregate fill: the ranked window the viewport needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fill {
    pub tab: StoryType,
    pub from_rank: usize,
    pub count: usize,
}

/// What applying an action asks the caller to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Applied {
    pub changed: bool,
    /// The window to fetch, when the action invalidated the loaded one.
    pub fill: Option<Fill>,
}

/// A settled fill, as the async bridge hands it back.
#[derive(Debug, Clone)]
pub struct Filled {
    pub tab: StoryType,
    pub ids: Vec<i32>,
    pub from_rank: usize,
    pub items: Vec<Option<HNCLIItem>>,
}

pub struct FeedModel {
    pub tab: StoryType,
    pub id_snapshot: Vec<i32>,
    pub stories: Vec<HNCLIItem>,
    pub story_cache: HashMap<i32, HNCLIItem>,
    /// Ranks whose item failed to load; their `stories` entry is a placeholder.
    pub failed: HashSet<i32>,
    pub page_size: u32,
    pub display_cursor: usize,
    pub selected: usize,
    pub status: FeedStatus,
    pub stories_for: Option<(StoryType, u32)>,
    pub feed_positions: HashMap<StoryType, FeedPosition>,
    pub page_landing: Option<PageLanding>,
    pub bookmark_notice: Option<String>,
}

impl Default for FeedModel {
    fn default() -> Self {
        Self::new()
    }
}

impl FeedModel {
    pub fn new() -> FeedModel {
        FeedModel {
            tab: StoryType::Top,
            id_snapshot: Vec::new(),
            stories: Vec::new(),
            story_cache: HashMap::new(),
            failed: HashSet::new(),
            page_size: feed_state::API_PAGE_SIZE,
            display_cursor: 0,
            selected: 0,
            status: FeedStatus::default(),
            stories_for: None,
            feed_positions: HashMap::new(),
            page_landing: None,
            bookmark_notice: None,
        }
    }

    /// The frozen contract's view of this feed.
    pub fn state(&self) -> FeedState {
        FeedState {
            tab: self.tab,
            id_snapshot: self.id_snapshot.clone(),
            stories: self.stories.clone(),
            story_cache: self.story_cache.clone(),
            page_size: self.page_size,
            display_cursor: self.display_cursor,
            selected: self.selected,
            status: self.status.clone(),
            stories_for: self.stories_for,
            feed_positions: self.feed_positions.clone(),
            page_landing: self.page_landing,
            bookmark_notice: self.bookmark_notice.clone(),
        }
    }

    /// The ranked window a `rows` × `columns` viewport needs, from the current cursor.
    pub fn grid(&self, rows: usize, columns: usize) -> (usize, usize, u32) {
        feed_state::fill_grid(rows, columns, self.page_size, self.display_cursor)
    }

    /// The `‹ N ›` page number: current page only, no invented total.
    pub fn stepper(&self) -> u32 {
        feed_state::stepper(self.display_cursor, self.page_size)
    }

    /// The fill a `rows` × `columns` viewport needs right now.
    pub fn fill(&self, rows: usize, columns: usize) -> Fill {
        let (from_rank, count, _pages) = self.grid(rows, columns);
        Fill {
            tab: self.tab,
            from_rank,
            count,
        }
    }

    /// The story id at the cursor, for re-anchoring a refresh by id rather than by index.
    pub fn selected_id(&self) -> Option<i32> {
        self.stories.get(self.selected).map(|story| story.id)
    }

    /// Apply a described change. `grid` is the viewport the caller measured.
    pub fn apply(&mut self, action: FeedAction, grid: (usize, usize)) -> Applied {
        match action {
            FeedAction::None => Applied::default(),
            FeedAction::Move { selected } => {
                self.selected = selected;
                Applied {
                    changed: true,
                    fill: None,
                }
            }
            FeedAction::TurnPage {
                display_cursor,
                landing,
            } => {
                self.display_cursor = display_cursor;
                if landing.is_some() {
                    self.page_landing = landing;
                }
                self.status.loading = true;
                Applied {
                    changed: true,
                    fill: Some(self.fill(grid.0, grid.1)),
                }
            }
            FeedAction::SwitchTab { tab, page } => {
                self.remember_position();
                self.tab = tab;
                self.display_cursor = (page.saturating_sub(1) as usize) * self.page_size as usize;
                self.selected = 0;
                self.stories.clear();
                self.failed.clear();
                self.stories_for = None;
                self.page_landing = None;
                self.status = FeedStatus {
                    loading: true,
                    ..FeedStatus::default()
                };
                Applied {
                    changed: true,
                    fill: Some(self.fill(grid.0, grid.1)),
                }
            }
            FeedAction::ToggleSaved { id, saved } => {
                self.bookmark_notice = Some(if saved {
                    format!("Saved · Press 4 for Saved")
                } else {
                    format!("Removed from Saved")
                });
                let _ = id;
                Applied {
                    changed: true,
                    fill: None,
                }
            }
            FeedAction::ApplyPage { tab, page, .. } => {
                self.stories_for = Some((tab, page));
                self.status.loading = false;
                Applied {
                    changed: true,
                    fill: None,
                }
            }
            FeedAction::Resize { .. } => Applied {
                changed: true,
                fill: Some(self.fill(grid.0, grid.1)),
            },
        }
    }

    /// Take a settled fill. A fill for another tab or another cursor is stale and dropped.
    pub fn apply_fill(&mut self, filled: Filled) -> bool {
        if filled.tab != self.tab || filled.from_rank != self.display_cursor {
            return false;
        }
        self.id_snapshot = filled.ids;
        self.stories.clear();
        self.failed.clear();
        for (offset, item) in filled.items.into_iter().enumerate() {
            let rank = filled.from_rank + offset;
            let id = self
                .id_snapshot
                .get(rank)
                .copied()
                .unwrap_or(-(rank as i32) - 1);
            match item {
                Some(item) => {
                    self.story_cache.insert(item.id, item.clone());
                    self.stories.push(item);
                }
                None => {
                    self.failed.insert(id);
                    self.stories.push(placeholder(id));
                }
            }
        }
        self.status.loading = false;
        self.status.error = None;
        self.stories_for = Some((self.tab, self.stepper()));
        match self.page_landing.take() {
            Some(PageLanding::Start) => self.selected = 0,
            Some(PageLanding::End) => self.selected = self.stories.len().saturating_sub(1),
            None => self.selected = self.selected.min(self.stories.len().saturating_sub(1)),
        }
        true
    }

    /// Re-anchor the cursor on `id` after a refresh replaced the snapshot.
    pub fn anchor_on(&mut self, id: i32) {
        if let Some(index) = self.stories.iter().position(|story| story.id == id) {
            self.selected = index;
        }
    }

    /// Whether the snapshot runs out before the window does: the "no more stories" footer.
    pub fn exhausted(&self, count: usize) -> bool {
        self.display_cursor + count > self.id_snapshot.len() && !self.id_snapshot.is_empty()
    }

    fn remember_position(&mut self) {
        if let Some((feed, page)) = self.stories_for {
            self.feed_positions.insert(
                feed,
                FeedPosition {
                    page,
                    selected_id: self.selected_id(),
                    index: self.selected,
                    scroll: 0,
                },
            );
        }
    }
}

fn placeholder(id: i32) -> HNCLIItem {
    HNCLIItem {
        id,
        title: UNAVAILABLE.to_string(),
        url: String::new(),
        author: String::new(),
        time: String::new(),
        time_ago: String::new(),
        score: 0,
        comments: None,
        text: String::new(),
        links: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    fn filled(tab: StoryType, from_rank: usize, count: usize) -> Filled {
        let ids: Vec<i32> = (1..=30).collect();
        let items = (from_rank..(from_rank + count).min(ids.len()))
            .map(|rank| Some(fixtures::item(rank, ids[rank])))
            .collect();
        Filled {
            tab,
            ids,
            from_rank,
            items,
        }
    }

    fn loaded(rows: usize, columns: usize) -> FeedModel {
        let mut feed = FeedModel::new();
        let fill = feed.fill(rows, columns);
        feed.apply_fill(filled(fill.tab, fill.from_rank, fill.count));
        feed
    }

    #[test]
    fn the_grid_is_the_shared_fill_formula() {
        let feed = FeedModel::new();
        assert_eq!(feed.grid(11, 1), (0, 11, 1));
        assert_eq!(feed.grid(14, 2), (0, 28, 1));
        assert_eq!(feed.grid(19, 2), (0, 38, 2));
        assert_eq!(feed.stepper(), 1);
    }

    #[test]
    fn a_fill_keeps_rank_order_and_marks_failures_in_place() {
        let mut feed = FeedModel::new();
        let mut fill = filled(StoryType::Top, 0, 5);
        fill.items[2] = None;
        assert!(feed.apply_fill(fill));
        assert_eq!(feed.stories.len(), 5, "a failed item never shrinks the window");
        assert_eq!(feed.stories[2].title, UNAVAILABLE);
        assert!(feed.failed.contains(&3));
        assert_eq!(feed.stories[3].id, 4);
        assert_eq!(feed.stories_for, Some((StoryType::Top, 1)));
    }

    #[test]
    fn a_stale_fill_is_dropped() {
        let mut feed = loaded(11, 1);
        let before = feed.stories.len();
        // Another tab's fill.
        assert!(!feed.apply_fill(filled(StoryType::New, 0, 11)));
        // Another cursor's fill.
        assert!(!feed.apply_fill(filled(StoryType::Top, 11, 11)));
        assert_eq!(feed.stories.len(), before);
    }

    #[test]
    fn j_past_the_last_story_turns_the_page_by_the_visible_cell_count() {
        let mut feed = loaded(11, 1);
        feed.selected = 10;
        let action = feed_state::next_story(&feed.state(), 11);
        let applied = feed.apply(action, (11, 1));
        assert_eq!(feed.display_cursor, 11);
        assert_eq!(feed.stepper(), 1, "11/30 + 1 is still page 1");
        assert_eq!(
            applied.fill,
            Some(Fill {
                tab: StoryType::Top,
                from_rank: 11,
                count: 11
            })
        );
        assert_eq!(feed.page_landing, Some(PageLanding::Start));
        feed.apply_fill(filled(StoryType::Top, 11, 11));
        assert_eq!(feed.selected, 0, "j lands on the first story of the page");
        assert_eq!(feed.stories[0].id, 12);
    }

    #[test]
    fn k_at_the_top_turns_back_and_lands_on_the_last_story() {
        let mut feed = loaded(11, 1);
        feed.display_cursor = 11;
        feed.apply_fill(filled(StoryType::Top, 11, 11));
        feed.selected = 0;
        let action = feed_state::prev_story(&feed.state(), 11);
        feed.apply(action, (11, 1));
        assert_eq!(feed.display_cursor, 0);
        feed.apply_fill(filled(StoryType::Top, 0, 11));
        assert_eq!(feed.selected, 10, "k lands on the last story of the page");
    }

    #[test]
    fn a_two_column_grid_fills_column_major() {
        let feed = loaded(14, 2);
        assert_eq!(feed.stories.len(), 28);
        // Column-major: rank 1..=14 down the left column, 15..=28 down the right.
        let per_column = feed.stories.len().div_ceil(2);
        assert_eq!(per_column, 14);
        assert_eq!(feed.stories[0].id, 1);
        assert_eq!(feed.stories[13].id, 14);
        assert_eq!(feed.stories[14].id, 15);
    }

    #[test]
    fn switching_tabs_remembers_the_page_and_restores_it() {
        let mut feed = loaded(11, 1);
        feed.display_cursor = 22;
        feed.apply_fill(filled(StoryType::Top, 22, 8));
        assert_eq!(feed.stepper(), 1, "22/30 + 1 is page 1");
        feed.display_cursor = 30;
        feed.apply_fill(filled(StoryType::Top, 30, 0));
        assert_eq!(feed.stepper(), 2);
        let action = feed_state::switch_tab(&feed.state(), StoryType::New);
        let applied = feed.apply(action, (11, 1));
        assert_eq!(feed.tab, StoryType::New);
        assert_eq!(feed.display_cursor, 0);
        assert_eq!(
            applied.fill,
            Some(Fill {
                tab: StoryType::New,
                from_rank: 0,
                count: 11
            })
        );
        // Back to Top: the remembered page 2.
        let action = feed_state::switch_tab(&feed.state(), StoryType::Top);
        feed.apply(action, (11, 1));
        assert_eq!(feed.display_cursor, 30);
        assert_eq!(feed.stepper(), 2);
    }

    #[test]
    fn saved_has_no_pages() {
        let mut feed = loaded(11, 1);
        feed.tab = StoryType::Saved;
        assert_eq!(
            feed_state::next_page(&feed.state(), 11),
            FeedAction::None,
            "Saved is one list, not a paged feed"
        );
    }

    #[test]
    fn a_resize_keeps_the_cursor_and_asks_for_more_pages() {
        let mut feed = loaded(11, 1);
        let action = feed_state::resize(&feed.state(), 19, 2);
        let applied = feed.apply(action, (19, 2));
        assert_eq!(feed.display_cursor, 0, "resize keeps the first visible rank");
        assert_eq!(
            applied.fill,
            Some(Fill {
                tab: StoryType::Top,
                from_rank: 0,
                count: 38
            })
        );
        // Shrinking back asks for fewer cells but never drops what it holds.
        let action = feed_state::resize(&feed.state(), 11, 1);
        let applied = feed.apply(action, (11, 1));
        assert_eq!(applied.fill.unwrap().count, 11);
        assert_eq!(feed.stories.len(), 11, "the held window is untouched");
    }

    #[test]
    fn exhaustion_is_a_footer_state_not_a_shrunk_list() {
        let mut feed = FeedModel::new();
        feed.apply_fill(filled(StoryType::Top, 0, 30));
        assert!(!feed.exhausted(11));
        assert!(feed.exhausted(38), "38 cells past a 30-id snapshot");
    }

    #[test]
    fn a_refresh_re_anchors_on_the_selected_id() {
        let mut feed = loaded(11, 1);
        feed.selected = 4;
        let id = feed.selected_id().unwrap();
        // A refresh replaces the snapshot; the same story is now at another rank.
        let mut ids: Vec<i32> = (1..=30).collect();
        ids.rotate_left(3);
        let items = (0..11).map(|rank| Some(fixtures::item(rank, ids[rank]))).collect();
        feed.apply_fill(Filled {
            tab: StoryType::Top,
            ids,
            from_rank: 0,
            items,
        });
        feed.anchor_on(id);
        assert_eq!(feed.selected_id(), Some(id));
    }
}
