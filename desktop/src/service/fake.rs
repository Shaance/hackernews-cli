//! Deterministic service data and delays for desktop scenes and tests.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use hn_lib::app::{Comment, StoryType};
use hn_lib::reader::{Article, Discussion};
use hn_lib::{HNCLIItem, HackerNewsCliService};

use crate::fixtures;
use crate::story_io::SummaryError;

type FakeArticle = dyn Fn(i32) -> Pin<Box<dyn Future<Output = std::result::Result<(String, bool), String>> + Send>> + Send + Sync;
type FakeArticleSummary = dyn Fn(i32) -> Pin<Box<dyn Future<Output = std::result::Result<Article, SummaryError>> + Send>> + Send + Sync;
type FakeSummary = dyn Fn(i32) -> Pin<Box<dyn Future<Output = std::result::Result<Discussion, SummaryError>> + Send>> + Send + Sync;

/// A deterministic `HackerNewsCliService`: fixed rankings, fixed items, optional failures and
/// an optional delay so a test can observe a request in flight.
pub struct FakeService {
    ids: Mutex<HashMap<StoryType, Vec<i32>>>,
    items: Mutex<HashMap<i32, HNCLIItem>>,
    failing: Mutex<HashSet<i32>>,
    failing_rankings: Mutex<HashSet<StoryType>>,
    delay: Duration,
    pub requests: Mutex<(usize, usize)>,
    pub comments: Mutex<Vec<Comment>>,
    pub failing_comments: Mutex<HashSet<i32>>,
    pub comment_requests: Mutex<Vec<Vec<i32>>>,
    pub summary_requests: Mutex<usize>,
    pub article_summary_requests: Mutex<usize>,
    pub(crate) article: Arc<FakeArticle>,
    pub(crate) article_summary: Arc<FakeArticleSummary>,
    pub(crate) summary: Arc<FakeSummary>,
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
            requests: Mutex::new((0, 0)),
            comments: Mutex::new(fixtures::thread()),
            failing_comments: Mutex::new(HashSet::new()),
            comment_requests: Mutex::new(Vec::new()),
            summary_requests: Mutex::new(0),
            article_summary_requests: Mutex::new(0),
            article: Arc::new(|_| Box::pin(async { Ok((fixtures::ARTICLE.to_string(), false)) })),
            article_summary: Arc::new(|_| Box::pin(async { Ok(Article { text: "The article describes Libghostty and its design.".to_string() }) })),
            summary: Arc::new(|_| Box::pin(async { Ok(fixtures::summary()) })),
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
            requests: Mutex::new((0, 0)),
            comments: Mutex::new(Vec::new()),
            failing_comments: Mutex::new(HashSet::new()),
            comment_requests: Mutex::new(Vec::new()),
            summary_requests: Mutex::new(0),
            article_summary_requests: Mutex::new(0),
            article: Arc::new(|_| Box::pin(async { Ok((fixtures::ARTICLE.to_string(), false)) })),
            article_summary: Arc::new(|_| Box::pin(async { Ok(Article { text: "The article describes Libghostty and its design.".to_string() }) })),
            summary: Arc::new(|_| Box::pin(async { Ok(fixtures::summary()) })),
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
        self.requests.lock().unwrap().0 += 1;
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

    async fn fetch_top_level_comments(&self, story_id: i32) -> Result<Vec<Comment>> {
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        if self.failing_comments.lock().unwrap().contains(&story_id) { anyhow::bail!("Discussion unavailable"); }
        Ok(self.comments.lock().unwrap().iter().filter(|c| c.depth == 0).cloned().collect())
    }

    async fn fetch_comment_children(
        &self,
        comment_ids: &[i32],
        depth: usize,
    ) -> Result<Vec<Comment>> {
        self.comment_requests.lock().unwrap().push(comment_ids.to_vec());
        if !self.delay.is_zero() { tokio::time::sleep(self.delay).await; }
        let comments = self.comments.lock().unwrap();
        let failed = self.failing_comments.lock().unwrap();
        let out: Vec<_> = comment_ids.iter().filter(|id| !failed.contains(id)).filter_map(|id|
            comments.iter().find(|c| c.id == *id).cloned().map(|mut c| { c.depth = depth; c })).collect();
        if out.is_empty() && !comment_ids.is_empty() { anyhow::bail!("Replies unavailable"); }
        Ok(out)
    }

    fn get_valid_story_types() -> HashSet<&'static str> {
        HashSet::from(["best", "new", "top"])
    }

    async fn fetch_items_by_ids(&self, ids: &[i32]) -> Vec<Option<HNCLIItem>> {
        self.requests.lock().unwrap().1 += 1;
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

