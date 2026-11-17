//! The async bridge: latest-wins requests driven off the UI thread.
//!
//! The desktop owns a tokio runtime. `AppSpec::setup` spawns the first fill; the
//! [`Latest`] worker closure drives the request to completion on that runtime, and `poll`
//! takes the answer. Story phases use Tokio tasks; replacing a handle detaches its work.
//!
//! Independent owners keep feed, article, discussion/summary and replies separate:
//!
//! - [`Request`] — one aggregate fill of the feed's visible window.
//! - [`StoryRequest`] — article and discussion start independently; the discussion capture
//!   starts the summary when it lands.
//!
//! Headless runs and tests use [`Bridge::fill_now`] and [`Bridge::story_now`], which run the
//! same requests on the same runtime but block, so a scene is deterministic and needs no frame
//! budget.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker as TaskWaker};

use anyhow::Result;
use hn_lib::app::Comment;
use hn_lib::reader::{Article, Config, Discussion, Source as CapturedSource};
use hn_lib::HNCLIItem;
use rune::{Latest, Waker};
use tokio::runtime::{Handle, Runtime};
use tokio::task::JoinHandle;

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
    article: Option<JoinHandle<ArticlePhase>>,
    story: Option<JoinHandle<StoryPhase>>,
    parked: HashMap<i32, (Option<JoinHandle<ArticlePhase>>, Option<JoinHandle<StoryPhase>>)>,
    refresh: Latest<CommentsRefreshed>,
    waker: Option<TaskWaker>,
    replies: Latest<ReplyFilled>,
}

struct StoryWaker(Waker);

impl Wake for StoryWaker {
    fn wake(self: Arc<Self>) { (self.0)(); }
    fn wake_by_ref(self: &Arc<Self>) { (self.0)(); }
}

fn take_task<T>(task: &mut Option<JoinHandle<T>>, waker: Option<&TaskWaker>) -> Option<T> {
    let mut context = Context::from_waker(waker.unwrap_or(TaskWaker::noop()));
    match std::pin::Pin::new(task.as_mut()?).poll(&mut context) {
        Poll::Pending => None,
        Poll::Ready(result) => { *task = None; result.ok() }
    }
}

/// One story open: the item to read, and the summary settings to read it with.
///
/// The `Config` is the reader's own, loaded once at launch; it is never logged, printed or
/// written into a tree.
#[derive(Clone)]
pub struct StoryRequest {
    pub story: HNCLIItem,
    pub config: Config,
}

/// The aggregate answer used for synchronous, deterministic story opens.
///
/// A failed article is a `String`; the card uses [`SummaryError`] to classify a failed summary.
pub struct StoryFilled {
    pub story_id: i32,
    pub article: Result<(String, bool), String>,
    pub article_summary: Result<Article, SummaryError>,
    /// The discussion as a tree: the top-level comments and their replies, level by level.
    pub comments: Result<Vec<Comment>, String>,
    pub sources: Vec<CapturedSource>,
    pub summary: Result<Discussion, SummaryError>,
}

/// A result that can be shown without waiting for the other story work.
pub enum StoryUpdate {
    Article { story_id: i32, article: Result<(String, bool), String>, summary_skipped: bool },
    Discussion { story_id: i32, comments: Result<Vec<Comment>, String>, sources: Vec<CapturedSource>, summary_skipped: bool },
    ArticleSummary { story_id: i32, summary: Result<Article, SummaryError> },
    Summary { story_id: i32, summary: Result<Discussion, SummaryError>, sources: Vec<CapturedSource> },
}

enum ArticlePhase {
    Captured { source: Arc<Source>, request: StoryRequest, article: Result<(String, bool), String>, manual: bool },
    Summary { story_id: i32, summary: Result<Article, SummaryError> },
}

enum StoryPhase {
    Captured {
        source: Arc<Source>,
        request: StoryRequest,
        comments: Result<Vec<Comment>, String>,
        sources: Vec<CapturedSource>,
        manual: bool,
    },
    Summary { story_id: i32, summary: Result<Discussion, SummaryError>, sources: Vec<CapturedSource> },
}

pub struct CommentsRefreshed {
    pub story_id: i32,
    pub comments: Result<Vec<Comment>, String>,
    pub descendants: Vec<Comment>,
    pub warning: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplyRequest {
    pub story_id: i32,
    pub parent_id: i32,
    pub ids: Vec<i32>,
    pub depth: usize,
}

pub struct ReplyFilled {
    pub request: ReplyRequest,
    pub comments: Result<Vec<Comment>, String>,
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
            article: None,
            story: None,
            parked: HashMap::new(),
            refresh: Latest::default(),
            waker: None,
            replies: Latest::default(),
        })
    }

    /// Attach the waker `AppSpec::setup` hands over, so a landed answer runs `poll`.
    pub fn set_waker(&mut self, waker: Waker) {
        self.latest.set_waker(waker.clone());
        self.replies.set_waker(waker.clone());
        self.refresh.set_waker(waker.clone());
        let task_waker = TaskWaker::from(Arc::new(StoryWaker(waker)));
        self.waker = Some(task_waker.clone());
        if self.article.is_some() || self.story.is_some() {
            task_waker.wake_by_ref();
        }
    }

    /// Ask for `request`, superseding any earlier one. The work runs on the runtime from a
    /// `Latest` worker thread; only the newest request's answer lands.
    pub fn request(&self, source: Arc<Source>, request: Request) {
        let handle = self.handle.clone();
        self.latest.request(request, move |request| {
            handle.block_on(run(source.as_ref(), request))
        });
    }

    /// Ask for one story open, superseding both parts of any earlier open or retry.
    pub fn request_story(&mut self, source: Arc<Source>, request: StoryRequest) {
        self.replies.cancel();
        self.refresh.cancel();
        let article_source = source.clone();
        let article_request = request.clone();
        self.article = Some(self.handle.spawn(async move {
            let article = article_source.article(&article_request.story).await;
            ArticlePhase::Captured { source: article_source, request: article_request, article, manual: false }
        }));
        self.story = Some(self.handle.spawn(async move {
            let comments = source.thread(request.story.id).await.map_err(|e| e.to_string());
            let sources = comments.as_ref().map(|comments| source.capture(comments)).unwrap_or_default();
            StoryPhase::Captured { source, request, comments, sources, manual: false }
        }));
        // Input dispatch may run after the frame's poll; schedule one to register these tasks.
        if let Some(waker) = &self.waker { waker.wake_by_ref(); }
    }

    /// Re-run summaries on the current captures without replacing the discussion tree.
    pub fn request_summaries(&mut self, source: Arc<Source>, request: StoryRequest,
        article: Option<(String, bool)>, sources: Option<Vec<CapturedSource>>) {
        let article_source = source.clone();
        let article_request = request.clone();
        self.article = Some(self.handle.spawn(async move {
            match article {
                Some(article) => ArticlePhase::Summary {
                    story_id: article_request.story.id,
                    summary: article_source.article_summary(&article_request.config, &article_request.story, &article).await,
                },
                None => {
                    let article = article_source.article(&article_request.story).await;
                    ArticlePhase::Captured { source: article_source, request: article_request, article, manual: true }
                }
            }
        }));
        self.story = Some(self.handle.spawn(async move {
            match sources {
                Some(sources) => {
                    let summary = source.summary(&request.config, &request.story, &sources).await;
                    StoryPhase::Summary { story_id: request.story.id, summary, sources }
                }
                None => {
                    let comments = source.thread(request.story.id).await.map_err(|e| e.to_string());
                    let sources = comments.as_ref().map(|comments| source.capture(comments)).unwrap_or_default();
                    StoryPhase::Captured { source, request, comments, sources, manual: true }
                }
            }
        }));
        if let Some(waker) = &self.waker { waker.wake_by_ref(); }
    }

    pub fn request_refresh(&self, source: Arc<Source>, story_id: i32, loaded: Vec<i32>) {
        let handle = self.handle.clone();
        self.refresh.request((story_id, loaded), move |(story_id, loaded)| {
            handle.block_on(run_refresh(source.as_ref(), story_id, loaded))
        });
    }

    pub fn refresh_now(&self, source: &Source, story_id: i32, loaded: Vec<i32>) -> CommentsRefreshed {
        self.runtime.block_on(run_refresh(source, story_id, loaded))
    }

    pub fn take_refresh(&self) -> Option<CommentsRefreshed> { self.refresh.take() }

    /// The same request, run to completion on this thread. Headless scenes and tests use it so
    /// the first frame already has its rows.
    pub fn fill_now(&self, source: &Source, request: Request) -> Filled {
        self.runtime.block_on(run(source, request))
    }

    /// The story open, run to completion on this thread.
    pub fn story_now(&mut self, source: &Source, request: StoryRequest) -> StoryFilled {
        self.cancel_story();
        self.runtime.block_on(run_story(source, request))
    }

    pub fn request_replies(&self, source: Arc<Source>, request: ReplyRequest) {
        let handle = self.handle.clone();
        self.replies.request(request, move |request| handle.block_on(run_replies(source.as_ref(), request)));
    }

    pub fn replies_now(&self, source: &Source, request: ReplyRequest) -> ReplyFilled {
        self.runtime.block_on(run_replies(source, request))
    }

    pub fn take_replies(&self) -> Option<ReplyFilled> { self.replies.take() }

    /// The newest answer, once.
    pub fn take(&self) -> Option<Filled> {
        self.latest.take()
    }

    /// Take a ready story result. Start deferred summaries with settings current at dispatch;
    /// article extraction never holds up the discussion.
    pub fn take_story(&mut self, config: &Config, summarize_article: bool, summarize_discussion: bool) -> Option<StoryUpdate> {
        if let Some(phase) = take_task(&mut self.article, self.waker.as_ref()) {
            return Some(match phase {
                ArticlePhase::Captured { source, request, article, manual } => {
                    let story_id = request.story.id;
                    if let Ok(captured) = &article && (manual || config.automatic) && summarize_article && (!source.is_live() || !config.key().is_empty()) {
                        let captured = captured.clone();
                        let config = config.clone();
                        self.article = Some(self.handle.spawn(async move {
                            let summary = source.article_summary(&config, &request.story, &captured).await;
                            ArticlePhase::Summary { story_id, summary }
                        }));
                    }
                    StoryUpdate::Article { story_id, article, summary_skipped: !manual && !config.automatic }
                }
                ArticlePhase::Summary { story_id, summary } => StoryUpdate::ArticleSummary { story_id, summary },
            });
        }
        match take_task(&mut self.story, self.waker.as_ref())? {
            StoryPhase::Captured { source, request, comments, sources, manual } => {
                let story_id = request.story.id;
                if comments.is_ok() && (manual || config.automatic) && summarize_discussion && (!source.is_live() || !config.key().is_empty()) {
                    let captured = sources.clone();
                    let config = config.clone();
                    self.story = Some(self.handle.spawn(async move {
                        let summary = source.summary(&config, &request.story, &captured).await;
                        StoryPhase::Summary { story_id, summary, sources: captured }
                    }));
                }
                Some(StoryUpdate::Discussion { story_id, comments, sources, summary_skipped: !manual && !config.automatic })
            }
            StoryPhase::Summary { story_id, summary, sources } => Some(StoryUpdate::Summary { story_id, summary, sources }),
        }
    }

    /// Whether a request is still in flight.
    pub fn is_pending(&self) -> bool {
        self.latest.is_pending()
    }

    /// Whether a story open is still in flight.
    pub fn is_story_pending(&self) -> bool {
        self.article.is_some() || self.story.is_some()
    }

    /// Retain paid work and its captures while the reader is on another story.
    pub(crate) fn park_story(&mut self, story_id: i32) {
        self.replies.cancel();
        self.refresh.cancel();
        self.parked.insert(story_id, (self.article.take(), self.story.take()));
    }

    pub(crate) fn resume_story(&mut self, story_id: i32) {
        if let Some((article, story)) = self.parked.remove(&story_id) {
            self.article = article;
            self.story = story;
            if let Some(waker) = &self.waker { waker.wake_by_ref(); }
        }
    }

    /// Drop the story open and any answer not yet taken, leaving the feed's request alone.
    pub fn cancel_story(&mut self) {
        self.article = None;
        self.story = None;
        self.replies.cancel();
        self.refresh.cancel();
    }

    /// Drop only the feed request when a held window settles synchronously.
    pub fn cancel_feed(&self) {
        self.latest.cancel();
    }

    /// Drop every request and any answer not yet taken.
    pub fn cancel(&mut self) {
        self.latest.cancel();
        self.cancel_story();
        self.parked.clear();
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
        .await
        .into_iter()
        .zip(ids.iter().skip(request.fill.from_rank))
        .map(|(item, &id)| item.ok_or(id))
        .collect();
    Filled {
        tab: request.fill.tab,
        ids,
        from_rank: request.fill.from_rank,
        items,
        error: None,
    }
}

/// The synchronous story-open shape, for deterministic headless scenes.
///
/// Summary capture requires the discussion, but neither it nor the discussion depends on the
/// article extractor. A failed article does not stop the discussion.
///
/// The discussion is fetched as a tree, because the inspector draws one; the summary is written
/// from the top-level comments only; the capture stays paired with its summary citations.
async fn run_story(source: &Source, request: StoryRequest) -> StoryFilled {
    let story_id = request.story.id;
    let ((article, article_summary), (comments, sources, summary)) = tokio::join!(async {
        let article = source.article(&request.story).await;
        let summary = match &article {
            Ok(_) if !request.config.automatic => Err(SummaryError::new("Automatic summaries are off", false)),
            Ok(captured) => source.article_summary(&request.config, &request.story, captured).await,
            Err(message) => Err(SummaryError::new(message.clone(), false)),
        };
        (article, summary)
    }, async {
        let comments = source.thread(story_id).await.map_err(|e| e.to_string());
        let sources = comments.as_ref().map(|comments| source.capture(comments)).unwrap_or_default();
        let summary = match &comments {
            Ok(_) if !request.config.automatic => Err(SummaryError::new("Automatic summaries are off", false)),
            Ok(_) => source.summary(&request.config, &request.story, &sources).await,
            Err(message) => Err(SummaryError::new(format!("Discussion unavailable: {message}"), false)),
        };
        (comments, sources, summary)
    });
    StoryFilled {
        story_id,
        article,
        article_summary,
        comments,
        sources,
        summary,
    }
}

async fn run_replies(source: &Source, request: ReplyRequest) -> ReplyFilled {
    let comments = source.children(&request.ids, request.depth).await.map_err(|e| e.to_string());
    ReplyFilled { request, comments }
}

/// Refresh only roots and metadata for replies the reader has already loaded.
async fn run_refresh(source: &Source, story_id: i32, loaded: Vec<i32>) -> CommentsRefreshed {
    let descendants = async {
        if loaded.is_empty() { return (Vec::new(), None); }
        match source.children(&loaded, 1).await {
            Ok(comments) => {
                let found: std::collections::HashSet<_> = comments.iter().map(|comment| comment.id).collect();
                let missing = loaded.iter().filter(|id| !found.contains(id)).count();
                let warning = (missing > 0).then(|| {
                    let noun = if missing == 1 { "reply" } else { "replies" };
                    format!("{missing} loaded {noun} could not be refreshed")
                });
                (comments, warning)
            }
            Err(error) => (Vec::new(), Some(format!("Loaded replies could not be refreshed: {error}"))),
        }
    };
    let (comments, (descendants, warning)) = tokio::join!(source.thread(story_id), descendants);
    CommentsRefreshed { story_id, comments: comments.map_err(|error| error.to_string()), descendants, warning }
}

#[cfg(test)]
#[path = "../../tests/support/http_fixture.rs"]
mod http_fixture;

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
            fake.fail(14);
        }
        let filled = bridge.fill_now(&source, Request::new(fill(StoryType::Top, 11, 5), None));
        assert_eq!(filled.items.len(), 5);
        assert_eq!(filled.items[2].as_ref().err(), Some(&14));
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
        let mut bridge = Bridge::new().unwrap();
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

    /// Poll for the three story surfaces, the way `App::poll` would.
    fn wait_for_story(bridge: &mut Bridge) -> Vec<StoryUpdate> {
        wait_for_story_with_config(bridge, &Config::default())
    }

    fn wait_for_story_with_config(bridge: &mut Bridge, config: &Config) -> Vec<StoryUpdate> {
        let mut updates = Vec::new();
        for _ in 0..600 {
            while let Some(update) = bridge.take_story(config, true, true) {
                updates.push(update);
            }
            if updates.len() == 4 {
                return updates;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("not all story updates landed");
    }

    fn update_id(update: &StoryUpdate) -> i32 {
        match update {
            StoryUpdate::Article { story_id, .. }
            | StoryUpdate::ArticleSummary { story_id, .. }
            | StoryUpdate::Discussion { story_id, .. }
            | StoryUpdate::Summary { story_id, .. } => *story_id,
        }
    }

    #[test]
    fn parked_captures_send_current_provider_credentials_and_profile() {
        use hn_lib::reader::{Network, Profile, Provider};
        use hn_lib::{HackerNewsCliServiceImpl, hn_client::HackerNewsClientImpl};
        use serde_json::json;
        let mut bridge = Bridge::new().unwrap();
        let (sent, received) = std::sync::mpsc::channel();
        let (base, _requests, server) = bridge.runtime.block_on(http_fixture::serve(move |path, body, headers| {
            sent.send((path.clone(), body, headers)).unwrap();
            let content = json!({"summary":"article", "overview":"discussion", "points":[{"text":"point", "sources":[901]}]}).to_string();
            let response = if path == "/messages" {
                json!({"content":[{"type":"text", "text":content}]})
            } else {
                json!({"choices":[{"message":{"content":content}}]})
            };
            std::future::ready((200, response.to_string()))
        }));
        let mut network = Network::default();
        network.endpoint = format!("{base}chat/completions");
        network.anthropic_endpoint = format!("{base}messages");
        network.response_log_dir = None;
        let source = Arc::new(Source::Live { client: HackerNewsClientImpl::new(),
            service: HackerNewsCliServiceImpl::new(), network });
        for provider in [Provider::OpenRouter, Provider::Anthropic] {
            let request = StoryRequest { story: crate::fixtures::item(0, 1), config: Config {
                api_key: "old-key".into(), ..Config::default()
            }};
            let current = Config { provider, api_key: "new-key".into(), anthropic_api_key: "new-key".into(),
                profile: Profile::Thorough, ..Config::default() };
            let comments = crate::fixtures::comments();
            let sources = source.capture(&comments);
            let article_source = source.clone();
            let article_request = request.clone();
            bridge.article = Some(bridge.handle.spawn(async move { ArticlePhase::Captured {
                source: article_source, request: article_request, article: Ok(("captured article".into(), false)), manual: false,
            }}));
            let discussion_source = source.clone();
            bridge.story = Some(bridge.handle.spawn(async move { StoryPhase::Captured {
                source: discussion_source, request, comments: Ok(comments), sources, manual: false,
            }}));
            bridge.park_story(1);
            bridge.resume_story(1);
            let updates = wait_for_story_with_config(&mut bridge, &current);
            assert!(updates.iter().any(|u| matches!(u, StoryUpdate::ArticleSummary { summary: Ok(_), .. })));
            assert!(updates.iter().any(|u| matches!(u, StoryUpdate::Summary { summary: Ok(_), .. })));
            for _ in 0..2 {
                let (path, body, headers) = received.recv_timeout(Duration::from_secs(2)).unwrap();
                assert_eq!(path, if provider == Provider::Anthropic { "/messages" } else { "/chat/completions" });
                assert!(headers.to_lowercase().contains("authorization: bearer new-key\r\n"));
                assert_eq!(body["model"], current.profile.model(current.provider));
            }
            assert!(received.try_recv().is_err());
        }
        server.abort();
    }

    #[test]
    fn story_tasks_wake_to_register_and_again_when_ready() {
        let mut bridge = Bridge::new().unwrap();
        let (wake, wakes) = std::sync::mpsc::channel();
        bridge.set_waker(Arc::new(move || { let _ = wake.send(()); }));
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        let article_gate = gate.clone();
        let mut fake = crate::service::FakeService::fixture().with_delay(Duration::from_secs(5));
        fake.article = Arc::new(move |_| {
            let gate = article_gate.clone();
            Box::pin(async move {
                gate.acquire().await.unwrap().forget();
                Ok(("ready".to_string(), false))
            })
        });
        bridge.request_story(Arc::new(Source::Fake(fake)), story_request(0, 1));
        wakes.recv_timeout(Duration::from_secs(2)).expect("request schedules registration poll");
        assert!(bridge.take_story(&Config::default(), true, true).is_none());
        gate.add_permits(1);
        wakes.recv_timeout(Duration::from_secs(2)).expect("completed task wakes the UI");
        assert!(matches!(bridge.take_story(&Config::default(), true, true), Some(StoryUpdate::Article { story_id: 1, .. })));
        bridge.cancel_story();
    }

    #[test]
    fn a_story_open_fills_the_article_the_discussion_and_the_summary() {
        let mut bridge = Bridge::new().unwrap();
        let source = Source::fake();
        let filled = bridge.story_now(
            &source,
            story_request(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID),
        );
        assert_eq!(filled.story_id, crate::fixtures::STORY_ID);
        assert_eq!(
            filled.article.as_ref().map(|(text, partial)| (text.as_str(), *partial)),
            Ok((crate::fixtures::ARTICLE, false)),
            "the article comes from the reader's own extractor"
        );
        assert_eq!(
            filled.comments.as_ref().unwrap().len(),
            5,
            "initial discussion contains every root, without fetching replies"
        );
        assert_eq!(
            filled
                .comments.as_ref().unwrap()
                .iter()
                .filter(|comment| comment.depth == 0)
                .count(),
            5,
            "five top-level threads"
        );
        assert_eq!(
            filled.sources.len(),
            5,
            "one captured source per top-level comment, which is what the card counts"
        );
        let summary = filled.summary.expect("the fake provider always answers");
        assert_eq!(summary.points.len(), 3);
    }

    #[test]
    fn each_story_surface_lands_once() {
        let mut bridge = Bridge::new().unwrap();
        let source = Arc::new(Source::fake());
        bridge.request_story(
            source,
            story_request(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID),
        );
        let landed = wait_for_story(&mut bridge);
        assert!(landed.iter().all(|update| update_id(update) == crate::fixtures::STORY_ID));
        assert!(bridge.take_story(&Config::default(), true, true).is_none(), "each answer lands once");
    }

    #[test]
    fn a_cancelled_story_open_never_lands() {
        let mut bridge = Bridge::new().unwrap();
        let source = Arc::new(Source::fake());
        bridge.request_story(
            source,
            story_request(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID),
        );
        bridge.cancel_story();
        std::thread::sleep(Duration::from_millis(120));
        assert!(bridge.take_story(&Config::default(), true, true).is_none());
        assert!(!bridge.is_story_pending());
    }

    #[test]
    fn reopening_the_same_story_discards_its_older_refresh() {
        let mut bridge = Bridge::new().unwrap();
        let slow = Arc::new(Source::Fake(crate::service::FakeService::fixture().with_delay(Duration::from_millis(150))));
        bridge.request_refresh(slow, 1, Vec::new());
        bridge.request_story(Arc::new(Source::fake()), story_request(0, 1));
        assert!(wait_for_story(&mut bridge).iter().all(|update| update_id(update) == 1));
        std::thread::sleep(Duration::from_millis(200));
        assert!(bridge.take_refresh().is_none());
    }

    #[test]
    fn a_second_story_open_supersedes_the_first() {
        let mut bridge = Bridge::new().unwrap();
        let source = Arc::new(Source::fake());
        // Two opens in a row: only the newest lands, so `Esc` then `Enter` on another row
        // never paints the story you left.
        bridge.request_story(source.clone(), story_request(0, 1));
        bridge.request_story(source, story_request(crate::fixtures::STORY_INDEX, 2));
        let landed = wait_for_story(&mut bridge);
        assert!(landed.iter().all(|update| update_id(update) == 2));
        assert!(bridge.take_story(&Config::default(), true, true).is_none());
    }

    #[test]
    fn a_story_open_leaves_the_feed_request_alone() {
        let mut bridge = Bridge::new().unwrap();
        let source = Arc::new(Source::fake());
        bridge.request(source.clone(), Request::new(fill(StoryType::Top, 0, 11), None));
        bridge.request_story(
            source,
            story_request(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID),
        );
        // Each kind has its own owner, so opening a story never cancels the feed's page.
        assert_eq!(wait_for(&bridge).items.len(), 11);
        assert!(wait_for_story(&mut bridge).iter().all(|update| update_id(update) == crate::fixtures::STORY_ID));
    }
}
