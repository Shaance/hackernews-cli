//! The feed's state machine: `hn_lib::feed_state`'s pure transitions, applied to owned fields.
//!
//! [`FeedModel::state`] hands the frozen contract a snapshot, the contract returns a
//! [`FeedAction`], and [`FeedModel::apply`] writes it back. The positive grid uses the same
//! saturating capacity as `feed_state::fill_grid`; the stepper remains the shared one.

use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;

use hn_lib::app::{FeedPosition, PageLanding, StoryType};
use hn_lib::feed_state::{self, FeedAction, FeedState, FeedStatus};
use hn_lib::HNCLIItem;

/// The title a rank whose item failed to load renders with. The rank keeps its slot, so the
/// list never shrinks silently.
pub const UNAVAILABLE: &str = "Story unavailable";

/// The window can show one or two columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Columns {
    One,
    Two,
}

impl Columns {
    pub const fn count(self) -> NonZeroUsize {
        match self {
            Self::One => NonZeroUsize::MIN,
            Self::Two => NonZeroUsize::MIN.saturating_add(1),
        }
    }
}

/// Measured feed geometry, clamped at the Rune boundary to at least one row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grid {
    pub rows: NonZeroUsize,
    pub columns: Columns,
}

impl Grid {
    pub const fn measured(rows: usize, columns: Columns) -> Self {
        Self { rows: match NonZeroUsize::new(rows) {
            Some(rows) => rows,
            None => NonZeroUsize::MIN,
        }, columns }
    }

    pub const fn capacity(self) -> NonZeroUsize {
        self.rows.saturating_mul(self.columns.count())
    }
}

/// One aggregate fill: the ranked window the viewport needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fill {
    pub tab: StoryType,
    pub from_rank: usize,
    pub count: NonZeroUsize,
}

/// Refresh and tab switches take a new ranking; paging and resize keep the shown ranks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ranking {
    Fresh,
    Keep,
}

/// The latest fill, ranking policy and measured grid, including after settlement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum RequestLifecycle {
    BeforeFirst,
    /// The clock starts at the first poll; a reflow keeps it until this request settles.
    Loading(Fill, Ranking, Grid, Option<f64>),
    Settled(Fill, Ranking, Grid),
}

impl RequestLifecycle {
    pub fn is_loading(self) -> bool {
        matches!(self, Self::Loading(..))
    }

    fn settle(&mut self) {
        if let Self::Loading(fill, ranking, grid, _) = *self {
            *self = Self::Settled(fill, ranking, grid);
        }
    }

    pub fn reset_clock(&mut self) {
        if let Self::Loading(_, _, _, since) = self { *since = None; }
    }
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
    pub from_rank: usize,
    /// A failed ranking keeps the current feed visible with a retry.
    pub outcome: Result<FeedWindow, String>,
}

#[derive(Debug, Clone)]
pub struct FeedWindow {
    pub ids: Vec<i32>,
    /// Successful items carry their own ID; failures retain the requested rank's ID.
    pub items: Vec<Result<HNCLIItem, i32>>,
}

pub struct FeedModel {
    pub tab: StoryType,
    pub id_snapshot: Vec<i32>,
    pub stories: Vec<HNCLIItem>,
    pub story_cache: HashMap<i32, HNCLIItem>,
    /// Ranks whose item failed to load; their `stories` entry is a placeholder.
    pub failed: HashSet<i32>,
    pub display_cursor: usize,
    /// The cursor a page turn is loading. `display_cursor` stays put until the new window
    /// lands, so the rows on screen keep their own ranks while the next page is in flight.
    pub pending_cursor: Option<usize>,
    /// The story a refresh or reflow must keep selected when its fill lands.
    pub pending_anchor: Option<i32>,
    pub selected: usize,
    pub(crate) request: RequestLifecycle,
    pub error: Option<String>,
    pub stale: bool,
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
            display_cursor: 0,
            pending_cursor: None,
            pending_anchor: None,
            selected: 0,
            request: RequestLifecycle::BeforeFirst,
            error: None,
            stale: false,
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
            page_size: feed_state::API_PAGE_SIZE,
            display_cursor: self.display_cursor,
            selected: self.selected,
            status: FeedStatus {
                loading: self.request.is_loading(),
                error: self.error.clone(),
                stale: self.stale,
            },
            stories_for: self.stories_for,
            feed_positions: self.feed_positions.clone(),
            page_landing: self.page_landing,
            bookmark_notice: self.bookmark_notice.clone(),
        }
    }

    /// The `‹ N ›` page number: current page only, no invented total.
    pub fn stepper(&self) -> u32 {
        feed_state::stepper(self.display_cursor, feed_state::API_PAGE_SIZE)
    }

    /// The rank the feed is loading towards: a pending page turn's cursor, else the shown one.
    pub fn target_cursor(&self) -> usize {
        self.pending_cursor.unwrap_or(self.display_cursor)
    }

    /// The fill a measured viewport needs right now.
    pub fn fill(&self, grid: Grid) -> Fill {
        Fill {
            tab: self.tab,
            from_rank: self.target_cursor(),
            count: grid.capacity(),
        }
    }

    /// The story id at the cursor, for re-anchoring a refresh by id rather than by index.
    pub fn selected_id(&self) -> Option<i32> {
        self.stories.get(self.selected).map(|story| story.id)
    }

    /// Apply a described change. `grid` is the viewport the caller measured.
    pub fn apply(&mut self, action: FeedAction, grid: Grid) -> Applied {
        match action {
            FeedAction::None => Applied::default(),
            FeedAction::Move { selected } => {
                // While a page turn is in flight the rows on screen are the page being left:
                // selecting one would select it under the ranks of the page being loaded.
                if self.request.is_loading() {
                    return Applied::default();
                }
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
                // One page turn at a time: a second one would page from a cursor that is not
                // on screen yet.
                if self.request.is_loading() {
                    return Applied::default();
                }
                self.pending_cursor = Some(display_cursor);
                if landing.is_some() {
                    self.page_landing = landing;
                }
                let fill = self.fill(grid);
                self.request = RequestLifecycle::Loading(fill, Ranking::Keep, grid, None);
                Applied {
                    changed: true,
                    fill: Some(fill),
                }
            }
            FeedAction::SwitchTab { tab, page } => {
                self.remember_position();
                self.tab = tab;
                self.display_cursor = (page.saturating_sub(1) as usize) * feed_state::API_PAGE_SIZE as usize;
                self.pending_cursor = None;
                self.pending_anchor = None;
                self.selected = 0;
                self.stories.clear();
                self.failed.clear();
                self.stories_for = None;
                self.page_landing = None;
                self.error = None;
                self.stale = false;
                let fill = self.fill(grid);
                self.request = RequestLifecycle::Loading(fill, Ranking::Fresh, grid, None);
                Applied {
                    changed: true,
                    fill: Some(fill),
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
                self.request.settle();
                Applied {
                    changed: true,
                    fill: None,
                }
            }
            FeedAction::Resize { .. } => Applied {
                changed: true,
                fill: Some(self.fill(grid)),
            },
        }
    }

    /// Take a settled fill. A fill for another tab or another cursor is stale and dropped.
    pub fn apply_fill(&mut self, filled: Filled) -> bool {
        if filled.tab != self.tab || filled.from_rank != self.target_cursor() {
            return false;
        }
        // A ranking that could not be fetched is an error state, not an empty feed: the window
        // the reader is looking at stays, and the banner offers a retry.
        let window = match filled.outcome {
            Ok(window) => window,
            Err(error) => {
                self.request.settle();
                self.error = Some(error);
                self.pending_cursor = None;
                return true;
            }
        };
        self.display_cursor = filled.from_rank;
        self.pending_cursor = None;
        self.id_snapshot = window.ids;
        self.stories.clear();
        self.failed.clear();
        for item in window.items {
            match item {
                Ok(item) => {
                    self.story_cache.insert(item.id, item.clone());
                    self.stories.push(item);
                }
                // A page the feed already holds is not dropped because this fill could not
                // return it: a shrink, or a transient item failure, keeps what was fetched.
                Err(id) => match self.story_cache.get(&id) {
                    Some(held) => self.stories.push(held.clone()),
                    None => {
                        self.failed.insert(id);
                        self.stories.push(placeholder(id));
                    }
                },
            }
        }
        self.request.settle();
        self.error = None;
        self.stories_for = Some((self.tab, self.stepper()));
        match self.page_landing.take() {
            Some(PageLanding::Start) => self.selected = 0,
            Some(PageLanding::End) => self.selected = self.stories.len().saturating_sub(1),
            None => self.selected = self.selected.min(self.stories.len().saturating_sub(1)),
        }
        // A refresh re-anchors on the story the reader had selected, by id: the ranking moved,
        // the story did not.
        if let Some(id) = self.pending_anchor.take() {
            self.anchor_on(id);
        }
        true
    }

    /// The Saved tab is the local store, not a feed: every saved story, in save-time order,
    /// with no request at all. The viewport applies to display only.
    pub fn apply_saved(&mut self, store: &[HNCLIItem]) -> bool {
        self.id_snapshot = store.iter().map(|story| story.id).collect();
        self.stories = store.to_vec();
        self.failed.clear();
        for story in store {
            self.story_cache.insert(story.id, story.clone());
        }
        self.display_cursor = 0;
        self.pending_cursor = None;
        self.pending_anchor = None;
        self.page_landing = None;
        self.selected = self.selected.min(self.stories.len().saturating_sub(1));
        self.request.settle();
        self.error = None;
        self.stale = false;
        self.stories_for = Some((StoryType::Saved, 1));
        true
    }

    /// Re-anchor on `id` after a refresh replaced the snapshot: the story keeps its selection
    /// and the window follows it, so a rank change never moves the reader to another story.
    /// A move of `display_cursor` changes [`FeedModel::fill`], which is what asks for the
    /// window the story now lives in.
    pub fn anchor_on(&mut self, id: i32) {
        let Some(rank) = self.id_snapshot.iter().position(|ranked| *ranked == id) else {
            return;
        };
        if rank >= self.display_cursor && rank < self.display_cursor + self.stories.len() {
            self.selected = rank - self.display_cursor;
        } else {
            self.display_cursor = rank;
            self.selected = 0;
        }
    }

    /// Poll the loading clock. `None` means no request, `Some(true)` a timeout.
    pub(crate) fn poll_timeout(&mut self, now: f64, timeout: f64) -> Option<bool> {
        let RequestLifecycle::Loading(fill, ranking, grid, since) = &mut self.request else {
            return None;
        };
        if now - *since.get_or_insert(now) < timeout { return Some(false); }
        self.request = RequestLifecycle::Settled(*fill, *ranking, *grid);
        self.error = Some("The feed did not load. Press r to retry.".to_string());
        self.pending_cursor = None;
        Some(true)
    }

    /// Whether the snapshot runs out before the window does: the "no more stories" footer.
    pub fn exhausted(&self, count: NonZeroUsize) -> bool {
        self.display_cursor + count.get() > self.id_snapshot.len() && !self.id_snapshot.is_empty()
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

pub(crate) fn tab_index(tab: StoryType) -> usize {
    match tab {
        StoryType::Top => 0,
        StoryType::New => 1,
        StoryType::Best => 2,
        StoryType::Saved => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    fn grid(rows: usize, columns: usize) -> Grid {
        Grid::measured(rows, match columns {
            1 => Columns::One,
            2 => Columns::Two,
            _ => panic!("invalid fixture columns"),
        })
    }

    fn filled(tab: StoryType, from_rank: usize, count: usize) -> Filled {
        let ids: Vec<i32> = (1..=30).collect();
        let items = (from_rank..(from_rank + count).min(ids.len()))
            .map(|rank| Ok(fixtures::item(rank, ids[rank])))
            .collect();
        Filled {
            tab,
            from_rank,
            outcome: Ok(FeedWindow { ids, items }),
        }
    }

    fn loaded(rows: usize, columns: usize) -> FeedModel {
        let mut feed = FeedModel::new();
        let fill = feed.fill(grid(rows, columns));
        feed.request = RequestLifecycle::Loading(fill, Ranking::Fresh, grid(rows, columns), None);
        feed.apply_fill(filled(fill.tab, fill.from_rank, fill.count.get()));
        feed
    }

    #[test]
    fn the_grid_is_the_shared_fill_formula() {
        let feed = FeedModel::new();
        assert_eq!(feed.request, RequestLifecycle::BeforeFirst);
        assert!(!feed.state().status.loading);
        for (rows, columns) in [(11, 1), (14, 2), (19, 2)] {
            let fill = feed.fill(grid(rows, columns));
            let (from_rank, count, _) = feed_state::fill_grid(rows, columns, feed_state::API_PAGE_SIZE, 0);
            assert_eq!((fill.from_rank, fill.count.get()), (from_rank, count));
        }
        assert_eq!(feed.stepper(), 1);
    }

    #[test]
    fn measured_grid_clamps_rows_and_saturates_capacity() {
        assert_eq!(Grid::measured(0, Columns::One).capacity().get(), 1);
        assert_eq!(Grid::measured(0, Columns::Two).capacity().get(), 2);
        assert_eq!(Grid::measured(usize::MAX, Columns::Two).capacity().get(), usize::MAX);
    }

    #[test]
    fn a_fill_keeps_rank_order_and_marks_failures_in_place() {
        let mut feed = FeedModel::new();
        feed.request = RequestLifecycle::Loading(feed.fill(grid(5, 1)), Ranking::Fresh, grid(5, 1), None);
        let mut fill = filled(StoryType::Top, 0, 5);
        fill.outcome.as_mut().unwrap().items[2] = Err(3);
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
        let applied = feed.apply(action, grid(11, 1));
        assert_eq!(feed.target_cursor(), 11, "the page being loaded");
        assert_eq!(feed.display_cursor, 0, "the page on screen until it lands");
        assert_eq!(feed.stepper(), 1, "11/30 + 1 is still page 1");
        assert_eq!(
            applied.fill,
            Some(Fill {
                tab: StoryType::Top,
                from_rank: 11,
                count: grid(11, 1).capacity()
            })
        );
        assert_eq!(feed.page_landing, Some(PageLanding::Start));
        feed.apply_fill(filled(StoryType::Top, 11, 11));
        assert_eq!(feed.display_cursor, 11);
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
        feed.apply(action, grid(11, 1));
        assert_eq!(feed.target_cursor(), 0);
        feed.apply_fill(filled(StoryType::Top, 0, 11));
        assert_eq!(feed.display_cursor, 0);
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
        let applied = feed.apply(action, grid(11, 1));
        assert_eq!(feed.tab, StoryType::New);
        assert_eq!(feed.display_cursor, 0);
        assert_eq!(
            applied.fill,
            Some(Fill {
                tab: StoryType::New,
                from_rank: 0,
                count: grid(11, 1).capacity()
            })
        );
        // Back to Top: the remembered page 2.
        let action = feed_state::switch_tab(&feed.state(), StoryType::Top);
        feed.apply(action, grid(11, 1));
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
        let applied = feed.apply(action, grid(19, 2));
        assert_eq!(feed.display_cursor, 0, "resize keeps the first visible rank");
        assert_eq!(
            applied.fill,
            Some(Fill {
                tab: StoryType::Top,
                from_rank: 0,
                count: grid(19, 2).capacity()
            })
        );
        // Shrinking back asks for fewer cells but never drops what it holds.
        let action = feed_state::resize(&feed.state(), 11, 1);
        let applied = feed.apply(action, grid(11, 1));
        assert_eq!(applied.fill.unwrap().count.get(), 11);
        assert_eq!(feed.stories.len(), 11, "the held window is untouched");
    }

    #[test]
    fn exhaustion_is_a_footer_state_not_a_shrunk_list() {
        let mut feed = FeedModel::new();
        feed.apply_fill(filled(StoryType::Top, 0, 30));
        assert!(!feed.exhausted(grid(11, 1).capacity()));
        assert!(feed.exhausted(grid(19, 2).capacity()), "38 cells past a 30-id snapshot");
    }

    #[test]
    fn a_refresh_re_anchors_on_the_selected_id() {
        let mut feed = loaded(11, 1);
        feed.selected = 4;
        let id = feed.selected_id().unwrap();
        // A refresh replaces the snapshot; the same story is now at another rank.
        let mut ids: Vec<i32> = (1..=30).collect();
        ids.rotate_left(3);
        let items = (0..11).map(|rank| Ok(fixtures::item(rank, ids[rank]))).collect();
        feed.apply_fill(Filled {
            tab: StoryType::Top,
            from_rank: 0,
            outcome: Ok(FeedWindow { ids, items }),
        });
        feed.anchor_on(id);
        assert_eq!(feed.selected_id(), Some(id));
    }

    #[test]
    fn a_refresh_that_moves_the_story_moves_the_window_with_it() {
        let mut feed = loaded(11, 1);
        feed.selected = 4;
        let id = feed.selected_id().unwrap();
        assert_eq!(id, 5);
        // The refresh puts the same story at rank 20, outside the window on screen.
        let mut ids: Vec<i32> = (1..=30).collect();
        ids.swap(4, 20);
        feed.pending_anchor = Some(id);
        let items = (0..11).map(|rank| Ok(fixtures::item(rank, ids[rank]))).collect();
        feed.apply_fill(Filled {
            tab: StoryType::Top,
            from_rank: 0,
            outcome: Ok(FeedWindow { ids: ids.clone(), items }),
        });
        assert_eq!(feed.display_cursor, 20, "the window follows the story");
        assert_eq!(feed.selected, 0);
        // The moved cursor is what asks for the window the story now lives in.
        assert_eq!(feed.fill(grid(11, 1)).from_rank, 20);
        let items = (20..30).map(|rank| Ok(fixtures::item(rank, ids[rank]))).collect();
        feed.apply_fill(Filled {
            tab: StoryType::Top,
            from_rank: 20,
            outcome: Ok(FeedWindow { ids, items }),
        });
        assert_eq!(feed.selected_id(), Some(id), "the same story stays selected");
    }

    #[test]
    fn a_refresh_that_keeps_the_story_in_the_window_keeps_the_window() {
        let mut feed = loaded(11, 1);
        feed.selected = 4;
        let id = feed.selected_id().unwrap();
        // The story moves from rank 5 to rank 2, still inside the window.
        let mut ids: Vec<i32> = (1..=30).collect();
        ids.swap(4, 1);
        feed.pending_anchor = Some(id);
        let items = (0..11).map(|rank| Ok(fixtures::item(rank, ids[rank]))).collect();
        feed.apply_fill(Filled {
            tab: StoryType::Top,
            from_rank: 0,
            outcome: Ok(FeedWindow { ids, items }),
        });
        assert_eq!(feed.display_cursor, 0, "no scroll for a story already on screen");
        assert_eq!(feed.selected_id(), Some(id));
        assert_eq!(feed.selected, 1);
    }

    #[test]
    fn a_page_turn_keeps_the_shown_page_until_the_new_one_lands() {
        let mut feed = loaded(11, 1);
        feed.selected = 10;
        let action = feed_state::next_story(&feed.state(), 11);
        feed.apply(action, grid(11, 1));
        // The rows on screen are still page 1, so they keep page 1's ranks and stepper.
        assert_eq!(feed.display_cursor, 0);
        assert_eq!(feed.stepper(), 1);
        assert_eq!(feed.stories[0].id, 1, "the old page is still what is shown");
        assert_eq!(feed.target_cursor(), 11, "the request is for the next page");
        assert_eq!(feed.fill(grid(11, 1)).from_rank, 11);
        // And nothing on the old page can be selected under the new ranks.
        assert_eq!(
            feed.apply(FeedAction::Move { selected: 3 }, grid(11, 1)),
            Applied::default()
        );
        assert_eq!(feed.selected, 10, "the selection did not move");
        // A second page turn is refused while the first is in flight.
        assert_eq!(
            feed.apply(feed_state::next_page(&feed.state(), 11), grid(11, 1)),
            Applied::default()
        );
        assert_eq!(feed.target_cursor(), 11);
        // The new page lands: the cursor moves and the old rows are replaced.
        feed.apply_fill(filled(StoryType::Top, 11, 11));
        assert_eq!(feed.display_cursor, 11);
        assert_eq!(feed.pending_cursor, None);
        assert_eq!(feed.stories[0].id, 12);
        assert_eq!(feed.selected, 0, "j lands on the first story of the page");
    }

    #[test]
    fn a_rejected_fill_does_not_leave_the_feed_loading_forever() {
        let mut feed = loaded(11, 1);
        feed.selected = 10;
        feed.apply(feed_state::next_story(&feed.state(), 11), grid(11, 1));
        assert!(feed.request.is_loading());
        let request = feed.fill(grid(11, 1));
        assert_eq!(feed.request, RequestLifecycle::Loading(request, Ranking::Keep, grid(11, 1), None));
        feed.error = Some("independent error".into());
        feed.stale = true;
        let state = feed.state();
        assert!(state.status.loading && state.status.stale);
        assert_eq!(state.status.error.as_deref(), Some("independent error"));
        // A fill for the page being left is stale and dropped.
        assert!(!feed.apply_fill(filled(StoryType::Top, 0, 11)));
        assert!(feed.request.is_loading(), "the newest request is still in flight");
        // The timeout is what ends the wait when no answer comes.
        assert_eq!(feed.poll_timeout(0.0, 10.0), Some(false));
        assert_eq!(feed.poll_timeout(1.0, 10.0), Some(false), "not yet");
        assert_eq!(feed.poll_timeout(10.0, 10.0), Some(true));
        assert!(!feed.request.is_loading());
        assert_eq!(feed.request, RequestLifecycle::Settled(request, Ranking::Keep, grid(11, 1)));
        assert!(!feed.state().status.loading && feed.state().status.stale);
        assert!(feed.error.is_some(), "the feed says it did not load");
        assert_eq!(feed.pending_cursor, None, "the pending turn is dropped");
        assert_eq!(feed.display_cursor, 0, "the page on screen is still page 1");
        assert_eq!(feed.stories[0].id, 1, "and it is still there");
    }

    #[test]
    fn a_ranking_failure_is_an_error_state_not_an_empty_feed() {
        let mut feed = loaded(11, 1);
        let before = feed.stories.len();
        assert!(feed.apply_fill(Filled {
            tab: StoryType::Top,
            from_rank: 0,
            outcome: Err("the ranking request failed".to_string()),
        }));
        assert_eq!(feed.stories.len(), before, "the window the reader has stays");
        assert!(!feed.request.is_loading());
        assert_eq!(
            feed.error.as_deref(),
            Some("the ranking request failed")
        );
        // A later successful fill clears it.
        feed.apply_fill(filled(StoryType::Top, 0, 11));
        assert!(feed.error.is_none());
    }

    #[test]
    fn a_shrink_keeps_the_pages_the_feed_already_holds() {
        let mut feed = loaded(19, 2);
        assert_eq!(feed.stories.len(), 30, "the whole 30-id snapshot");
        // Shrinking asks for fewer cells; the window shrinks with the viewport.
        let action = feed_state::resize(&feed.state(), 11, 1);
        let applied = feed.apply(action, grid(11, 1));
        assert_eq!(applied.fill.unwrap().count.get(), 11);
        feed.apply_fill(filled(StoryType::Top, 0, 11));
        assert_eq!(feed.stories.len(), 11);
        // Growing back is served from the held pages: a rank the source cannot return now
        // renders the story the feed already has, not a placeholder.
        let mut grow = filled(StoryType::Top, 0, 38);
        grow.outcome.as_mut().unwrap().items[20] = Err(21);
        feed.apply_fill(grow);
        assert_eq!(feed.stories.len(), 30);
        assert_eq!(feed.stories[20].id, 21, "the held page, not a placeholder");
        assert!(!feed.failed.contains(&21));
    }

    #[test]
    fn saved_renders_the_whole_store_not_the_viewport() {
        let mut feed = FeedModel::new();
        feed.tab = StoryType::Saved;
        let request = feed.fill(grid(11, 1));
        feed.request = RequestLifecycle::Loading(request, Ranking::Fresh, grid(11, 1), None);
        // Four saved stories, newest first, in a viewport that holds eleven cells.
        let store: Vec<HNCLIItem> = [11, 6, 2, 1]
            .into_iter()
            .enumerate()
            .map(|(index, id)| fixtures::item(index, id))
            .collect();
        assert!(feed.apply_saved(&store));
        assert_eq!(feed.request, RequestLifecycle::Settled(request, Ranking::Fresh, grid(11, 1)));
        assert_eq!(feed.stories.len(), 4, "every saved story, not a window of them");
        assert_eq!(
            feed.stories.iter().map(|story| story.id).collect::<Vec<_>>(),
            vec![11, 6, 2, 1],
            "save-time order"
        );
        assert_eq!(feed.id_snapshot, vec![11, 6, 2, 1]);
        assert!(!feed.request.is_loading(), "the store is local: nothing is in flight");
        assert_eq!(feed.stories_for, Some((StoryType::Saved, 1)));
        // The viewport still applies to display: the fill is the visible window.
        assert_eq!(feed.fill(grid(11, 1)).count.get(), 11);
    }
}
