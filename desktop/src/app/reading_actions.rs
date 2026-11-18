//! Story opening, reading navigation and capture actions.

use super::{App, Notice, Arrival, Live};
use std::sync::Arc;
use hn_lib::HNCLIItem;
use rune::Tx;
use crate::story_io::StoryRequest;
use crate::story::Story;
use crate::webview;
use super::input::reveal_citation;

impl App {
    /// Open `story`: the split springs open and its independent captures start.
    pub(super) fn open_story(&mut self, story: HNCLIItem) {
        self.hide_live();
        if !matches!(self.live,
            Live::Ready { story_id } | Live::Failed { story_id, .. } if story_id == story.id)
        {
            webview::close();
            self.live = Live::Idle;
        }
        // A newly mounted story-keyed region must report current bounds before showing.
        if self.story.as_ref().is_none_or(|open| open.id() != story.id) {
            self.live_bounds = None;
        }
        // Returning to the feed is not another page arrival.
        self.feed_revision = 0;
        self.pending_arrival = None;
        self.arrival = Arrival::None;
        self.park_story();
        self.inspector.set_open(true);
        if let Some(mut reading) = self.readings.remove(&story.id) {
            self.bridge.resume_story(story.id);
            reading.story = story;
            self.story = Some(reading);
        } else {
            self.story = Some(Story::opening(story.clone(), self.configured() && self.config.automatic));
            self.request_story(story);
        }
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
        if self.headless {
            let filled = self.bridge.story_now(self.source.as_ref(), request);
            if let Some(open) = &mut self.story {
                open.land(filled);
            }
        } else {
            self.bridge.request_story(self.source.clone(), request);
        }
    }

    /// `Esc` from the split: back to the feed, with the feed's request untouched.
    pub(crate) fn close_story(&mut self) {
        self.hide_live();
        self.park_story();
        self.inspector.set_open(false);
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
            if story.source.is_none() { story.citation_focus = false; }
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
            let id = story.source.as_ref()?.id;
            (story.thread.children_of(id).is_empty() && !story.thread.reply_batch(id, false).is_empty()).then_some(id)
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
        if story.source.is_none() { story.citation_focus = false; }
        story.with_thread(|thread| { thread.select(id); });
        if story.pending_reply.as_ref().is_some_and(|request| request.parent_id == id) {
            story.with_thread(|thread| { if thread.is_folded(id) { thread.unfold(); } else { thread.fold(); } });
            return;
        }
        let unloaded = story.thread.has_children(id) && story.thread.children_of(id).is_empty();
        let retry = unloaded && story.thread.reply_batch(id, false).is_empty()
            && !story.thread.reply_batch(id, true).is_empty();
        story.with_thread(|thread| { if unloaded { thread.unfold(); } else { thread.activate(); } });
        if unloaded { self.load_replies(id, retry); }
    }

    /// Retry or regenerate summaries from the current captures, keeping the thread in place.
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
                while self.bridge.is_story_pending() {
                    if let Some(update) = self.bridge.take_story(&self.config, true, true) {
                        if let Some(story) = self.story.as_mut() { story.land_update(update); }
                    } else {
                        std::thread::yield_now();
                    }
                }
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
        let sources = result.comments.as_ref().map(|comments| self.source.capture(comments)).unwrap_or_default();
        story.land_refresh(result, sources);
        true
    }
}
