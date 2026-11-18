//! Story requests and results exchanged by the desktop service and bridge.

use hn_lib::app::Comment;
use hn_lib::reader::{Article, Config, Discussion, Source as CapturedSource};
use hn_lib::HNCLIItem;

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

