//! Story reader, optional AI summaries, and companion article pane.
mod browser;
#[cfg(test)]
mod capture_tests;
mod config;
mod interaction;
mod jobs;
mod network;
mod preparation;
#[cfg(test)]
mod replay;
#[cfg(test)]
mod sampling_tests;
mod settings;
pub(crate) mod snapshot;
pub(crate) mod sources;
mod summary_prompts;
#[cfg(test)]
mod tests;
mod text;
mod view;

use config::{Config, Profile};
use hn_lib::{theme::Theme, HNCLIItem};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{sync::mpsc, task::JoinSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Source {
    pub id: i32,
    pub parent: Option<i32>,
    pub author: String,
    pub text: String,
    pub excerpt: String,
}
#[derive(Debug, Deserialize)]
pub struct Point {
    pub text: String,
    pub sources: Vec<i32>,
}
#[derive(Debug, Deserialize)]
pub struct Discussion {
    pub overview: String,
    pub points: Vec<Point>,
}
#[derive(Debug)]
pub struct Article {
    text: String,
}
#[derive(Debug)]
struct ArticleCapture {
    title: String,
    text: String,
    partial: bool,
}
#[derive(Debug)]
struct DiscussionCapture {
    title: String,
    post: String,
    sources: Vec<Source>,
    coverage: String,
}
#[derive(Debug)]
struct Capture<T> {
    input: T,
    captured: String,
}
impl<T> Capture<T> {
    fn new(input: T) -> Self {
        Self {
            input,
            captured: chrono::Local::now().format("%H:%M").to_string(),
        }
    }
}
#[derive(Debug, Clone)]
pub struct Failure {
    pub(crate) message: String,
    auth: bool,
}
impl Failure {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            auth: false,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Attempt {
    id: u64,
    profile: Profile,
    automatic: bool,
    refresh: bool,
}
#[derive(Clone, Copy)]
enum Request {
    Automatic,
    Generate,
    Missing,
    Refresh,
    Retry,
}
#[derive(Debug, Default)]
enum Progress {
    #[default]
    Idle,
    Queued(Attempt),
    Preparing(Attempt, tokio::task::AbortHandle, Profile),
    Running(Attempt),
    Failed(Attempt, Failure),
}
#[derive(Debug)]
struct Completed<T, C> {
    value: T,
    profile: Profile,
    capture: Arc<Capture<C>>,
}
#[derive(Debug)]
struct Section<T, C> {
    result: Option<Completed<T, C>>,
    capture: Option<Arc<Capture<C>>>,
    progress: Progress,
}
impl<T, C> Default for Section<T, C> {
    fn default() -> Self {
        Self {
            result: None,
            capture: None,
            progress: Progress::Idle,
        }
    }
}
impl<T, C> Section<T, C> {
    fn requested(&self) -> bool {
        self.result.is_some() || !matches!(self.progress, Progress::Idle)
    }
    fn pending(&self) -> bool {
        matches!(
            self.progress,
            Progress::Queued(_) | Progress::Preparing(_, _, _) | Progress::Running(_)
        )
    }
    fn failed(&self) -> bool {
        matches!(self.progress, Progress::Failed(_, _))
    }
}
pub struct Reading {
    theme: Theme,
    story: HNCLIItem,
    article: Section<Article, ArticleCapture>,
    discussion: Section<Discussion, DiscussionCapture>,
    staged: Option<Completed<Discussion, DiscussionCapture>>,
    shown: bool,
    moved: bool,
    // Track the panel height until movement freezes it; explicit opens resume tracking.
    growth_cap: usize,
    // Retain the settled target for the next opening; elapsed time determines activity.
    panel_motion: Option<(Instant, u16, u16)>,
    prefix_height: usize,
    summary_scroll: usize,
    page_rows: usize,
    citation: usize,
    reveal_citation: bool,
    citation_focus: bool,
    source: Option<i32>,
    source_scroll: usize,
}
impl Reading {
    fn new(story: HNCLIItem) -> Self {
        Self {
            theme: Theme::default(),
            story,
            article: Section::default(),
            discussion: Section::default(),
            staged: None,
            shown: true,
            moved: false,
            growth_cap: 0,
            panel_motion: None,
            prefix_height: 0,
            summary_scroll: 0,
            page_rows: 1,
            citation: 0,
            reveal_citation: false,
            citation_focus: false,
            source: None,
            source_scroll: 0,
        }
    }
    fn leave_source(&mut self) {
        self.reveal_citation = self.source.take().is_some() && self.citation_focus;
        if let Some(result) = self.staged.take() {
            self.discussion.result = Some(result);
            self.citation = 0;
            self.citation_focus = false;
            self.reveal_citation = false;
        }
    }
    fn citations(&self) -> Vec<i32> {
        let mut ids = vec![];
        if let Some(result) = &self.discussion.result {
            for id in result.value.points.iter().flat_map(|p| &p.sources) {
                if !ids.contains(id) && result.capture.input.sources.iter().any(|s| s.id == *id) {
                    ids.push(*id);
                }
            }
        }
        ids
    }
}
pub struct Reader {
    snapshot: Option<Arc<hn_lib::snapshot::Snapshot>>,
    theme: Theme,
    readings: HashMap<i32, Reading>,
    active: Option<i32>,
    config: Config,
    config_path: Option<PathBuf>,
    settings: Option<settings::Settings>,
    settings_error: String,
    auth_failed: bool,
    network: network::Network,
    jobs: JoinSet<jobs::Completion>,
    next_attempt: u64,
    browser: Option<browser::Target>,
    browser_busy: bool,
    desired_browser: Option<(HNCLIItem, bool)>,
    browser_notice: String,
    // Opening/success notices are timed; errors have no clock and use the error role.
    notice_started: Option<Instant>,
    tx: mpsc::UnboundedSender<Result<(Option<browser::Target>, String), String>>,
    rx: mpsc::UnboundedReceiver<Result<(Option<browser::Target>, String), String>>,
}
impl Default for Reader {
    fn default() -> Self {
        Self::new(Theme::default())
    }
}
impl Reader {
    pub fn new(theme: Theme) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let path = Config::path();
        let loaded = path
            .as_ref()
            .map_err(|e| e.to_string())
            .and_then(|p| Config::load(p).map_err(|e| e.to_string()));
        let (config, settings_error) = match loaded {
            Ok(c) => (c, String::new()),
            Err(e) => (Config::default(), e),
        };
        Self {
            snapshot: None,
            theme,
            readings: HashMap::new(),
            active: None,
            config,
            config_path: path.ok(),
            settings: None,
            settings_error,
            auth_failed: false,
            network: network::Network::default(),
            jobs: JoinSet::new(),
            next_attempt: 0,
            browser: None,
            browser_busy: false,
            desired_browser: None,
            browser_notice: String::new(),
            notice_started: None,
            tx,
            rx,
        }
    }
}
