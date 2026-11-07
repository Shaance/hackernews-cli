//! The async bridge: one aggregate request at a time, driven off the UI thread.
//!
//! The desktop owns a tokio runtime. `AppSpec::setup` spawns the first fill; the
//! [`Latest`] worker closure drives the request to completion on that runtime, and `poll`
//! takes the answer. Cancellation is `Latest` generations only: a newer request supersedes
//! an older one, and a stale answer never lands.
//!
//! Two kinds of request, each with its own `Latest` so opening a story never cancels the feed
//! and a page turn never cancels a story:
//!
//! - [`Request`] — one aggregate fill of the feed's visible window.
//! - [`StoryRequest`] — one aggregate story open: the article capture, the discussion, and the
//!   summary job, in that order, on one worker.
//!
//! Headless runs and tests use [`Bridge::fill_now`] and [`Bridge::story_now`], which run the
//! same requests on the same runtime but block, so a scene is deterministic and needs no frame
//! budget.

use std::sync::Arc;

use anyhow::Result;
use hn_lib::app::Comment;
use hn_lib::reader::{Article, Config, Discussion, Source as CapturedSource};
use hn_lib::HNCLIItem;
use rune::{Latest, Waker};
use tokio::runtime::{Handle, Runtime};

use crate::feed::{Fill, Filled};
use crate::service::Source;

/// One fill request: the window, plus the ranking to page it against.
///
/// `snapshot` is the feed's `id_snapshot`. A page turn or a resize pages against the ranking
/// the feed already has, so the ranks on screen stay the ranks that were shown; only a refresh
/// (or the first load of a tab) asks the source for a new one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub fill: Fill,
    pub snapshot: Option<Vec<i32>>,
}

impl Request {
    /// A request that pages against `snapshot`, taking a fresh one only when there is none.
    pub fn new(fill: Fill, snapshot: Option<Vec<i32>>) -> Request {
        Request { fill, snapshot }
    }
}

pub struct Bridge {
    runtime: Runtime,
    handle: Handle,
    latest: Latest<Filled>,
    story: Latest<StoryFilled>,
}

/// One story open: the item to read, and the summary settings to read it with.
///
/// The `Config` is the reader's own, loaded once at launch; it is never logged, printed or
/// written into a tree.
pub struct StoryRequest {
    pub story: HNCLIItem,
    pub config: Config,
}

/// The aggregate answer to one story open.
///
/// The three stages run in order on one worker, so the card's "N of 3 sources captured" counts
/// something real. A failed article is a `String`; a failed summary is a [`SummaryError`],
/// because the card acts on the one classification the reader boundary exposes.
pub struct StoryFilled {
    pub story_id: i32,
    pub article: Result<Article, String>,
    pub article_partial: bool,
    pub comments: Vec<Comment>,
    pub sources: Vec<CapturedSource>,
    pub summary: Result<Discussion, SummaryError>,
}

/// A provider failure, with the one classification the summary card acts on.
///
/// `hn_lib::reader::Failure` keeps its `auth` field private but exposes `is_auth()`, so the
/// window can offer the key prompt without reading the message text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryError {
    pub message: String,
    /// The provider rejected the key: the retry card offers settings, not another attempt.
    pub auth: bool,
}

impl SummaryError {
    pub fn new(message: impl Into<String>, auth: bool) -> SummaryError {
        SummaryError {
            message: message.into(),
            auth,
        }
    }
}

impl Bridge {
    pub fn new() -> Result<Bridge> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("hn-desktop-io")
            .build()?;
        let handle = runtime.handle().clone();
        Ok(Bridge {
            runtime,
            handle,
            latest: Latest::default(),
            story: Latest::default(),
        })
    }

    /// Attach the waker `AppSpec::setup` hands over, so a landed answer runs `poll`.
    pub fn set_waker(&self, waker: Waker) {
        self.latest.set_waker(waker.clone());
        self.story.set_waker(waker);
    }

    /// Ask for `request`, superseding any earlier one. The work runs on the runtime from a
    /// `Latest` worker thread; only the newest request's answer lands.
    pub fn request(&self, source: Arc<Source>, request: Request) {
        let handle = self.handle.clone();
        self.latest.request(request, move |request| {
            handle.block_on(run(source.as_ref(), request))
        });
    }

    /// Ask for one story open, superseding any earlier one. A second open cancels the first
    /// story's answer, which is what "Latest-only" means here.
    pub fn request_story(&self, source: Arc<Source>, request: StoryRequest) {
        let handle = self.handle.clone();
        self.story.request(request, move |request| {
            handle.block_on(run_story(source.as_ref(), request))
        });
    }

    /// The same request, run to completion on this thread. Headless scenes and tests use it so
    /// the first frame already has its rows.
    pub fn fill_now(&self, source: &Source, request: Request) -> Filled {
        self.runtime.block_on(run(source, request))
    }

    /// The story open, run to completion on this thread.
    pub fn story_now(&self, source: &Source, request: StoryRequest) -> StoryFilled {
        self.runtime.block_on(run_story(source, request))
    }

    /// The newest answer, once.
    pub fn take(&self) -> Option<Filled> {
        self.latest.take()
    }

    /// The newest story answer, once.
    pub fn take_story(&self) -> Option<StoryFilled> {
        self.story.take()
    }

    /// Whether a request is still in flight.
    pub fn is_pending(&self) -> bool {
        self.latest.is_pending()
    }

    /// Whether a story open is still in flight.
    pub fn is_story_pending(&self) -> bool {
        self.story.is_pending()
    }

    /// Drop the story open and any answer not yet taken, leaving the feed's request alone.
    pub fn cancel_story(&self) {
        self.story.cancel();
    }

    /// Drop every request and any answer not yet taken.
    pub fn cancel(&self) {
        self.latest.cancel();
        self.story.cancel();
    }
}

/// The one request shape: take the ranking (or the caller's), then fetch the window against it.
///
/// A ranking that cannot be fetched is an error the feed renders with a retry, not an empty
/// list: `unwrap_or_default` here would turn a network failure into "no stories".
async fn run(source: &Source, request: Request) -> Filled {
    let ids = match request.snapshot {
        Some(ids) => ids,
        None => match source.ranked_ids(request.fill.tab).await {
            Ok(ids) => ids,
            Err(error) => {
                return Filled {
                    tab: request.fill.tab,
                    ids: Vec::new(),
                    from_rank: request.fill.from_rank,
                    items: Vec::new(),
                    error: Some(format!("{error:#}")),
                };
            }
        },
    };
    let items = source
        .fetch_pages_against(&ids, request.fill.from_rank, request.fill.count)
        .await;
    Filled {
        tab: request.fill.tab,
        ids,
        from_rank: request.fill.from_rank,
        items,
        error: None,
    }
}

/// The one story-open shape: article, then discussion, then the summary over that discussion.
///
/// The order is the card's provenance: the article and the discussion are the two captures the
/// summary is written from, so a summary is never requested over a discussion that failed to
/// load. A failed article does not stop the discussion — the reader can still read the thread.
async fn run_story(source: &Source, request: StoryRequest) -> StoryFilled {
    let story_id = request.story.id;
    let (article, article_partial) = match source.article(&request.story).await {
        Ok((text, partial)) => (Ok(Article { text }), partial),
        Err(message) => (Err(message), false),
    };
    let comments = source.comments(story_id).await.unwrap_or_default();
    let sources = source.capture(&comments);
    let summary = source
        .summary(&request.config, &request.story, &sources)
        .await;
    StoryFilled {
        story_id,
        article,
        article_partial,
        comments,
        sources,
        summary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hn_lib::app::StoryType;
    use std::time::Duration;

    fn fill(tab: StoryType, from_rank: usize, count: usize) -> Fill {
        Fill {
            tab,
            from_rank,
            count,
        }
    }

    #[test]
    fn a_synchronous_fill_returns_the_window_in_rank_order() {
        let bridge = Bridge::new().unwrap();
        let source = Source::fake();
        let filled = bridge.fill_now(&source, Request::new(fill(StoryType::Top, 0, 11), None));
        assert_eq!(filled.ids.len(), 30);
        assert_eq!(filled.items.len(), 11);
        assert_eq!(filled.items[0].as_ref().unwrap().id, 1);
        assert_eq!(filled.items[10].as_ref().unwrap().id, 11);
        assert!(filled.error.is_none());
    }

    #[test]
    fn a_request_with_a_snapshot_pages_against_it_and_never_re_ranks() {
        let bridge = Bridge::new().unwrap();
        let source = Source::fake();
        let first = bridge.fill_now(&source, Request::new(fill(StoryType::Top, 0, 11), None));
        let snapshot = first.ids.clone();
        // The ranking changes upstream, as it does between two refreshes.
        if let Source::Fake(fake) = &source {
            fake.set_ranking(StoryType::Top, vec![9, 8, 7]);
        }
        // A page turn carries the snapshot it was shown with, so the ranks do not move.
        let next = bridge.fill_now(
            &source,
            Request::new(fill(StoryType::Top, 11, 11), Some(snapshot.clone())),
        );
        assert_eq!(next.ids, snapshot, "the ranking did not change under the reader");
        assert_eq!(next.items[0].as_ref().unwrap().id, 12);
        // A refresh is the one request that takes a new one.
        let refreshed = bridge.fill_now(&source, Request::new(fill(StoryType::Top, 0, 11), None));
        assert_eq!(refreshed.ids, vec![9, 8, 7]);
    }

    #[test]
    fn a_ranking_failure_is_reported_not_swallowed() {
        let bridge = Bridge::new().unwrap();
        let source = Source::fake();
        if let Source::Fake(fake) = &source {
            fake.fail_ranking(StoryType::Top);
        }
        let filled = bridge.fill_now(&source, Request::new(fill(StoryType::Top, 0, 11), None));
        assert!(filled.items.is_empty());
        assert_eq!(
            filled.error.as_deref(),
            Some("the ranking request failed"),
            "a failed ranking is an error, not an empty feed"
        );
        // Another tab is unaffected.
        let other = bridge.fill_now(&source, Request::new(fill(StoryType::New, 0, 11), None));
        assert!(other.error.is_none());
        assert_eq!(other.items.len(), 11);
    }

    #[test]
    fn a_failed_item_keeps_its_slot() {
        let bridge = Bridge::new().unwrap();
        let source = Source::fake();
        if let Source::Fake(fake) = &source {
            fake.fail(3);
        }
        let filled = bridge.fill_now(&source, Request::new(fill(StoryType::Top, 0, 5), None));
        assert_eq!(filled.items.len(), 5);
        assert!(filled.items[2].is_none());
    }

    #[test]
    fn a_newer_request_supersedes_an_older_one() {
        let bridge = Bridge::new().unwrap();
        // The older request is genuinely slower, so if `Latest` did not supersede it, its
        // answer would land last and win.
        let slow = Arc::new(Source::Fake(
            crate::service::FakeService::fixture().with_delay(Duration::from_millis(150)),
        ));
        let fast = Arc::new(Source::fake());
        bridge.request(slow, Request::new(fill(StoryType::Top, 0, 11), None));
        bridge.request(fast, Request::new(fill(StoryType::New, 0, 11), None));
        let landed = wait_for(&bridge);
        assert_eq!(landed.tab, StoryType::New);
        assert_eq!(landed.items[0].as_ref().unwrap().id, 101);
        // The slow answer is discarded when it arrives, not taken as the newest.
        std::thread::sleep(Duration::from_millis(250));
        assert!(bridge.take().is_none(), "the superseded answer never lands");
    }

    #[test]
    fn a_cancelled_request_never_lands() {
        let bridge = Bridge::new().unwrap();
        let source = Arc::new(Source::fake());
        bridge.request(source, Request::new(fill(StoryType::Top, 0, 11), None));
        bridge.cancel();
        std::thread::sleep(Duration::from_millis(120));
        assert!(bridge.take().is_none());
        assert!(!bridge.is_pending());
    }

    /// Poll for a landed answer, the way `App::poll` would.
    fn wait_for(bridge: &Bridge) -> Filled {
        for _ in 0..600 {
            if let Some(filled) = bridge.take() {
                return filled;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("no answer landed");
    }

    fn story_request(index: usize, id: i32) -> StoryRequest {
        StoryRequest {
            story: crate::fixtures::item(index, id),
            config: Config::default(),
        }
    }

    /// Poll for a landed story open, the way `App::poll` would.
    fn wait_for_story(bridge: &Bridge) -> StoryFilled {
        for _ in 0..600 {
            if let Some(filled) = bridge.take_story() {
                return filled;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("no story landed");
    }

    #[test]
    fn a_story_open_fills_the_article_the_discussion_and_the_summary() {
        let bridge = Bridge::new().unwrap();
        let source = Source::fake();
        let filled = bridge.story_now(
            &source,
            story_request(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID),
        );
        assert_eq!(filled.story_id, crate::fixtures::STORY_ID);
        assert_eq!(
            filled.article.as_ref().map(|a| a.text.as_str()),
            Ok(crate::fixtures::ARTICLE),
            "the article comes from the reader's own extractor"
        );
        assert!(!filled.article_partial);
        assert_eq!(filled.comments.len(), 5, "the top-level discussion");
        assert_eq!(
            filled.sources.len(),
            5,
            "one captured source per comment, which is what the card counts"
        );
        let summary = filled.summary.expect("the fake provider always answers");
        assert_eq!(summary.points.len(), 3);
    }

    #[test]
    fn a_story_answer_lands_once() {
        let bridge = Bridge::new().unwrap();
        let source = Arc::new(Source::fake());
        bridge.request_story(
            source,
            story_request(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID),
        );
        let landed = wait_for_story(&bridge);
        assert_eq!(landed.story_id, crate::fixtures::STORY_ID);
        assert!(bridge.take_story().is_none(), "an answer lands once");
    }

    #[test]
    fn a_cancelled_story_open_never_lands() {
        let bridge = Bridge::new().unwrap();
        let source = Arc::new(Source::fake());
        bridge.request_story(
            source,
            story_request(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID),
        );
        bridge.cancel_story();
        std::thread::sleep(Duration::from_millis(120));
        assert!(bridge.take_story().is_none());
        assert!(!bridge.is_story_pending());
    }

    #[test]
    fn a_second_story_open_supersedes_the_first() {
        let bridge = Bridge::new().unwrap();
        let source = Arc::new(Source::fake());
        // Two opens in a row: only the newest lands, so `Esc` then `Enter` on another row
        // never paints the story you left.
        bridge.request_story(source.clone(), story_request(0, 1));
        bridge.request_story(source, story_request(crate::fixtures::STORY_INDEX, 2));
        let landed = wait_for_story(&bridge);
        assert_eq!(landed.story_id, 2);
        assert!(bridge.take_story().is_none());
    }

    #[test]
    fn a_story_open_leaves_the_feed_request_alone() {
        let bridge = Bridge::new().unwrap();
        let source = Arc::new(Source::fake());
        bridge.request(source.clone(), Request::new(fill(StoryType::Top, 0, 11), None));
        bridge.request_story(
            source,
            story_request(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID),
        );
        // Each kind has its own `Latest`, so opening a story never cancels the feed's page.
        assert_eq!(wait_for(&bridge).items.len(), 11);
        assert_eq!(wait_for_story(&bridge).story_id, crate::fixtures::STORY_ID);
    }
}
