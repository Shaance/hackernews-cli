//! Story reader, optional AI summaries, and companion article pane.
//!
//! # Public boundary
//!
//! A non-terminal front end drives summaries through the items below and never touches the
//! terminal front end ([`crate::tui`], [`crate::reader_actions`]):
//!
//! - Configuration: [`Config`], [`load_config`], [`save_config`], [`Provider`],
//!   [`SummaryProfile`].
//! - Request inputs: [`ArticleCapture`], [`DiscussionCapture`].
//! - Provider calls: [`Network::article`], [`Network::discussion`].
//! - Results: [`Article`], [`Discussion`], [`Point`], [`Source`]; failures are [`Failure`].
//! - Source material: [`sources::article`], [`sources::clean`].
//! - Snapshot summaries: [`snapshot::prepare`].
//!
//! [`Reader`] is the terminal reader pane. It is public so the TUI can own it, but it is not
//! the interface a desktop front end binds to.
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
pub mod saved;
mod settings;
pub mod snapshot;
mod source_view;
pub mod sources;
mod summary_prompts;
#[cfg(test)]
mod tests;
mod text;
mod view;

use crate::{theme::Theme, HNCLIItem};
pub use config::{Config, Profile, Profile as SummaryProfile, Provider};
pub use network::Network;
use saved::{DiscussionContext, SummaryStore};
use serde::{Deserialize, Serialize};
use source_view::SourceView;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{sync::mpsc, task::JoinSet};

/// Load summary settings from `path`, falling back to defaults when the file is absent.
pub fn load_config(path: &std::path::Path) -> anyhow::Result<Config> {
    Config::load(path)
}
/// Persist summary settings to `path`.
pub fn save_config(config: &Config, path: &std::path::Path) -> anyhow::Result<()> {
    config.save(path)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Source {
    pub id: i32,
    pub parent: Option<i32>,
    pub author: String,
    pub text: String,
    pub excerpt: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Point {
    pub text: String,
    pub sources: Vec<i32>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Discussion {
    pub overview: String,
    pub points: Vec<Point>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Article {
    pub text: String,
}
#[derive(Debug)]
pub struct ArticleCapture {
    pub title: String,
    pub text: String,
    pub partial: bool,
}
#[derive(Debug)]
pub struct DiscussionCapture {
    pub title: String,
    pub post: String,
    pub sources: Vec<Source>,
    pub coverage: String,
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
    pub message: String,
    auth: bool,
}
impl Failure {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            auth: false,
        }
    }
    /// True when the failure was an auth error (401/403) — desktop callers use
    /// this to decide whether to re-prompt for an API key.
    pub fn is_auth(&self) -> bool {
        self.auth
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
struct Completed<T, C, S> {
    value: T,
    profile: Profile,
    basis: Basis<C, S>,
}
#[derive(Debug)]
enum Basis<C, S> {
    Captured(Arc<Capture<C>>),
    Saved { saved_at: u64, context: S },
}
impl<T, C, S> Completed<T, C, S> {
    fn capture(&self) -> Option<&Arc<Capture<C>>> {
        match &self.basis {
            Basis::Captured(capture) => Some(capture),
            Basis::Saved { .. } => None,
        }
    }
}
impl Completed<Discussion, DiscussionCapture, DiscussionContext> {
    fn sources(&self) -> &[Source] {
        match &self.basis {
            Basis::Captured(capture) => &capture.input.sources,
            Basis::Saved { context, .. } => &context.sources,
        }
    }
}
#[derive(Debug)]
struct Section<T, C, S> {
    result: Option<Completed<T, C, S>>,
    capture: Option<Arc<Capture<C>>>,
    progress: Progress,
}
impl<T, C, S> Default for Section<T, C, S> {
    fn default() -> Self {
        Self {
            result: None,
            capture: None,
            progress: Progress::Idle,
        }
    }
}
impl<T, C, S> Section<T, C, S> {
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
    article: Section<Article, ArticleCapture, bool>,
    discussion: Section<Discussion, DiscussionCapture, DiscussionContext>,
    staged: Option<(
        Completed<Discussion, DiscussionCapture, DiscussionContext>,
        Provider,
        u32,
    )>,
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
    source: Option<SourceView>,
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
        }
    }
    fn restore(&mut self, store: &SummaryStore) {
        if let Some(record) = store.load_article(self.story.id) {
            self.article.result = Some(Completed {
                value: record.data.article,
                profile: record.header.profile,
                basis: Basis::Saved {
                    saved_at: record.header.saved_at,
                    context: record.data.partial,
                },
            });
        }
        if let Some(record) = store.load_discussion(self.story.id) {
            self.discussion.result = Some(Completed {
                value: record.data.discussion,
                profile: record.header.profile,
                basis: Basis::Saved {
                    saved_at: record.header.saved_at,
                    context: record.data.context,
                },
            });
        }
    }
    fn leave_source(&mut self, store: Option<&mut SummaryStore>) -> std::io::Result<()> {
        self.reveal_citation = self.source.take().is_some() && self.citation_focus;
        if let Some((result, provider, comments)) = self.staged.take() {
            self.discussion.result = Some(result);
            self.citation = 0;
            self.citation_focus = false;
            self.reveal_citation = false;
            if let Some(store) = store {
                return jobs::save_discussion(
                    store,
                    self.story.id,
                    self.discussion.result.as_ref().unwrap(),
                    provider,
                    comments,
                );
            }
        }
        Ok(())
    }
    fn citations(&self) -> Vec<i32> {
        let mut ids = vec![];
        if let Some(result) = &self.discussion.result {
            for id in result.value.points.iter().flat_map(|p| &p.sources) {
                if !ids.contains(id) && result.sources().iter().any(|s| s.id == *id) {
                    ids.push(*id);
                }
            }
        }
        ids
    }
}
pub struct Reader {
    snapshot: Option<Arc<crate::snapshot::Snapshot>>,
    theme: Theme,
    readings: HashMap<i32, Reading>,
    active: Option<i32>,
    config: Config,
    config_path: Option<PathBuf>,
    settings: Option<settings::Settings>,
    settings_error: String,
    auth_failed: bool,
    network: network::Network,
    store: Option<SummaryStore>,
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
        Self::new(Theme::default(), None)
    }
}
impl Reader {
    pub fn new(theme: Theme, summary_root: Option<PathBuf>) -> Self {
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
        let store = summary_root.map(SummaryStore::disk);
        if let Some(store) = &store {
            store.prune();
        }
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
            store,
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
