//! Story opening, reading navigation and capture actions.

use super::{App, Arrival, Notice, Live};
use std::sync::Arc;
use hn_lib::HNCLIItem;
use rune::Tx;
use crate::story_io::{StoryRequest, StoryUpdate};
use crate::story::{ArticleSummaryState, RestoredSummary, Story, SummaryState};
use crate::summary_store::SummaryStore;
use super::input::reveal_citation;

impl App {
    /// Open `story`: the split springs open and its independent captures start.
    pub(super) fn open_story(&mut self, story: HNCLIItem) {
        self.hide_live();
        if !matches!(&self.live,
            Live::Created { story_id, .. } | Live::Failed { story_id, .. } if *story_id == story.id)
        {
            self.live = Live::Idle;
        }
        // A newly mounted story-keyed region must report current bounds before showing.
        if self.story.as_ref().is_none_or(|open| open.id() != story.id) {
            self.live_bounds = None;
        }
        // Returning to the feed has its own arrival, independent of this page's fill.
        self.feed_revision = 0;
        self.pending_arrival = None;
        self.arrival = None;
        self.park_story();
        self.inspector.get_mut().set_open(true);
        if let Some(mut reading) = self.readings.remove(&story.id) {
            self.bridge.resume_story(story.id);
            reading.story = story;
            self.story = Some(reading);
        } else {
            let mut open = Story::opening(story.clone(), self.configured() && self.config.automatic);
            self.restore_summary(&mut open);
            self.story = Some(open);
            self.request_story(story);
        }
    }

    fn restore_summary(&mut self, open: &mut Story) {
        let Some(store) = &mut self.summary_store else { return; };
        let Some(saved) = store.load(open.id()) else { return; };
        let mut article_age = None;
        let mut discussion = None;
        if let Some(article) = saved.article {
            article_age = Some(store.age(article.saved_at));
            open.article_summary = ArticleSummaryState::Generating { previous: None };
            open.land_update(StoryUpdate::ArticleSummary {
                story_id: open.id(), summary: Ok(article.article),
                provenance: article.provenance, empty_capture: false,
            });
        }
        if let Some(saved_discussion) = saved.discussion {
            discussion = Some((store.age(saved_discussion.saved_at), saved_discussion.comments_at_save));
            open.summary = SummaryState::Generating { previous: None };
            open.land_update(StoryUpdate::Summary {
                story_id: open.id(), summary: Ok((saved_discussion.discussion, saved_discussion.sources)),
                provenance: saved_discussion.provenance,
            });
        }
        open.restored_summary = (article_age.is_some() || discussion.is_some()).then(|| RestoredSummary {
            article_age, discussion, observed: std::time::Instant::now(),
        });
    }

    /// The row the feed has selected, if it is a story that loaded.
    pub(super) fn open_selected(&mut self) {
        let Some(id) = self.feed.selected_id() else {
            return;
        };
        self.open_id(id);
    }

    /// Open the current story's real page in the system browser with
    /// [`hn_lib::url_open::open_story_url`] — the same safe path the TUI uses
    /// (http/https only, falls back to the HN item page). No-op in headless runs.
    pub(super) fn open_current_external(&mut self) {
        if self.headless {
            return;
        }
        let item = self
            .story
            .as_ref()
            .map(|open| open.story.clone())
            .or_else(|| {
                let id = self.feed.selected_id()?;
                self.feed.stories.iter().find(|s| s.id == id).cloned()
            });
        if let Some(story) = item {
            if let Err(err) = hn_lib::url_open::open_story_url(&story.url, story.id) {
                self.feed.error = Some(err.to_string());
            }
        }
    }

    /// Open `id`, wherever the press came from: the row itself, or `Enter` on the
    /// selected row.
    pub(super) fn open_id(&mut self, id: i32) {
        let Some(story) = self
            .feed
            .stories
            .iter()
            .find(|story| story.id == id)
            .cloned()
        else {
            return;
        };
        if self.feed.failed.contains(&story.id) {
            return;
        }
        self.open_story(story);
    }

    /// Start the article and discussion captures; summary follows the discussion.
    fn request_story(&mut self, story: HNCLIItem) {
        let request = StoryRequest {
            story,
            config: self.config.clone(),
        };
        self.bridge.request_story(self.source.clone(), request);
        if self.headless {
            self.finish_story_now();
        }
    }

    pub(super) fn take_story_update(&mut self) -> Option<StoryUpdate> {
        let configured = self.configured();
        let article = self.story.as_ref().is_some_and(|story|
            matches!(story.article_summary, ArticleSummaryState::Generating { .. }));
        let discussion = self.story.as_ref().is_some_and(|story| story.summary.is_generating());
        self.bridge.take_story(&self.config, configured && article, configured && discussion)
    }

    pub(super) fn land_story_update(&mut self, update: StoryUpdate) -> bool {
        let fresh_article = matches!(&update, StoryUpdate::ArticleSummary { summary: Ok(_), .. });
        let fresh_discussion = matches!(&update, StoryUpdate::Summary { summary: Ok(_), .. });
        let comments = self.story.as_ref().and_then(|story| story.story.comments)
            .and_then(|count| u32::try_from(count).ok()).unwrap_or(0);
        let save = self.summary_store.as_ref().and_then(|_| SummaryStore::prepare(&update, comments));
        let landed = self.story.as_mut().is_some_and(|story| story.land_update(update));
        if landed {
            if let Some(story) = &mut self.story && let Some(restored) = &mut story.restored_summary {
                if fresh_article { restored.article_age = None; }
                if fresh_discussion { restored.discussion = None; }
                if restored.article_age.is_none() && restored.discussion.is_none() {
                    story.restored_summary = None;
                }
            }
            if let (Some(store), Some(save)) = (&mut self.summary_store, save) { store.save(save); }
        }
        landed
    }

    fn finish_story_now(&mut self) {
        while self.bridge.is_story_pending() {
            if let Some(update) = self.take_story_update() {
                self.land_story_update(update);
            } else {
                std::thread::yield_now();
            }
        }
    }

    /// `Esc` from the split: back to the feed, with the feed's request untouched.
    pub(crate) fn close_story(&mut self) {
        self.hide_live();
        self.park_story();
        self.inspector.get_mut().set_open(false);
        self.arrival = (!self.reduced).then_some(Arrival::Back);
    }

    fn park_story(&mut self) {
        if let Some(mut story) = self.story.take() {
            story.close_links();
            story.close_source();
            story.pending_reply = None;
            story.refreshing_comments = false;
            self.bridge.park_story(story.id());
            self.readings.insert(story.id(), story);
        }
    }

    /// Unwind the same reading layers for Escape, the footer and the mouse Back button.
    pub(crate) fn story_back(&mut self, tx: &mut Tx) -> bool {
        if self.settings.is_some() || self.help { return false; }
        let Some(story) = self.story.as_mut() else { return false; };
        if story.close_links() { return true; }
        if story.close_source() {
            reveal_citation(story, tx, true);
        } else {
            self.close_story();
            // The feed's scroll viewport is remounted on return.
            tx.scroll_to_instant("feed-list", 0.0, self.feed_scroll_y.get());
        }
        true
    }

    /// The open story, for the views' press handlers: a press on a comment row, a trail segment
    /// or a link is a transition on the thread, and the thread lives here.
    pub(crate) fn story_mut(&mut self) -> Option<&mut Story> {
        self.story.as_mut()
    }

    pub(crate) fn story(&self) -> Option<&Story> { self.story.as_ref() }

    pub(crate) fn load_replies(&mut self, parent: i32, retry: bool) {
        if let Some(story) = self.story.as_mut() {
            if story.source().is_none() { story.clear_citation_focus(); }
            story.with_thread(|thread| { thread.select(parent); });
        }
        let Some(request) = self.story.as_mut().and_then(|s| s.request_replies(parent, retry)) else { return; };
        if self.headless {
            let filled = self.bridge.replies_now(&self.source, request);
            if let Some(story) = self.story.as_mut() { story.land_replies(filled); }
        } else {
            self.bridge.request_replies(Arc::clone(&self.source), request);
        }
    }

    /// Opening a citation also starts the first reply batch without moving the discussion cursor.
    pub(crate) fn open_citation(&mut self, index: usize) {
        if self.story.as_mut().is_some_and(|story| story.select_citation(index)) {
            self.load_source_replies();
        }
    }

    pub(super) fn load_source_replies(&mut self) {
        let id = self.story.as_ref().and_then(|story| {
            let id = story.source()?.id();
            (story.thread.children_of(id).is_empty() && story.thread.reply_batch(id, false).is_some()).then_some(id)
        });
        if let Some(id) = id { self.load_replies(id, false); }
    }

    pub(crate) fn choose_links(&mut self) {
        if !self.story.as_mut().is_some_and(|story| story.open_links()) {
            self.toast = Some(Notice { message: "No links in this post or selected comment".to_string(), until: f64::INFINITY, show_dot: true });
        }
    }

    pub(crate) fn activate_comment(&mut self, id: i32) {
        let Some(story) = self.story.as_mut() else { return; };
        if story.source().is_none() { story.clear_citation_focus(); }
        story.with_thread(|thread| { thread.select(id); });
        if story.pending_reply.as_ref().is_some_and(|request| request.batch.parent_id() == id) {
            story.with_thread(|thread| { if thread.is_folded(id) { thread.unfold(); } else { thread.fold(); } });
            return;
        }
        let unloaded = story.thread.has_children(id) && story.thread.children_of(id).is_empty();
        let retry = unloaded && story.thread.reply_batch(id, false).is_none()
            && story.thread.reply_batch(id, true).is_some();
        story.with_thread(|thread| { if unloaded { thread.unfold(); } else { thread.activate(); } });
        if unloaded { self.load_replies(id, retry); }
    }

    /// Retry or regenerate summaries from the current captures, keeping the thread in place.
    /// The saved line's Regenerate. Without a key there is nothing to regenerate with, so it
    /// opens the summary settings instead of stopping the restored summaries.
    pub(crate) fn regenerate_saved_summary(&mut self) {
        if self.configured() { self.retry_summary(); } else { self.open_summary_settings(); }
    }

    pub(crate) fn retry_summary(&mut self) {
        let configured = self.configured();
        if let Some(story) = self.story.as_mut() {
            if !configured { story.stop_summaries(); return; }
            let request = StoryRequest { story: story.story.clone(), config: self.config.clone() };
            let article = match &story.article {
                crate::story::ArticleState::Ready { text, partial } => Some((text.clone(), *partial)),
                _ => None,
            };
            let sources = matches!(story.discussion, Some(Ok(()))).then(|| story.current_sources.clone());
            story.retry(configured);
            self.bridge.request_summaries(self.source.clone(), request, article, sources);
            if self.headless {
                self.finish_story_now();
            }
        }
    }

    /// Refresh roots and loaded replies without regenerating summaries.
    pub(crate) fn refresh_comments(&mut self) {
        let Some(story) = self.story.as_mut() else { return; };
        story.refreshing_comments = true;
        story.refresh_error = None;
        let story_id = story.id();
        let loaded = story.thread.loaded_reply_ids();
        if self.headless {
            let result = self.bridge.refresh_now(&self.source, story_id, loaded);
            self.land_comment_refresh(result);
        } else {
            self.bridge.request_refresh(self.source.clone(), story_id, loaded);
        }
    }

    pub(super) fn land_comment_refresh(&mut self, result: crate::story_io::CommentsRefreshed) -> bool {
        let Some(story) = self.story.as_mut().filter(|story| story.id() == result.story_id) else { return false; };
        story.land_refresh(result);
        true
    }
}
