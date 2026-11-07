//! The desktop's data seam: the frozen `hn_cli_service` paging seam, plus a deterministic fake.
//!
//! `Live` is the shipping path. It takes the ranked ids from `hn_client` (the snapshot the feed
//! pages against) and the items from `hn_cli_service::fetch_pages_against`, so HTML decoding,
//! `time_ago` and the URL fallback stay in `hn_lib` and are never reimplemented here.
//!
//! It is also the story-open seam: the article comes from `hn_lib::reader::sources::article`,
//! the discussion from the same `HackerNewsCliService` the feed uses, and the summary from
//! `hn_lib::reader::Network`. Nothing in this file builds a prompt, parses a provider response
//! or samples a comment tree — those are the reader's, and they stay there.
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
use hn_lib::reader::{
    self, Config, Discussion, DiscussionCapture, Network, Source as CapturedSource,
};
use hn_lib::{HNCLIItem, HackerNewsCliService, HackerNewsCliServiceImpl};

use crate::bridge::SummaryError;
use crate::fixtures;

/// How many reply levels one story open walks below the top-level comments. The depth cap is
/// four levels, so this leaves room for one `continue thread` re-root before the walk's edge.
const TREE_DEPTH: usize = 6;

/// How many comments one story open collects, across every level. A front-page thread can run to
/// thousands; the reader gets the top of it, and `r` refetches.
const TREE_NODES: usize = 200;

/// Where the feed's ranked ids, the story's article and its discussion come from.
pub enum Source {
    Live {
        client: HackerNewsClientImpl,
        service: HackerNewsCliServiceImpl,
        /// The reader's provider boundary: the same `Network` the terminal app calls.
        network: Network,
    },
    Fake(FakeService),
}

impl Source {
    /// The shipping source: the HN Firebase API and the reader's provider boundary, both
    /// through `hn_lib`.
    pub fn live() -> Source {
        Source::Live {
            client: HackerNewsClientImpl::new(),
            service: HackerNewsCliServiceImpl::new(),
            network: Network::default(),
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
            Source::Fake(fake) => match fake.ranking_error(tab) {
                Some(error) => Err(anyhow::anyhow!(error)),
                None => Ok(fake.ranked_ids(tab)),
            },
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

    /// The article's text and whether it was truncated, through the reader's own extractor.
    ///
    /// `sources::article` is the public half of the reader's capture: it fetches, extracts and
    /// bounds the text. The desktop never parses HTML.
    pub async fn article(&self, story: &HNCLIItem) -> std::result::Result<(String, bool), String> {
        match self {
            Source::Live { network, .. } => reader::sources::article(&network.client, story)
                .await
                .map_err(|failure| failure.message),
            Source::Fake(_) => Ok((fixtures::ARTICLE.to_string(), false)),
        }
    }

    /// The story's top-level comments, through the frozen service seam.
    pub async fn comments(&self, story_id: i32) -> Result<Vec<Comment>> {
        match self {
            Source::Live { service, .. } => service.fetch_top_level_comments(story_id).await,
            Source::Fake(fake) => fake.fetch_top_level_comments(story_id).await,
        }
    }

    /// The story's discussion as a tree: the top-level comments, then their replies level by
    /// level, through the same frozen seam the terminal reader uses.
    ///
    /// The walk is bounded twice — [`TREE_DEPTH`] levels and [`TREE_NODES`] comments — because
    /// one story open is one aggregate request and a front-page thread can run to thousands of
    /// comments. A level that fails to fetch stops the walk and keeps what landed: a partial
    /// thread is readable, and a row never claims a reply the capture does not hold.
    pub async fn thread(&self, story_id: i32) -> Result<Vec<Comment>> {
        let mut all = self.comments(story_id).await?;
        let mut frontier: Vec<i32> = all.iter().map(|c| c.id).collect();
        let mut depth = 1;
        while !frontier.is_empty() && depth <= TREE_DEPTH && all.len() < TREE_NODES {
            let children = match self {
                Source::Live { service, .. } => {
                    service.fetch_comment_children(&frontier, depth).await
                }
                Source::Fake(fake) => fake.fetch_comment_children(&frontier, depth).await,
            };
            let Ok(children) = children else { break };
            frontier = children.iter().map(|c| c.id).collect();
            all.extend(children);
            depth += 1;
        }
        Ok(all)
    }

    /// The captured sources a summary is written from: the same comments, with the excerpt the
    /// model sees and the full text the citation viewer shows.
    ///
    /// This is the desktop's own capture, not the reader's: `sources::discussion` (the terminal
    /// reader's 60-comment round-robin sampler) is `pub(super)` at `src/reader/sources.rs:55`,
    /// so it is not reachable from here. A top-level capture is what the milestone-2 discussion
    /// pane shows anyway; the coverage string says so rather than implying the whole tree.
    pub fn capture(&self, comments: &[Comment]) -> Vec<CapturedSource> {
        comments
            .iter()
            .filter(|comment| !comment.deleted)
            .map(|comment| {
                let text = reader::sources::clean(&comment.text);
                CapturedSource {
                    id: comment.id,
                    parent: None,
                    author: reader::sources::clean(&comment.author),
                    excerpt: text.chars().take(1800).collect(),
                    text,
                }
            })
            .collect()
    }

    /// The summary, through the reader's provider boundary.
    ///
    /// `Network::discussion` owns the prompt, the JSON schema, the citation check and the
    /// failure messages; the desktop only supplies the capture and the settings. The key guard
    /// sits on the live branch because it guards the provider call: the fake provider is a
    /// fixture, so a headless scene needs no key.
    pub async fn summary(
        &self,
        config: &Config,
        story: &HNCLIItem,
        sources: &[CapturedSource],
    ) -> std::result::Result<Discussion, SummaryError> {
        match self {
            Source::Live { network, .. } => {
                if config.key().is_empty() {
                    return Err(SummaryError::new("No summary API key. M opens settings.", true));
                }
                let captured = DiscussionCapture {
                    title: reader::sources::clean(&story.title),
                    post: reader::sources::clean(&story.text).chars().take(6000).collect(),
                    sources: sources.to_vec(),
                    coverage: format!("{} top-level comment excerpts", sources.len()),
                };
                network
                    .discussion(config.provider, config.key(), config.profile, &captured)
                    .await
                    .map_err(|failure| {
                        let auth = failure.is_auth();
                        SummaryError::new(failure.message, auth)
                    })
            }
            Source::Fake(_) => Ok(fixtures::summary()),
        }
    }
}

/// A deterministic `HackerNewsCliService`: fixed rankings, fixed items, optional failures and
/// an optional delay so a test can observe a request in flight.
pub struct FakeService {
    ids: Mutex<HashMap<StoryType, Vec<i32>>>,
    items: Mutex<HashMap<i32, HNCLIItem>>,
    failing: Mutex<HashSet<i32>>,
    failing_rankings: Mutex<HashSet<StoryType>>,
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
            failing_rankings: Mutex::new(HashSet::new()),
            delay: Duration::ZERO,
        }
    }

    /// An empty service, for tests that build their own ranking.
    pub fn empty() -> FakeService {
        FakeService {
            ids: Mutex::new(HashMap::new()),
            items: Mutex::new(HashMap::new()),
            failing: Mutex::new(HashSet::new()),
            failing_rankings: Mutex::new(HashSet::new()),
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

    /// Make `tab`'s ranking request fail, so the feed's error state is reachable in a test.
    pub fn fail_ranking(&self, tab: StoryType) {
        self.failing_rankings.lock().unwrap().insert(tab);
    }

    /// The failure a ranking request for `tab` reports, when a test asked for one.
    pub fn ranking_error(&self, tab: StoryType) -> Option<String> {
        self.failing_rankings
            .lock()
            .unwrap()
            .contains(&tab)
            .then(|| "the ranking request failed".to_string())
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
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        Ok(fixtures::comments())
    }

    async fn fetch_comment_children(
        &self,
        comment_ids: &[i32],
        _depth: usize,
    ) -> Result<Vec<Comment>> {
        Ok(fixtures::replies(comment_ids))
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

    #[tokio::test]
    async fn the_fake_serves_the_story_split_without_a_network() {
        let source = Source::fake();
        let story = fixtures::item(fixtures::STORY_INDEX, fixtures::STORY_ID);
        let (article, partial) = source.article(&story).await.unwrap();
        assert!(article.contains("```zig"), "the article is Markdown");
        assert!(!partial);
        let comments = source.comments(story.id).await.unwrap();
        assert_eq!(comments.len(), fixtures::comments().len());
        let sources = source.capture(&comments);
        assert_eq!(sources.len(), comments.len());
        assert_eq!(sources[0].id, 901);
        assert_eq!(sources[0].parent, None, "a top-level capture has no parents");
        // The fake provider answers without a key, so a headless scene never needs one.
        let summary = source
            .summary(&Config::default(), &story, &sources)
            .await
            .unwrap();
        assert_eq!(summary.points.len(), 3);
    }

    #[tokio::test]
    async fn a_live_reader_without_a_key_says_so_instead_of_calling_out() {
        // A live source with no key must not reach the provider. The guard is on the live
        // branch, so this asserts the guard rather than the fake's answer.
        let source = Source::live();
        let story = fixtures::item(fixtures::STORY_INDEX, fixtures::STORY_ID);
        let mut config = Config::default();
        config.api_key.clear();
        config.environment_key.clear();
        assert!(config.key().is_empty());
        let error = source
            .summary(&config, &story, &[])
            .await
            .expect_err("an empty key is a failure, not a call");
        assert!(error.message.contains("No summary API key"));
        assert!(error.auth, "a missing key is the auth case");
    }

    #[test]
    fn a_deleted_comment_is_not_a_captured_source() {
        let source = Source::fake();
        let mut comments = fixtures::comments();
        comments[1].deleted = true;
        let sources = source.capture(&comments);
        assert_eq!(sources.len(), comments.len() - 1);
        assert!(sources.iter().all(|source| source.id != 902));
    }
}
