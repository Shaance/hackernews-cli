//! The story split's state: the article, the summary card's machine, and the flat discussion.
//!
//! Every transition here is a pure function over owned fields, so a scene, the bridge's landing
//! and the keyboard all move the same state. The summary machine is the desktop's own: the
//! reader's job lifecycle (`src/reader/jobs.rs` — `Section`, `Progress`, `Attempt`, `Completion`,
//! `Reader::request`/`poll_jobs`) is `pub(super)`, so the window drives the public
//! `hn_lib::reader::Network` boundary itself and models the card's four states here.
//!
//! The discussion is a flat list of top-level comments. Threading, the ancestor trail, the depth
//! cap and focus mode are WS-D's; nothing in this module knows about depth.

use hn_lib::app::Comment;
use hn_lib::reader::{Article, Discussion, Source as CapturedSource};
use hn_lib::HNCLIItem;

use crate::bridge::StoryFilled;

/// How many capture stages one story open runs: the article, the discussion, the summary.
///
/// The card's provenance line counts these — "2 of 3 sources captured" — because they are the
/// three things the aggregate request actually does. It is not an invented total: the HN API
/// reports no comment count the card could divide by.
pub const STAGES: usize = 3;

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
    /// Capture stages that landed, out of [`STAGES`].
    pub captured: usize,
    pub of: usize,
}

/// The summary card's four states (DESIGN.md screen 04).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SummaryState {
    /// No API key configured: the card says what to do instead of pretending to work.
    Unconfigured,
    /// Skeleton rows. The reader boundary returns a whole `Discussion` at once, so the app
    /// lands here with `captured` at 0 and leaves for `Done`/`Failed` in one step; the count is
    /// the state a streaming boundary would advance.
    Generating { captured: usize, of: usize },
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
            SummaryState::Failed { previous, .. } => previous.as_deref(),
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

/// One top-level comment in the discussion pane: author, age, an excerpt of the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentRow {
    pub id: i32,
    pub author: String,
    pub time_ago: String,
    pub excerpt: String,
    /// Replies under this comment. Shown as a count only — opening them is WS-D's.
    pub replies: usize,
}

/// How much of a comment the flat list shows before it stops.
const EXCERPT_CHARS: usize = 320;

impl CommentRow {
    fn new(comment: &Comment) -> CommentRow {
        let text = comment.text.trim();
        let excerpt: String = text.chars().take(EXCERPT_CHARS).collect();
        CommentRow {
            id: comment.id,
            author: comment.author.clone(),
            time_ago: comment.time_ago.clone(),
            excerpt: if excerpt.chars().count() < text.chars().count() {
                format!("{excerpt}\u{2026}")
            } else {
                excerpt
            },
            replies: comment.child_ids.len(),
        }
    }
}

/// One open story: the split's left pane and the inspector's two cards.
pub struct Story {
    pub story: HNCLIItem,
    pub article: ArticleState,
    pub summary: SummaryState,
    pub discussion: Vec<CommentRow>,
    /// The captured comment sources, for the citation viewer's excerpt and extra text.
    pub sources: Vec<CapturedSource>,
    /// `S` folds the summary card; the discussion gets the height.
    pub summary_open: bool,
    /// The selected citation, as an index into [`Story::citations`].
    pub citation: usize,
    /// `Tab` has selected a citation, so `Enter` reads it.
    pub citation_focus: bool,
    /// The source viewer's comment id, when it is open.
    pub source: Option<i32>,
}

impl Story {
    /// A story that has just been opened: the article and the summary are both in flight.
    pub fn opening(story: HNCLIItem, configured: bool) -> Story {
        Story {
            story,
            article: ArticleState::Loading,
            summary: if configured {
                SummaryState::Generating {
                    captured: 0,
                    of: STAGES,
                }
            } else {
                SummaryState::Unconfigured
            },
            discussion: Vec::new(),
            sources: Vec::new(),
            summary_open: true,
            citation: 0,
            citation_focus: false,
            source: None,
        }
    }

    pub fn id(&self) -> i32 {
        self.story.id
    }

    /// Take the aggregate request's answer. A landing for another story is dropped, so a
    /// superseded request can never paint over the story the reader is looking at.
    pub fn land(&mut self, filled: StoryFilled) -> bool {
        if filled.story_id != self.story.id {
            return false;
        }
        self.article = match filled.article {
            Ok(Article { text }) => ArticleState::Ready {
                text,
                partial: filled.article_partial,
            },
            Err(message) => ArticleState::Failed(message),
        };
        self.discussion = filled.comments.iter().map(CommentRow::new).collect();
        self.sources = filled.sources;
        let previous = self.summary.readable().cloned().map(Box::new);
        self.summary = match filled.summary {
            Ok(discussion) => SummaryState::Done(summary_of(discussion)),
            Err(error) => SummaryState::Failed {
                message: error.message,
                auth: error.auth,
                previous,
            },
        };
        // A citation index past the new bullet list would select nothing.
        self.citation = self.citation.min(self.citations().len().saturating_sub(1));
        true
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
            self.source = Some(id);
        }
    }

    /// `Enter` on a selected citation: open the source viewer. Returns whether it opened.
    pub fn open_source(&mut self) -> bool {
        if self.source.is_some() || !self.citation_focus {
            return false;
        }
        let Some(id) = self.citations().get(self.citation).copied() else {
            return false;
        };
        self.source = Some(id);
        true
    }

    /// `Esc` in the viewer: back to the summary, keeping the citation selected.
    pub fn close_source(&mut self) -> bool {
        self.source.take().is_some()
    }

    /// The captured source the viewer is showing.
    pub fn shown_source(&self) -> Option<&CapturedSource> {
        let id = self.source?;
        self.sources.iter().find(|source| source.id == id)
    }

    /// `r`: ask for the aggregate again, keeping what is already readable on screen.
    pub fn retry(&mut self, configured: bool) {
        self.article = ArticleState::Loading;
        self.summary = if configured {
            SummaryState::Generating {
                captured: 0,
                of: STAGES,
            }
        } else {
            SummaryState::Unconfigured
        };
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
        captured: STAGES,
        of: STAGES,
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

    fn filled(summary: Result<Discussion, SummaryError>) -> StoryFilled {
        StoryFilled {
            story_id: 2,
            article: Ok(Article {
                text: "# Libghostty 1.0\n\nBody.".to_string(),
            }),
            article_partial: false,
            comments: fixtures::comments(),
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
            SummaryState::Generating {
                captured: 0,
                of: STAGES
            }
        );
        assert!(story.discussion.is_empty());
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
        assert_eq!(story.discussion.len(), fixtures::comments().len());
        assert_eq!(story.discussion[0].author, "tptacek");
        assert_eq!(story.discussion[0].replies, 2);
        let SummaryState::Done(summary) = &story.summary else {
            panic!("a landed summary is done");
        };
        assert_eq!(summary.bullets.len(), 2);
        assert_eq!(summary.captured, STAGES);
        assert_eq!(story.citations(), vec![901, 902, 903]);
    }

    #[test]
    fn a_landing_for_another_story_is_dropped() {
        let mut story = story();
        let mut other = filled(Ok(discussion()));
        other.story_id = 99;
        assert!(!story.land(other));
        assert_eq!(story.article, ArticleState::Loading);
        assert!(story.discussion.is_empty());
    }

    #[test]
    fn a_failed_summary_keeps_the_old_one_readable() {
        let mut failed = story();
        failed.land(filled(Ok(discussion())));
        failed.retry(true);
        assert!(failed.summary.is_generating());
        assert!(
            failed.summary.readable().is_none(),
            "a retry clears the card while it runs"
        );
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
        assert!(previous.is_none(), "the retry had already cleared it");
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
        assert_eq!(story.source, Some(901));
        assert_eq!(story.shown_source().unwrap().author, "tptacek");
        // A second Enter does not re-open; Esc closes and keeps the selection.
        assert!(!story.open_source());
        assert!(story.close_source());
        assert_eq!(story.source, None);
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
    fn a_long_comment_is_excerpted_with_an_ellipsis() {
        let mut comment = fixtures::comments()[0].clone();
        comment.text = "x".repeat(EXCERPT_CHARS + 10);
        let row = CommentRow::new(&comment);
        assert_eq!(row.excerpt.chars().count(), EXCERPT_CHARS + 1);
        assert!(row.excerpt.ends_with('\u{2026}'));
    }
}
