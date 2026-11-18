//! Feed requests, saved stories and accepted arrivals.

use super::{App, Notice, Ranking, RequestLifecycle, Arrival, DEFAULT_GRID};
use hn_lib::app::StoryType;
use hn_lib::feed_state;
use crate::bridge::feed::Request;
use crate::feed::Filled;
use crate::feed::tab_index;

impl App {
    /// Ask for the window `grid` shows. `ranking` says whether this request re-snapshots.
    ///
    /// Saved is the local store, not a feed: it renders every saved story from memory and
    /// never reaches the source, so the viewport applies to display only.
    pub(super) fn request(&mut self, grid: (usize, usize), ranking: Ranking) {
        let fill = self.feed.fill(grid.0, grid.1);
        self.feed.request = RequestLifecycle::Loading(fill, ranking, grid);
        self.feed.error = None;
        if fill.tab == StoryType::Saved {
            self.feed.apply_saved(self.saved.store());
            self.accept_arrival();
            return;
        }
        let snapshot = match ranking {
            Ranking::Fresh => None,
            Ranking::Keep if !self.feed.id_snapshot.is_empty()
                && self.feed.stories_for.is_some_and(|(tab, _)| tab == self.feed.tab) => {
                Some(self.feed.id_snapshot.clone())
            }
            Ranking::Keep => None,
        };
        // A complete held window can settle immediately against the same ranked snapshot.
        // Fresh requests always reach the source; missing/failed items retain the normal fetch path.
        if let Some(ids) = snapshot.as_ref() {
            let end = (fill.from_rank + fill.count).min(ids.len());
            if let Some(window) = ids.get(fill.from_rank..end) {
                let held: Option<Vec<_>> = window.iter()
                    .map(|id| self.feed.story_cache.get(id).cloned().map(Ok)).collect();
                if let Some(items) = held {
                    self.bridge.cancel_feed();
                    self.feed.apply_fill(Filled { tab: fill.tab, ids: ids.clone(), from_rank: fill.from_rank, items, error: None });
                    self.accept_arrival();
                    return;
                }
            }
        }
        let request = Request::new(fill, snapshot);
        if self.headless {
            let filled = self.bridge.fill_now(self.source.as_ref(), request);
            self.feed.apply_fill(filled);
            self.accept_arrival();
        } else {
            self.bridge.request(self.source.clone(), request);
        }
    }

    /// Re-request the current window with a new ranking, as `r` does. The story the reader has
    /// selected stays selected: the feed re-anchors on its id when the new snapshot lands.
    pub(super) fn refresh(&mut self) {
        self.loading_since = None;
        self.pending_arrival = None;
        self.arrival = Arrival::None;
        self.feed.pending_anchor = self.feed.selected_id();
        let grid = self.measured.get().unwrap_or(DEFAULT_GRID);
        self.request(grid, Ranking::Fresh);
    }

    pub(super) fn toggle_saved(&mut self) {
        let Some(id) = self.feed.selected_id() else {
            return;
        };
        self.toggle_saved_id(id);
    }

    /// Save or unsave `id`, wherever the press came from: the row's hover action, or `s` on
    /// the selected row.
    pub(super) fn toggle_saved_id(&mut self, id: i32) {
        let Some(story) = self
            .feed
            .stories
            .iter()
            .find(|story| story.id == id)
            .cloned()
        else {
            return;
        };
        if self.feed.failed.contains(&story.id) {
            return;
        }
        let saved = self.saved.toggle(&story);
        // A store that could not be read or written says so, rather than confirming a save
        // that did not land.
        self.toast = Some(Notice {
            message: match (self.saved.error(), saved) {
                (Some(_), _) => "Saved stories unavailable · Reload in Saved".to_string(),
                (None, true) => "Saved · Press 4 for Saved".to_string(),
                (None, false) => "Removed from Saved".to_string(),
            },
            until: f64::INFINITY,
            show_dot: self.saved.error().is_some() || saved,
        });
        if self.feed.tab == StoryType::Saved {
            self.refresh();
        }
    }

    pub(super) fn apply(&mut self, action: feed_state::FeedAction) {
        let grid = self.measured.get().unwrap_or(DEFAULT_GRID);
        // A tab switch is a new feed and needs its own ranking; a page turn or a resize pages
        // against the one on screen.
        let ranking = match action {
            feed_state::FeedAction::SwitchTab { .. } => Ranking::Fresh,
            _ => Ranking::Keep,
        };
        let arrival = match action {
            feed_state::FeedAction::TurnPage { display_cursor, .. } => Some(Arrival::Page(
                if display_cursor < self.feed.display_cursor { -1 } else { 1 })),
            feed_state::FeedAction::SwitchTab { tab, .. } => Some(Arrival::Tab(
                if tab_index(tab) < tab_index(self.feed.tab) { -1 } else { 1 })),
            _ => None,
        };
        let applied = self.feed.apply(action, grid);
        if applied.fill.is_some() {
            self.loading_since = None;
            self.pending_arrival = arrival;
            self.request(grid, ranking);
        }
    }

    pub(super) fn accept_arrival(&mut self) -> bool {
        let Some(arrival) = self.pending_arrival.take() else { return false; };
        if self.feed.error.is_some() { self.arrival = Arrival::None; return false; }
        self.arrival = arrival;
        if arrival != Arrival::Initial {
            self.feed_revision += 1;
            self.hover = None;
        }
        true
    }

    pub(super) fn cells(&self) -> usize {
        let (rows, cols) = self.measured.get().unwrap_or(DEFAULT_GRID);
        rows * cols
    }
}
