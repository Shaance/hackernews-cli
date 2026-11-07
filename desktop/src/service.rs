//! The desktop's data seam: the frozen `hn_cli_service` paging seam, plus a deterministic fake.
//!
//! `Live` is the shipping path. It takes the ranked ids from `hn_client` (the snapshot the feed
//! pages against) and the items from `hn_cli_service::fetch_pages_against`, so HTML decoding,
//! `time_ago` and the URL fallback stay in `hn_lib` and are never reimplemented here.
//!
//! `Fake` is the deterministic path: headless scenes and tests. It is a real
//! `HackerNewsCliService` impl, so the provided `fetch_pages_against` composes it exactly as it
//! composes the live service.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use hn_lib::app::{Comment, StoryType};
use hn_lib::hn_client::{HackerNewsClient, HackerNewsClientImpl};
use hn_lib::{HNCLIItem, HackerNewsCliService, HackerNewsCliServiceImpl};

use crate::fixtures;

/// Where the feed's ranked ids and items come from.
pub enum Source {
    Live {
        client: HackerNewsClientImpl,
        service: HackerNewsCliServiceImpl,
    },
    Fake(FakeService),
}

impl Source {
    /// The shipping source: the HN Firebase API through `hn_lib`.
    pub fn live() -> Source {
        Source::Live {
            client: HackerNewsClientImpl::new(),
            service: HackerNewsCliServiceImpl::new(),
        }
    }

    /// The deterministic source: one fixture page per tab, no network.
    pub fn fake() -> Source {
        Source::Fake(FakeService::fixture())
    }

    pub fn is_live(&self) -> bool {
        matches!(self, Source::Live { .. })
    }

    /// The ranked ids for `tab`: the snapshot the feed pages against.
    pub async fn ranked_ids(&self, tab: StoryType) -> Result<Vec<i32>> {
        match self {
            Source::Live { client, .. } => client.get_story_ids(tab.as_str()).await,
            Source::Fake(fake) => Ok(fake.ranked_ids(tab)),
        }
    }

    /// The ranked window `[from_rank, from_rank + count)` against `ids`.
    pub async fn fetch_pages_against(
        &self,
        ids: &[i32],
        from_rank: usize,
        count: usize,
    ) -> Vec<Option<HNCLIItem>> {
        match self {
            Source::Live { service, .. } => service.fetch_pages_against(ids, from_rank, count).await,
            Source::Fake(fake) => fake.fetch_pages_against(ids, from_rank, count).await,
        }
    }
}

/// A deterministic `HackerNewsCliService`: fixed rankings, fixed items, optional failures and
/// an optional delay so a test can observe a request in flight.
pub struct FakeService {
    ids: Mutex<HashMap<StoryType, Vec<i32>>>,
    items: Mutex<HashMap<i32, HNCLIItem>>,
    failing: Mutex<HashSet<i32>>,
    delay: Duration,
}

impl FakeService {
    /// One fixture page per tab, with distinct ids so a tab switch is visible in the tree.
    pub fn fixture() -> FakeService {
        let mut ids = HashMap::new();
        let mut items = HashMap::new();
        for (offset, tab) in [StoryType::Top, StoryType::New, StoryType::Best]
            .into_iter()
            .enumerate()
        {
            let base = offset as i32 * 100;
            let ranked: Vec<i32> = (1..=30).map(|rank| base + rank).collect();
            for (index, id) in ranked.iter().enumerate() {
                items.insert(*id, fixtures::item(index, *id));
            }
            ids.insert(tab, ranked);
        }
        let saved: Vec<i32> = fixtures::saved_ids();
        ids.insert(StoryType::Saved, saved);
        FakeService {
            ids: Mutex::new(ids),
            items: Mutex::new(items),
            failing: Mutex::new(HashSet::new()),
            delay: Duration::ZERO,
        }
    }

    /// An empty service, for tests that build their own ranking.
    pub fn empty() -> FakeService {
        FakeService {
            ids: Mutex::new(HashMap::new()),
            items: Mutex::new(HashMap::new()),
            failing: Mutex::new(HashSet::new()),
            delay: Duration::ZERO,
        }
    }

    /// Replace a tab's ranking, as a refresh does.
    pub fn set_ranking(&self, tab: StoryType, ids: Vec<i32>) {
        self.ids.lock().unwrap().insert(tab, ids);
    }

    pub fn insert_item(&self, item: HNCLIItem) {
        self.items.lock().unwrap().insert(item.id, item);
    }

    /// Make `id` fail to fetch, so it renders as a placeholder row.
    pub fn fail(&self, id: i32) {
        self.failing.lock().unwrap().insert(id);
    }

    /// Hold every fetch for `delay`, so a test can see a request in flight.
    pub fn with_delay(mut self, delay: Duration) -> FakeService {
        self.delay = delay;
        self
    }

    pub fn ranked_ids(&self, tab: StoryType) -> Vec<i32> {
        self.ids
            .lock()
            .unwrap()
            .get(&tab)
            .cloned()
            .unwrap_or_default()
    }
}

#[async_trait]
impl HackerNewsCliService for FakeService {
    async fn fetch_stories_page(
        &self,
        story_type: &str,
        page_size: u8,
        page: u32,
    ) -> Result<Vec<HNCLIItem>> {
        let tab = match story_type {
            "new" => StoryType::New,
            "best" => StoryType::Best,
            "saved" => StoryType::Saved,
            _ => StoryType::Top,
        };
        let ids = self.ranked_ids(tab);
        let start = ((page.max(1) - 1) as usize) * page_size as usize;
        let end = (start + page_size as usize).min(ids.len());
        if start >= end {
            return Ok(Vec::new());
        }
        Ok(self
            .fetch_items_by_ids(&ids[start..end])
            .await
            .into_iter()
            .flatten()
            .collect())
    }

    async fn fetch_top_level_comments(&self, _story_id: i32) -> Result<Vec<Comment>> {
        Ok(Vec::new())
    }

    async fn fetch_comment_children(
        &self,
        _comment_ids: &[i32],
        _depth: usize,
    ) -> Result<Vec<Comment>> {
        Ok(Vec::new())
    }

    fn get_valid_story_types() -> HashSet<&'static str> {
        HashSet::from(["best", "new", "top"])
    }

    async fn fetch_items_by_ids(&self, ids: &[i32]) -> Vec<Option<HNCLIItem>> {
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        let items = self.items.lock().unwrap();
        let failing = self.failing.lock().unwrap();
        ids.iter()
            .map(|id| {
                if failing.contains(id) {
                    None
                } else {
                    items.get(id).cloned()
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_fake_pages_against_a_snapshot_through_the_provided_seam() {
        let fake = FakeService::fixture();
        let ids = fake.ranked_ids(StoryType::Top);
        assert_eq!(ids.len(), 30);
        // The provided `fetch_pages_against` slices the snapshot and composes
        // `fetch_items_by_ids`; the fake never overrides it.
        let window = fake.fetch_pages_against(&ids, 0, 11).await;
        assert_eq!(window.len(), 11);
        assert_eq!(window[0].as_ref().unwrap().id, 1);
        assert_eq!(window[10].as_ref().unwrap().id, 11);
        // Past the end of the snapshot the window is short, not padded: the caller renders
        // its "no more stories" footer instead of placeholder rows.
        let tail = fake.fetch_pages_against(&ids, 28, 11).await;
        assert_eq!(tail.len(), 2);
        let past = fake.fetch_pages_against(&ids, 30, 11).await;
        assert!(past.is_empty());
    }

    #[tokio::test]
    async fn a_failed_item_is_none_in_place_and_never_shrinks_the_window() {
        let fake = FakeService::fixture();
        fake.fail(3);
        let ids = fake.ranked_ids(StoryType::Top);
        let window = fake.fetch_pages_against(&ids, 0, 5).await;
        assert_eq!(window.len(), 5);
        assert!(window[2].is_none(), "rank 3 keeps its slot");
        assert_eq!(window[3].as_ref().unwrap().id, 4);
    }

    #[tokio::test]
    async fn a_changed_ranking_is_what_the_next_request_sees() {
        let fake = FakeService::fixture();
        fake.set_ranking(StoryType::Top, vec![9, 8, 7]);
        let ids = fake.ranked_ids(StoryType::Top);
        assert_eq!(ids, vec![9, 8, 7]);
        let window = fake.fetch_pages_against(&ids, 0, 3).await;
        assert_eq!(
            window.iter().flatten().map(|item| item.id).collect::<Vec<_>>(),
            vec![9, 8, 7]
        );
    }

    #[tokio::test]
    async fn tabs_have_distinct_rankings() {
        let fake = FakeService::fixture();
        assert_eq!(fake.ranked_ids(StoryType::Top)[0], 1);
        assert_eq!(fake.ranked_ids(StoryType::New)[0], 101);
        assert_eq!(fake.ranked_ids(StoryType::Best)[0], 201);
        assert_eq!(fake.ranked_ids(StoryType::Saved), fixtures::saved_ids());
    }
}
