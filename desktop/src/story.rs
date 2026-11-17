//! The story split's state: the article, the summary card's machine, and the discussion tree.
//!
//! Every transition here is a pure function over owned fields, so a scene, the bridge's landing
//! and the keyboard all move the same state. The summary machine is the desktop's own: the
//! reader's job lifecycle (`src/reader/jobs.rs` — `Section`, `Progress`, `Attempt`, `Completion`,
//! `Reader::request`/`poll_jobs`) is `pub(super)`, so the window drives the public
//! `hn_lib::reader::Network` boundary itself and models the card's four states here.
//!
//! The discussion is [`crate::comments::Thread`]: the tree, its fold set, the cursor, the
//! re-root and focus mode. This module owns the story around it — the article, the card, the
//! citation selection and the link chooser — and hands the thread's transitions straight through.

use hn_lib::reader::{Article, Discussion, Source as CapturedSource};
use hn_lib::HNCLIItem;

use crate::bridge::{CommentsRefreshed, StoryFilled, StoryUpdate, SummaryError};
use crate::comments::{Navigation, Thread};

/// One summary bullet, in the desktop's own shape so state can be cloned and compared.
///
/// `hn_lib::reader::Point` is `Deserialize` only, so it cannot be held in state that a test
/// compares; the conversion is a field copy, not a second implementation of anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bullet {
    pub text: String,
    pub sources: Vec<i32>,
}

/// A finished summary: its bullets and its provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub overview: String,
    pub bullets: Vec<Bullet>,
}

/// The summary card's four states (DESIGN.md screen 04).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SummaryState {
    /// No API key configured: the card says what to do instead of pretending to work.
    Unconfigured,
    /// Skeleton rows while the article and discussion captures and then summary are pending.
    Generating { previous: Option<Box<Summary>> },
    Done(Summary),
    /// A provider failure. The previous summary stays readable underneath, as DESIGN.md asks.
    Failed {
        message: String,
        /// The provider rejected the key, so the retry card offers settings instead of a retry.
        auth: bool,
        previous: Option<Box<Summary>>,
    },
}

impl SummaryState {
    /// The summary the card can still show, if any.
    pub fn readable(&self) -> Option<&Summary> {
        match self {
            SummaryState::Done(summary) => Some(summary),
            SummaryState::Failed { previous, .. } | SummaryState::Generating { previous, .. } => previous.as_deref(),
            _ => None,
        }
    }

    pub fn is_generating(&self) -> bool {
        matches!(self, SummaryState::Generating { .. })
    }

    pub fn is_failed(&self) -> bool {
        matches!(self, SummaryState::Failed { .. })
    }
}

/// The article pane's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArticleState {
    Loading,
    Ready { text: String, partial: bool },
    Failed(String),
}

/// The article synopsis is independent of the discussion and its citations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArticleSummaryState {
    Unconfigured,
    Generating { previous: Option<String> },
    Done(String),
    Failed { message: String, auth: bool, previous: Option<String> },
}

impl ArticleSummaryState {
    pub fn readable(&self) -> Option<&str> {
        match self {
            Self::Done(text) => Some(text),
            Self::Generating { previous } | Self::Failed { previous, .. } => previous.as_deref(),
            Self::Unconfigured => None,
        }
    }
}

/// One link the chooser offers: where it came from, and where it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkItem {
    /// The comment's author, or "the post": the chooser says which surface the link is on.
    pub source: String,
    pub url: String,
}

/// `L`'s chooser: the links the post and the focused comment carry.
///
/// README.md's `L` is "choose a link from the post or selected comment", so the list is both,
/// in that order, and the selection is a row index rather than a URL so two identical links stay
/// two rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkChooser {
    pub items: Vec<LinkItem>,
    pub selected: usize,
}

/// The captured comment and its temporary, independent thread navigation.
pub(crate) struct SourceView {
    pub id: i32,
    pub nav: Navigation,
}

impl LinkChooser {
    /// The link `Enter` would open.
    pub fn selected_url(&self) -> Option<&str> {
        self.items.get(self.selected).map(|item| item.url.as_str())
    }

    fn step(&mut self, forward: bool) {
        let count = self.items.len();
        if count == 0 {
            return;
        }
        self.selected = if forward {
            (self.selected + 1) % count
        } else {
            (self.selected + count - 1) % count
        };
    }
}

/// One open story: the split's left pane and the inspector's two cards.
pub struct Story {
    pub story: HNCLIItem,
    pub article: ArticleState,
    pub article_summary: ArticleSummaryState,
    pub summary: SummaryState,
    /// The discussion: the tree, the fold set, the cursor, the re-root and focus mode.
    pub thread: Thread,
    pub discussion: Option<Result<(), String>>,
    pub pending_reply: Option<crate::bridge::ReplyRequest>,
    /// The captured comment sources, for the citation viewer's excerpt and extra text.
    pub sources: Vec<CapturedSource>,
    /// Latest roots, separate from the sources attached to the displayed summary.
    pub current_sources: Vec<CapturedSource>,
    staged_summary: Option<(Summary, Vec<CapturedSource>)>,
    pub refreshing_comments: bool,
    pub refresh_error: Option<String>,
    /// `S` folds the summary card; the discussion gets the height.
    pub summary_open: bool,
    /// The selected citation, as an index into [`Story::citations`].
    pub citation: usize,
    /// `Tab` has selected a citation, so `Enter` reads it.
    pub citation_focus: bool,
    /// The source viewer's comment id, when it is open.
    pub(crate) source: Option<SourceView>,
    /// `L`'s chooser, when it is open.
    pub links: Option<LinkChooser>,
}

impl Story {
    /// A story that has just been opened: the article and the summary are both in flight.
    pub fn opening(story: HNCLIItem, configured: bool) -> Story {
        Story {
            story,
            article: ArticleState::Loading,
            article_summary: if configured { ArticleSummaryState::Generating { previous: None } } else { ArticleSummaryState::Unconfigured },
            summary: if configured {
                SummaryState::Generating {
                    previous: None,
                }
            } else {
                SummaryState::Unconfigured
            },
            thread: Thread::empty(),
            discussion: None,
            pending_reply: None,
            sources: Vec::new(),
            current_sources: Vec::new(),
            staged_summary: None,
            refreshing_comments: false,
            refresh_error: None,
            summary_open: true,
            citation: 0,
            citation_focus: false,
            source: None,
            links: None,
        }
    }

    pub fn id(&self) -> i32 {
        self.story.id
    }

    /// Take the synchronous aggregate answer through the same updates as a live story open.
    pub fn land(&mut self, filled: StoryFilled) -> bool {
        if filled.story_id != self.story.id {
            return false;
        }
        let story_id = filled.story_id;
        self.land_update(StoryUpdate::Article { story_id, article: filled.article, summary_skipped: false });
        self.land_update(StoryUpdate::ArticleSummary { story_id, summary: filled.article_summary });
        self.land_update(StoryUpdate::Discussion { story_id, comments: filled.comments, sources: filled.sources.clone(), summary_skipped: false });
        self.land_update(StoryUpdate::Summary { story_id, summary: filled.summary, sources: filled.sources });
        true
    }

    /// Each phase changes only its own surface. A late summary must not replace the article,
    /// the comment tree, or a reply the reader has opened since the discussion arrived.
    pub fn land_update(&mut self, update: StoryUpdate) -> bool {
        match update {
            StoryUpdate::Article { story_id, article, summary_skipped } if story_id == self.id() => {
                if summary_skipped { self.stop_summary(true); }
                self.article = match article {
                    Ok((text, partial)) => ArticleState::Ready { text, partial },
                    Err(message) => {
                        if matches!(self.article_summary, ArticleSummaryState::Generating { .. }) {
                            self.land_article_summary(Err(SummaryError::new(message.clone(), false)));
                        }
                        ArticleState::Failed(message)
                    },
                };
            }
            StoryUpdate::ArticleSummary { story_id, summary } if story_id == self.id() => {
                if !matches!(self.article_summary, ArticleSummaryState::Generating { .. }) { return false; }
                self.land_article_summary(summary);
            }
            StoryUpdate::Discussion { story_id, comments, sources, summary_skipped } if story_id == self.id() => {
                if summary_skipped { self.stop_summary(false); }
                if matches!(self.discussion, Some(Ok(()))) {
                    // A newer comments refresh may have arrived before this initial capture.
                    if let Err(message) = comments && self.summary.is_generating() {
                        self.land_summary(Err(SummaryError::new(format!("Discussion unavailable: {message}"), false)), Vec::new());
                    }
                    return true;
                }
                let first = self.discussion.is_none() && self.thread.is_empty();
                self.discussion = Some(match comments {
                    Ok(comments) => {
                        if first { self.thread = Thread::from_comments(&comments); }
                        else { self.thread.merge_comments(&comments, &[]); }
                        self.merge_current_sources(&comments, sources);
                        Ok(())
                    },
                    Err(message) => {
                        if self.summary.is_generating() {
                            self.land_summary(Err(SummaryError::new(format!("Discussion unavailable: {message}"), false)), Vec::new());
                        }
                        Err(message)
                    }
                });
            }
            StoryUpdate::Summary { story_id, summary, sources } if story_id == self.id() => {
                if !self.summary.is_generating() { return false; }
                self.land_summary(summary, sources);
            }
            _ => return false,
        }
        true
    }

    fn land_article_summary(&mut self, summary: Result<Article, SummaryError>) {
        let previous = self.article_summary.readable().map(str::to_owned);
        self.article_summary = match summary {
            Ok(article) => ArticleSummaryState::Done(article.text),
            Err(error) => ArticleSummaryState::Failed { message: error.message, auth: error.auth, previous },
        };
    }

    fn land_summary(&mut self, summary: Result<Discussion, SummaryError>, sources: Vec<CapturedSource>) {
        let previous = self.summary.readable().cloned().map(Box::new);
        self.summary = match summary {
            Ok(discussion) if self.source.is_some() && previous.is_some() => {
                self.staged_summary = Some((summary_of(discussion), sources));
                SummaryState::Done(*previous.unwrap())
            }
            Ok(discussion) => {
                self.sources = sources;
                SummaryState::Done(summary_of(discussion))
            }
            Err(error) => SummaryState::Failed {
                message: error.message,
                auth: error.auth,
                previous,
            },
        };
        // A citation index past the new bullet list would select nothing.
        self.citation = self.citation.min(self.citations().len().saturating_sub(1));
    }

    pub fn request_replies(&mut self, parent_id: i32, retry: bool) -> Option<crate::bridge::ReplyRequest> {
        if self.pending_reply.is_some() { return None; }
        let ids = self.thread.reply_batch(parent_id, retry);
        if ids.is_empty() { return None; }
        let request = crate::bridge::ReplyRequest { story_id: self.id(), parent_id, ids,
            depth: self.thread.tree_depth(parent_id)? as usize + 1 };
        self.pending_reply = Some(request.clone());
        Some(request)
    }

    pub fn land_replies(&mut self, filled: crate::bridge::ReplyFilled) -> bool {
        if filled.request.story_id != self.id() || self.pending_reply.as_ref() != Some(&filled.request) { return false; }
        let selected = self.source.as_ref().and_then(|source| self.thread.source_rows(&source.nav)
            .iter().position(|row| Some(row.id) == source.nav.cursor));
        self.pending_reply = None;
        self.thread.merge_replies(filled.request.parent_id, &filled.request.ids, filled.comments);
        self.repair_source_navigation(selected);
        true
    }

    /// What the toast says when a summary lands, if anything. A success is worth confirming
    /// because the card is in the inspector and may be folded; a failure is worth a toast
    /// because the card may be scrolled away.
    pub fn summary_notice(&self) -> Option<String> {
        if self.staged_summary.is_some() { return None; }
        if matches!(self.discussion, Some(Err(_))) {
            return Some("Discussion unavailable · press r to retry comments".to_string());
        }
        match &self.summary {
            SummaryState::Done(_) => Some("Discussion summary ready".to_string()),
            SummaryState::Failed { auth: true, .. } => {
                Some("Summary failed \u{00b7} re-enter your key".to_string())
            }
            SummaryState::Failed { .. } => {
                Some("Summary failed \u{00b7} retry in summary settings".to_string())
            }
            SummaryState::Generating { .. } | SummaryState::Unconfigured => None,
        }
    }

    /// `S`: fold or unfold the summary card.
    pub fn toggle_summary(&mut self) {
        self.summary_open = !self.summary_open;
        if !self.summary_open {
            self.citation_focus = false;
        }
    }

    /// The distinct comment ids the bullets cite, in first-cited order, restricted to sources
    /// this capture actually holds — the same rule the terminal reader applies, so a chip can
    /// never point at a comment the viewer cannot show.
    pub fn citations(&self) -> Vec<i32> {
        let mut ids = Vec::new();
        let Some(summary) = self.summary.readable() else {
            return ids;
        };
        for id in summary.bullets.iter().flat_map(|bullet| &bullet.sources) {
            if !ids.contains(id) && self.sources.iter().any(|source| source.id == *id) {
                ids.push(*id);
            }
        }
        ids
    }

    /// `Tab`: select the next citation, and ask for the summary to be readable.
    pub fn next_citation(&mut self) {
        self.step_citation(true);
    }

    /// `Shift+Tab`: select the previous citation.
    pub fn prev_citation(&mut self) {
        self.step_citation(false);
    }

    fn step_citation(&mut self, forward: bool) {
        let count = self.citations().len();
        if count == 0 {
            return;
        }
        // Tab is an explicit request to read the summary, as in the terminal reader.
        self.summary_open = true;
        if self.citation_focus {
            self.citation = if forward {
                (self.citation + 1) % count
            } else {
                (self.citation + count - 1) % count
            };
        }
        self.citation_focus = true;
        if let Some(id) = self.citations().get(self.citation).copied()
            && self.source.is_some()
        {
            // The viewer follows the selection, so Tab walks the sources.
            self.source = Some(SourceView { id, nav: Navigation::rooted(id) });
        }
    }

    /// A press on citation chip `index`: select it and read it, which is what `Tab` then
    /// `Enter` does from the keyboard.
    pub fn select_citation(&mut self, index: usize) -> bool {
        if index >= self.citations().len() {
            return false;
        }
        self.citation = index;
        self.citation_focus = true;
        self.summary_open = true;
        self.open_source()
    }

    /// `Enter` on a selected citation: open the source viewer. Returns whether it opened.
    pub fn open_source(&mut self) -> bool {
        if self.source.is_some() || !self.citation_focus {
            return false;
        }
        let Some(id) = self.citations().get(self.citation).copied() else {
            return false;
        };
        self.source = Some(SourceView { id, nav: Navigation::rooted(id) });
        true
    }

    /// `Esc` in the viewer: back to the summary, keeping the citation selected.
    pub fn close_source(&mut self) -> bool {
        let closed = self.source.take().map(|source| source.id);
        if let Some((summary, sources)) = self.staged_summary.take() {
            self.summary = SummaryState::Done(summary);
            self.sources = sources;
            let citations = self.citations();
            self.citation = closed.and_then(|id| citations.iter().position(|source| *source == id)).unwrap_or(0);
            self.citation_focus = !citations.is_empty();
        }
        closed.is_some()
    }

    /// The captured source the viewer is showing.
    pub fn shown_source(&self) -> Option<&CapturedSource> {
        let id = self.source.as_ref()?.id;
        self.sources.iter().find(|source| source.id == id)
    }

    /// Apply the same navigation operations to whichever view owns input.
    pub(crate) fn with_thread<T>(&mut self, f: impl FnOnce(&mut Thread) -> T) -> T {
        match &mut self.source {
            Some(source) => self.thread.with_navigation(&mut source.nav, f),
            None => f(&mut self.thread),
        }
    }

    /// `L`: offer the links the post and the focused comment carry. Returns whether there is
    /// anything to choose from — an empty chooser would be a dialog with nothing in it.
    pub fn open_links(&mut self) -> bool {
        let mut items: Vec<LinkItem> = self
            .story
            .links
            .iter()
            .map(|url| LinkItem {
                source: "the post".to_string(),
                url: url.clone(),
            })
            .collect();
        if let Some(node) = self.thread.cursor().and_then(|id| self.thread.node(id)) {
            items.extend(node.links.iter().map(|url| LinkItem {
                source: node.author.clone(),
                url: url.clone(),
            }));
        }
        if items.is_empty() {
            return false;
        }
        self.links = Some(LinkChooser { items, selected: 0 });
        true
    }

    /// `j` in the chooser.
    pub fn next_link(&mut self) -> bool {
        match &mut self.links {
            Some(chooser) => {
                chooser.step(true);
                true
            }
            None => false,
        }
    }

    /// `k` in the chooser.
    pub fn prev_link(&mut self) -> bool {
        match &mut self.links {
            Some(chooser) => {
                chooser.step(false);
                true
            }
            None => false,
        }
    }

    /// `Esc` in the chooser: close it and keep the discussion where it was.
    pub fn close_links(&mut self) -> bool {
        self.links.take().is_some()
    }

    /// `Enter` in the chooser: the URL to open, and the chooser closes behind it.
    pub fn take_link(&mut self) -> Option<String> {
        let url = self.links.as_ref()?.selected_url()?.to_string();
        self.links = None;
        Some(url)
    }

    /// Stop pending summaries after the key is cleared, retaining already paid-for results.
    pub(crate) fn stop_summaries(&mut self) { self.stop_summary(true); self.stop_summary(false); }

    fn stop_summary(&mut self, article: bool) {
        if !article && let SummaryState::Generating { previous } = &mut self.summary {
            self.summary = previous.take().map_or(SummaryState::Unconfigured, |summary| SummaryState::Done(*summary));
        }
        if article && let ArticleSummaryState::Generating { previous } = &mut self.article_summary {
            self.article_summary = previous.take().map_or(ArticleSummaryState::Unconfigured, ArticleSummaryState::Done);
        }
    }

    /// Start a replacement while keeping both readable summaries and their citations.
    pub fn retry(&mut self, configured: bool) {
        self.staged_summary = None;
        let previous = self.summary.readable().cloned().map(Box::new);
        let article_previous = self.article_summary.readable().map(str::to_owned);
        self.summary = if configured {
            SummaryState::Generating {
                previous,
            }
        } else {
            SummaryState::Unconfigured
        };
        self.article_summary = if configured { ArticleSummaryState::Generating { previous: article_previous } }
            else { ArticleSummaryState::Unconfigured };
    }

    pub fn land_refresh(&mut self, result: CommentsRefreshed, sources: Vec<CapturedSource>) {
        self.refreshing_comments = false;
        let CommentsRefreshed { comments, descendants, warning, .. } = result;
        let selected = self.source.as_ref().and_then(|source| self.thread.source_rows(&source.nav)
            .iter().position(|row| Some(row.id) == source.nav.cursor));
        self.thread.merge_comments(comments.as_deref().unwrap_or(&[]), &descendants);
        self.repair_source_navigation(selected);
        self.refresh_error = match comments {
            Ok(comments) => {
                self.merge_current_sources(&comments, sources);
                self.discussion = Some(Ok(()));
                warning
            }
            Err(error) => Some(match warning { Some(warning) => format!("{error}; {warning}"), None => error }),
        };
    }

    fn merge_current_sources(&mut self, comments: &[hn_lib::app::Comment], sources: Vec<CapturedSource>) {
        self.current_sources.retain(|source| !comments.iter().any(|comment| comment.id == source.id));
        self.current_sources.extend(sources);
    }

    fn repair_source_navigation(&mut self, selected: Option<usize>) {
        let Some(source) = &mut self.source else { return; };
        if source.nav.root.is_some_and(|id| id != source.id
            && self.thread.node(id).is_none_or(|node| !node.readable && !self.thread.has_children(id))) {
            let focus = source.nav.focus;
            source.nav = Navigation { focus, ..Navigation::rooted(source.id) };
        }
        let rows = self.thread.source_rows(&source.nav);
        if !rows.iter().any(|row| Some(row.id) == source.nav.cursor) {
            source.nav.cursor = rows.get(selected.unwrap_or(0).min(rows.len().saturating_sub(1))).map(|row| row.id);
        }
    }
}

fn summary_of(discussion: Discussion) -> Summary {
    Summary {
        overview: discussion.overview,
        bullets: discussion
            .points
            .into_iter()
            .map(|point| Bullet {
                text: point.text,
                sources: point.sources,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::SummaryError;
    use crate::fixtures;
    use hn_lib::reader::Point;

    fn story() -> Story {
        Story::opening(fixtures::item(1, 2), true)
    }

    #[test]
    fn late_initial_discussion_preserves_a_newer_refresh() {
        for fails in [false, true] {
            let mut story = story();
            let source = crate::service::Source::fake();
            let mut newer = fixtures::comments();
            newer[0].text = "newer comment".into();
            story.land_refresh(CommentsRefreshed {
                story_id: story.id(), comments: Ok(newer.clone()), descendants: Vec::new(), warning: None,
            }, source.capture(&newer));
            let mut older = newer.clone();
            older[0].text = "older comment".into();
            let old_capture = source.capture(&older);
            story.land_update(StoryUpdate::Discussion {
                story_id: story.id(),
                comments: if fails { Err("initial capture failed".into()) } else { Ok(older) },
                sources: old_capture,
                summary_skipped: false,
            });
            assert_eq!(story.discussion, Some(Ok(())));
            assert_eq!(story.thread.node(newer[0].id).unwrap().text, "newer comment");
            assert_eq!(story.current_sources.iter().find(|item| item.id == newer[0].id).unwrap().text, "newer comment");
            if fails { assert!(matches!(story.summary, SummaryState::Failed { .. })); }
        }
    }

    fn filled(summary: Result<Discussion, SummaryError>) -> StoryFilled {
        StoryFilled {
            story_id: 2,
            article: Ok(("# Libghostty 1.0\n\nBody.".to_string(), false)),
            article_summary: Ok(Article { text: "Article overview.".to_string() }),
            comments: Ok(fixtures::thread()),
            sources: fixtures::sources(),
            summary,
        }
    }

    fn discussion() -> Discussion {
        Discussion {
            overview: "Commenters split on the ABI.".to_string(),
            points: vec![
                Point {
                    text: "The ABI is deliberately narrow.".to_string(),
                    sources: vec![901, 902],
                },
                Point {
                    text: "A stable C ABI costs Zig's comptime.".to_string(),
                    sources: vec![903],
                },
            ],
        }
    }

    #[test]
    fn opening_starts_both_captures_and_no_discussion() {
        let story = story();
        assert_eq!(story.article, ArticleState::Loading);
        assert_eq!(
            story.summary,
            SummaryState::Generating { previous: None }
        );
        assert!(story.thread.is_empty());
        assert!(story.summary_open, "the card is pinned open on open");
        assert_eq!(story.citations(), Vec::<i32>::new());
    }

    #[test]
    fn an_unconfigured_reader_does_not_pretend_to_generate() {
        let story = Story::opening(fixtures::item(1, 2), false);
        assert_eq!(story.summary, SummaryState::Unconfigured);
        assert!(!story.summary.is_generating());
    }

    #[test]
    fn a_landing_fills_the_article_the_discussion_and_the_summary() {
        let mut story = story();
        assert!(story.land(filled(Ok(discussion()))));
        assert!(matches!(story.article, ArticleState::Ready { .. }));
        assert_eq!(story.thread.len(), fixtures::thread().len());
        assert_eq!(story.thread.node(901).unwrap().author, "tptacek");
        assert_eq!(story.thread.children_of(901), vec![9010, 9011]);
        let SummaryState::Done(summary) = &story.summary else {
            panic!("a landed summary is done");
        };
        assert_eq!(summary.bullets.len(), 2);
        assert_eq!(story.citations(), vec![901, 902, 903]);
    }

    #[test]
    fn a_landing_for_another_story_is_dropped() {
        let mut story = story();
        let mut other = filled(Ok(discussion()));
        other.story_id = 99;
        assert!(!story.land(other));
        assert_eq!(story.article, ArticleState::Loading);
        assert!(story.thread.is_empty());
    }

    #[test]
    fn a_failed_summary_keeps_the_old_one_readable() {
        let mut failed = story();
        failed.land(filled(Ok(discussion())));
        failed.retry(true);
        assert!(failed.summary.is_generating());
        assert!(failed.summary.readable().is_some(), "a retry keeps the card readable");
        failed.land(filled(Err(SummaryError::new(
            "OpenRouter is rate limited.",
            false,
        ))));
        let SummaryState::Failed {
            message,
            auth,
            previous,
        } = &failed.summary
        else {
            panic!("a failed landing is the failure card");
        };
        assert_eq!(message, "OpenRouter is rate limited.");
        assert!(!auth, "a rate limit is not an auth failure");
        assert!(previous.is_some(), "the failed replacement keeps the earlier summary");
        // The failure card still renders the bullets it had, when it had them.
        let mut kept = story();
        kept.land(filled(Ok(discussion())));
        let previous = kept.summary.readable().cloned();
        kept.summary = SummaryState::Failed {
            message: "boom".to_string(),
            auth: false,
            previous: previous.map(Box::new),
        };
        assert_eq!(kept.summary.readable().unwrap().bullets.len(), 2);
        assert_eq!(kept.citations(), vec![901, 902, 903]);
    }

    #[test]
    fn replacement_waits_until_the_open_citation_is_closed() {
        let mut story = story();
        story.land(filled(Ok(discussion())));
        story.next_citation();
        assert!(story.open_source());
        let original = story.shown_source().unwrap().text.clone();
        story.retry(true);
        let mut sources = fixtures::sources();
        sources.iter_mut().find(|source| source.id == 901).unwrap().text = "replacement capture".into();
        let mut replacement = discussion();
        replacement.overview = "A new discussion summary".into();
        assert!(story.land_update(StoryUpdate::Summary { story_id: 2, summary: Ok(replacement), sources }));
        assert_eq!(story.shown_source().unwrap().text, original);
        assert_eq!(story.summary.readable().unwrap().overview, "Commenters split on the ABI.");
        assert!(story.close_source());
        assert_eq!(story.sources.iter().find(|s| s.id == 901).unwrap().text, "replacement capture");
        assert_eq!(story.summary.readable().unwrap().overview, "A new discussion summary");
        assert!(story.citation_focus);
        assert_eq!(story.citations()[story.citation], 901);
    }

    #[test]
    fn replacement_keeps_the_source_selected_when_citations_reorder_or_disappear() {
        for replacement_kind in 0..3 {
            let mut story = story();
            story.land(filled(Ok(discussion())));
            story.next_citation();
            assert!(story.open_source());
            story.retry(true);
            let mut replacement = discussion();
            match replacement_kind {
                0 => replacement.points[0].sources.reverse(),
                1 => for point in &mut replacement.points { point.sources.retain(|id| *id != 901); },
                _ => for point in &mut replacement.points { point.sources.clear(); },
            }
            story.land_update(StoryUpdate::Summary { story_id: 2, summary: Ok(replacement), sources: fixtures::sources() });
            assert!(story.close_source());
            assert_eq!(story.citation_focus, replacement_kind != 2);
            match replacement_kind {
                0 => assert_eq!(story.citations()[story.citation], 901),
                1 => assert_eq!(story.citations()[story.citation], 902),
                _ => assert!(story.citations().is_empty()),
            }
        }
    }

    #[test]
    fn failed_article_replacement_keeps_its_previous_synopsis() {
        let mut story = story();
        story.land(filled(Ok(discussion())));
        story.retry(true);
        assert_eq!(story.article_summary.readable(), Some("Article overview."));
        story.land_update(StoryUpdate::ArticleSummary {
            story_id: 2, summary: Err(SummaryError::new("Provider unavailable", false)),
        });
        assert!(matches!(story.article_summary, ArticleSummaryState::Failed { .. }));
        assert_eq!(story.article_summary.readable(), Some("Article overview."));
        assert_eq!(story.summary.readable().unwrap().bullets.len(), 2);
    }

    #[test]
    fn a_failed_article_is_a_state_not_an_empty_pane() {
        let mut story = story();
        let mut landing = filled(Ok(discussion()));
        landing.article = Err("Could not fetch this article.".to_string());
        story.land(landing);
        assert_eq!(
            story.article,
            ArticleState::Failed("Could not fetch this article.".to_string())
        );
    }

    #[test]
    fn tab_selects_the_first_citation_and_enter_reads_it() {
        let mut story = story();
        story.land(filled(Ok(discussion())));
        assert!(!story.citation_focus);
        assert!(!story.open_source(), "Enter does nothing before Tab");
        story.next_citation();
        assert!(story.citation_focus);
        assert_eq!(story.citation, 0);
        assert!(story.open_source());
        assert_eq!(story.shown_source().map(|source| source.id), Some(901));
        assert_eq!(story.shown_source().unwrap().author, "tptacek");
        // A second Enter does not re-open; Esc closes and keeps the selection.
        assert!(!story.open_source());
        assert!(story.close_source());
        assert!(story.source.is_none());
        assert!(story.citation_focus);
        // Tab wraps.
        story.next_citation();
        story.next_citation();
        story.next_citation();
        assert_eq!(story.citation, 0);
    }

    #[test]
    fn a_citation_the_capture_does_not_hold_is_not_a_chip() {
        let mut story = story();
        let mut landing = filled(Ok(discussion()));
        landing.sources.retain(|source| source.id != 903);
        story.land(landing);
        assert_eq!(
            story.citations(),
            vec![901, 902],
            "a chip must point at a comment the viewer can show"
        );
    }

    #[test]
    fn folding_the_summary_drops_the_citation_selection() {
        let mut story = story();
        story.land(filled(Ok(discussion())));
        story.next_citation();
        assert!(story.citation_focus);
        story.toggle_summary();
        assert!(!story.summary_open);
        assert!(!story.citation_focus);
        story.toggle_summary();
        assert!(story.summary_open);
    }

    #[test]
    fn the_link_chooser_offers_the_post_and_the_focused_comment() {
        let mut story = story();
        story.land(filled(Ok(discussion())));
        // The fixture post is a link post, so its own links are empty; the focused comment
        // (901) carries none either, so there is nothing to choose from.
        assert!(!story.open_links());
        assert!(story.links.is_none());
        // 905 carries the design-notes link.
        story.thread.select(905);
        assert!(story.open_links());
        let chooser = story.links.as_ref().unwrap();
        assert_eq!(chooser.items.len(), 1);
        assert_eq!(chooser.items[0].source, "dang");
        assert_eq!(
            chooser.selected_url(),
            Some("https://mitchellh.com/writing/libghostty-is-coming")
        );
        // Enter takes the URL and closes the chooser behind it.
        assert_eq!(
            story.take_link().as_deref(),
            Some("https://mitchellh.com/writing/libghostty-is-coming")
        );
        assert!(story.links.is_none());
        assert!(story.take_link().is_none(), "a closed chooser opens nothing");
    }

    #[test]
    fn the_chooser_wraps_and_esc_closes_it() {
        let mut story = story();
        story.land(filled(Ok(discussion())));
        // 90100000 is behind the depth cap, so the tree has to be re-rooted at its parent.
        story.thread.reroot(901000);
        story.thread.select(90100000);
        assert!(story.open_links());
        assert_eq!(story.links.as_ref().unwrap().items.len(), 1);
        assert!(story.next_link());
        assert_eq!(story.links.as_ref().unwrap().selected, 0, "one row wraps to itself");
        assert!(story.prev_link());
        assert!(story.close_links());
        assert!(!story.close_links(), "closing twice is not a transition");
        assert!(!story.next_link(), "a closed chooser does not move");
    }
}
