//! The window: `SplitView` shell, the responsive feed, the Saved view, and the story split.
//!
//! The view reads [`App`] and native child readiness, whose commit callback wakes the UI;
//! it writes back only the measured grid because `view` takes `&S` and `poll` needs the number.
//! Everything else — the
//! ranked window, the page stepper, the selection — is `hn_lib::feed_state`'s arithmetic
//! applied by [`FeedModel`].
//!
//! One window, one `SplitView`. Opening a story puts the article in the main pane and the
//! summary card plus the discussion in the inspector, which springs open from 0 width on
//! `MORPH`; `Esc` closes it and the feed is exactly where it was.
//!
//! The article pane is the story's **live page**: a `WKWebView` child view pinned to the pane's
//! rect (see [`crate::webview`]). The readability capture still runs in the background — it is
//! what the summary card and its citations read — but it is never drawn unless the child view
//! cannot exist, which is the fallback branch [`Pane::Extracted`] draws.

use std::cell::Cell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use hn_lib::app::StoryType;
use hn_lib::event::StoryAction;
use hn_lib::feed_state;
use hn_lib::reader::{self, Config};
use hn_lib::HNCLIItem;
use rune::tok::{self, radius, type_};
use rune::{
    col, el, row, text, AppSpec, Color, Cx, El, KeyEvent, ScrollSpec, Spring, SpringPair,
    TextStyle, Transition, Tx, Waker, WindowHandle,
};
use rune_kit::shell::PaneMotion;
use rune_kit::{Keybind, Kit, PaneState, SplitView, Toast, ToastRegion, Toolbar};

use crate::bookmarks::SavedView;
use crate::bridge::{Bridge, Request, StoryRequest, StoryUpdate};
use crate::feed::{FeedModel, Fill, Filled};
use crate::help;
use crate::keys::{self, StoryKey, WindowKey};
use crate::layout::Layout;
use crate::service::Source;
use crate::settings::Settings;
use crate::story::{Story, SummaryState};
use crate::story_view::{self, Pane};
use crate::webview;

/// One feed row, in points. The measured row count is the viewport height over this.
const ROW_HEIGHT: f32 = 56.0;
/// Window width at which the feed becomes two columns (DESIGN.md's table).
const BREAKPOINT: f32 = 1400.0;
/// The grid the first request asks for: the 1180×760 default window, one column.
const DEFAULT_GRID: (usize, usize) = (10, 1);
const FEED_GUTTER: f32 = 20.0;
const DEFAULT_FEED_HEIGHT: f32 = 661.0;
/// How long the save confirmation stays.
const TOAST_SECONDS: f64 = 2.4;
/// How long a request may stay in flight before the feed says so and offers a retry. Without
/// it a dropped answer leaves the feed loading forever, and every page turn is refused.
const LOAD_TIMEOUT: f64 = 10.0;
/// How often the window's own state may be written. A grip drag moves the width every frame,
/// and a file per frame is not worth it.
const LAYOUT_SECONDS: f64 = 2.0;
/// Rune can stop its display link between wheel events; keeping it warm briefly gives the
/// discussion a more even native presentation cadence without drawing continuously at rest.
const SCROLL_PACING_SECONDS: f64 = 0.05;
/// The inspector's width range: 46% of the 1180 pt default window, and enough either side for
/// the summary card to stay readable.
const INSPECTOR_WIDTH: f32 = 543.0;
const INSPECTOR_MIN: f32 = 360.0;
const INSPECTOR_MAX: f32 = 640.0;

/// The transient save confirmation. `until` is on the frame clock, so `poll` expires it.
struct Notice {
    message: String,
    until: f64,
}

/// Whether a request takes a new ranking or pages against the one the feed already has.
///
/// Only a refresh (and the first load of a tab) re-snapshots: a page turn or a resize must keep
/// the ranks the reader was shown, or the story under the cursor changes under them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ranking {
    Fresh,
    Keep,
}

/// Why a newly accepted feed group arrives. Refresh and resize keep their current identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Arrival {
    Initial,
    Tab(i8),
    Page(i8),
    None,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ScrollPacing {
    Idle,
    Moved,
    Until(f64),
}

/// The live article pane's child view.
///
/// The pane is the story's real page by default. This is the one thing that can take it away:
/// a child view that could not be built. Everything else — headless, non-macOS — is decided by
/// [`App::live_capable`] before a child is ever asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Live {
    /// No child view yet: nothing has asked for one.
    Idle,
    /// A child belongs to this story; its native commit callback owns readiness.
    Ready { story_id: i32 },
    /// Creation failed. The pane falls back to the extracted text and says why.
    Failed { story_id: i32, error: String },
}

/// `--open-story <id>`: the story to open once the feed holds it. `AppSpec::init` is a plain
/// `fn() -> App`, so the command line reaches the state through this instead of a capture.
static OPEN_STORY: OnceLock<Option<i32>> = OnceLock::new();

pub struct App {
    feed: FeedModel,
    saved: SavedView,
    source: Arc<Source>,
    bridge: Bridge,
    /// The grid the last commit measured. Written by `view`, read by `poll`.
    measured: Cell<Option<(usize, usize)>>,
    /// The newest fill and ranking policy, so resize preserves a fresh request in flight.
    requested: Option<(Fill, Ranking)>,
    /// Accepted tab/page changes replace this group; refresh and resize keep its identity.
    feed_revision: usize,
    pending_arrival: Option<Arrival>,
    arrival: Arrival,
    /// When the in-flight request started, on the frame clock, for [`LOAD_TIMEOUT`].
    loading_since: Option<f64>,
    /// The row under the pointer, for the hover actions.
    hover: Option<i32>,
    /// Passive shortcut hints yield visual emphasis to pointer use and return on navigation.
    hints: rune::Live<f32>,
    /// The open story, when the split is showing one. `None` is the feed.
    story: Option<Story>,
    /// Readings kept for this window's lifetime, including their summary citation snapshots.
    readings: HashMap<i32, Story>,
    /// The reading viewports' scroll observers and poll share this frame-pacing state.
    scroll_pacing: Rc<Cell<ScrollPacing>>,
    scroll_waker: Option<Waker>,
    /// The inspector's width and open state. Opening a story opens it; `Esc` closes it.
    inspector: PaneState,
    /// The reader's summary settings, loaded once at launch and never logged.
    config: Config,
    /// The summary settings dialog, when it is open. `None` is closed.
    settings: Option<Settings>,
    /// The keyboard sheet, when it is open.
    pub help: bool,
    /// What the window remembers between runs, and where it writes it. A headless run has
    /// neither: a golden must not depend on the machine's window size.
    layout: Layout,
    layout_path: Option<PathBuf>,
    /// The layout as last written, so `poll` writes only when something moved.
    layout_written: Layout,
    /// When the layout was last written, on the frame clock, for [`LAYOUT_SECONDS`].
    layout_saved_at: f64,
    /// The window size the last commit measured. Written by `view`, read by `poll`.
    window_size: Cell<Option<(f32, f32)>>,
    /// Whether the remembered window size has been asked for. `AppSpec::size` is compile-time,
    /// so the restore is one `Tx::resize_window_instant` on the first frame.
    restored: bool,
    toast: Option<Notice>,
    /// `HN_REDUCED_MOTION=1`, the same override the terminal app honours.
    reduced: bool,
    /// Headless runs use the fake service and an in-memory Saved store, so a golden never
    /// depends on the network or on the machine's `saved.json`.
    headless: bool,
    /// Set by `O` or the footer's browser action; drained in `poll`,
    /// which opens the real page in the system browser (no-op when headless).
    pub(crate) pending_external: bool,
    /// The window handle Rune lent in `on_window`, for building the live pane's child view.
    /// `None` before the window exists, and always `None` in a headless run.
    native_window: Option<WindowHandle>,
    /// The live pane's child view: whether it exists, and what it is showing.
    live: Live,
    /// The rect the article pane's live region last reported, in window points. The child is
    /// pinned to it.
    live_bounds: Option<[f32; 4]>,
    /// Whether the child is currently shown, so `poll` only calls `set_visible` on a change.
    live_shown: bool,
    /// `--open-story <id>`, until the feed holds that story and it opens.
    open_story: Option<i32>,
}

impl App {
    pub(crate) fn keyboard_navigation(&self, ev: &KeyEvent) {
        if self.settings.is_some() || self.help || keys::parity_key(ev).is_none() { return; }
        if matches!(ev.key, rune::Key::Tab | rune::Key::Home | rune::Key::End | rune::Key::PageUp | rune::Key::PageDown)
            || (self.story.is_some() && keys::story_key(ev) != StoryKey::None)
            || (self.story.is_none() && (keys::story_action(ev) != StoryAction::None
                || matches!(ev.key, rune::Key::Left | rune::Key::Right)))
        {
            self.hints.snap(1.0);
        }
    }

    pub(crate) fn pointer_wheel(s: &mut App, ev: rune::WheelEvent, _: &mut Tx) -> bool {
        if !ev.momentum && (ev.dx != 0.0 || ev.dy != 0.0) { s.hints.set(0.3); }
        false
    }

    fn scroll_observer(&self) -> impl Fn(rune::ScrollPos) + 'static {
        let pacing = Rc::clone(&self.scroll_pacing);
        let waker = self.scroll_waker.clone();
        let previous = Cell::new(0.0);
        // At this Rune pin, a source-less live-only handler can leave its live wake pending.
        // The offset observer wakes poll without a handler commit or a live-only event.
        move |at| {
            if previous.replace(at.offset[1]) == at.offset[1] { return; }
            if pacing.replace(ScrollPacing::Moved) == ScrollPacing::Idle && let Some(waker) = &waker {
                waker();
            }
        }
    }

    /// Ask for the window `grid` shows. `ranking` says whether this request re-snapshots.
    ///
    /// Saved is the local store, not a feed: it renders every saved story from memory and
    /// never reaches the source, so the viewport applies to display only.
    fn request(&mut self, grid: (usize, usize), ranking: Ranking) {
        let fill = self.feed.fill(grid.0, grid.1);
        self.requested = Some((fill, ranking));
        self.feed.status.error = None;
        if fill.tab == StoryType::Saved {
            self.feed.apply_saved(self.saved.store());
            self.accept_arrival();
            return;
        }
        let snapshot = match ranking {
            Ranking::Fresh => None,
            Ranking::Keep if !self.feed.id_snapshot.is_empty()
                && self.feed.stories_for.is_some_and(|(tab, _)| tab == self.feed.tab) => {
                Some(self.feed.id_snapshot.clone())
            }
            Ranking::Keep => None,
        };
        // A complete held window can settle immediately against the same ranked snapshot.
        // Fresh requests always reach the source; missing/failed items retain the normal fetch path.
        if let Some(ids) = snapshot.as_ref() {
            let end = (fill.from_rank + fill.count).min(ids.len());
            if let Some(window) = ids.get(fill.from_rank..end) {
                let held: Option<Vec<_>> = window.iter()
                    .map(|id| self.feed.story_cache.get(id).cloned().map(Ok)).collect();
                if let Some(items) = held {
                    self.bridge.cancel_feed();
                    self.feed.apply_fill(Filled { tab: fill.tab, ids: ids.clone(), from_rank: fill.from_rank, items, error: None });
                    self.accept_arrival();
                    return;
                }
            }
        }
        self.feed.status.loading = true;
        let request = Request::new(fill, snapshot);
        if self.headless {
            let filled = self.bridge.fill_now(self.source.as_ref(), request);
            self.feed.apply_fill(filled);
            self.accept_arrival();
        } else {
            self.bridge.request(self.source.clone(), request);
        }
    }

    /// Re-request the current window with a new ranking, as `r` does. The story the reader has
    /// selected stays selected: the feed re-anchors on its id when the new snapshot lands.
    fn refresh(&mut self) {
        self.loading_since = None;
        self.pending_arrival = None;
        self.arrival = Arrival::None;
        self.feed.pending_anchor = self.feed.selected_id();
        let grid = self.measured.get().unwrap_or(DEFAULT_GRID);
        self.request(grid, Ranking::Fresh);
    }

    fn toggle_saved(&mut self) {
        let Some(id) = self.feed.selected_id() else {
            return;
        };
        self.toggle_saved_id(id);
    }

    /// Save or unsave `id`, wherever the press came from: the row's hover action, or `s` on
    /// the selected row.
    fn toggle_saved_id(&mut self, id: i32) {
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
        let saved = self.saved.toggle(&story);
        // A store that could not be read or written says so, rather than confirming a save
        // that did not land.
        self.toast = Some(Notice {
            message: match (self.saved.error(), saved) {
                (Some(_), _) => "Saved stories unavailable · Reload in Saved".to_string(),
                (None, true) => "Saved · Press 4 for Saved".to_string(),
                (None, false) => "Removed from Saved".to_string(),
            },
            until: f64::INFINITY,
        });
        if self.feed.tab == StoryType::Saved {
            self.refresh();
        }
    }

    fn apply(&mut self, action: feed_state::FeedAction) {
        let grid = self.measured.get().unwrap_or(DEFAULT_GRID);
        // A tab switch is a new feed and needs its own ranking; a page turn or a resize pages
        // against the one on screen.
        let ranking = match action {
            feed_state::FeedAction::SwitchTab { .. } => Ranking::Fresh,
            _ => Ranking::Keep,
        };
        let arrival = match action {
            feed_state::FeedAction::TurnPage { display_cursor, .. } => Some(Arrival::Page(
                if display_cursor < self.feed.display_cursor { -1 } else { 1 })),
            feed_state::FeedAction::SwitchTab { tab, .. } => Some(Arrival::Tab(
                if tab_index(tab) < tab_index(self.feed.tab) { -1 } else { 1 })),
            _ => None,
        };
        let applied = self.feed.apply(action, grid);
        if applied.fill.is_some() {
            self.loading_since = None;
            self.pending_arrival = arrival;
            self.request(grid, ranking);
        }
    }

    fn accept_arrival(&mut self) -> bool {
        let Some(arrival) = self.pending_arrival.take() else { return false; };
        if self.feed.status.error.is_some() { self.arrival = Arrival::None; return false; }
        self.arrival = arrival;
        if arrival != Arrival::Initial {
            self.feed_revision += 1;
            self.hover = None;
        }
        true
    }

    fn cells(&self) -> usize {
        let (rows, cols) = self.measured.get().unwrap_or(DEFAULT_GRID);
        rows * cols
    }

    /// Whether a summary can be asked for: the fake provider is a fixture, so a headless run
    /// needs no key, and a live one needs the reader's own `Config::key`.
    fn configured(&self) -> bool {
        self.headless || !self.config.key().is_empty()
    }

    /// Open `story`: the split springs open and its independent captures start.
    fn open_story(&mut self, story: HNCLIItem) {
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
            self.story = Some(Story::opening(story.clone(), self.configured()));
            self.request_story(story);
        }
    }

    /// The row the feed has selected, if it is a story that loaded.
    fn open_selected(&mut self) {
        let Some(id) = self.feed.selected_id() else {
            return;
        };
        self.open_id(id);
    }

    /// Open the current story's real page in the system browser with
    /// [`hn_lib::url_open::open_story_url`] — the same safe path the TUI uses
    /// (http/https only, falls back to the HN item page). No-op in headless runs.
    fn open_current_external(&mut self) {
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
                self.feed.status.error = Some(err.to_string());
            }
        }
    }

    /// Open `id`, wherever the press came from: the row itself, or `Enter` on the
    /// selected row.
    fn open_id(&mut self, id: i32) {
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

    /// Whether this run can host a live page at all: macOS, with a real window.
    ///
    /// A `WKWebView` needs an `NSWindow`, and a headless run has none — `--snap` and `--tree`
    /// render offscreen. So headless always takes the fallback branch, which is what makes the
    /// scene trees deterministic: the same arguments produce the same tree on every machine,
    /// with no child view to depend on.
    fn live_capable(&self) -> bool {
        cfg!(target_os = "macos") && !self.headless
    }

    /// Whether the article pane is the live page rather than the extracted text.
    fn live_mode(&self) -> bool {
        self.live_capable() && !matches!(self.live, Live::Failed { .. })
    }

    /// What the article pane draws: the live page, or the extracted text and why it is there.
    fn pane(&self) -> Pane<'_> {
        if self.live_mode() {
            Pane::Live { loading: !webview::ready() }
        } else {
            Pane::Extracted {
                reason: match &self.live {
                    Live::Failed { error, .. } => Some(error.as_str()),
                    _ => None,
                },
            }
        }
    }

    /// The article pane's live region reported its rect: pin the child to it.
    ///
    /// Called from `on_layout`, which runs on the UI thread — the only thread a `WKWebView` may
    /// be resized on.
    pub(crate) fn live_rect(&mut self, rect: [f32; 4]) {
        self.live_bounds = Some(rect);
        if self.live_shown {
            webview::set_bounds(rect);
        }
    }

    /// Build this story's hidden child once the window and current pane bounds exist.
    /// Show it after its own navigation commits, only while the pane is on screen.
    ///
    /// The child is a subview of the window's content view, so it draws above everything Rune
    /// paints. That is why it is hidden while a dialog is open: the settings dialog and the
    /// keyboard sheet are Rune's, and a webview over them would bury the controls.
    fn sync_live(&mut self) -> bool {
        if !self.live_mode() {
            return self.hide_live();
        }
        let Some(story) = self.story.as_ref() else {
            return self.hide_live();
        };
        // The same scheme-checked URL the terminal reader opens, with the HN item page as the
        // fallback for a story that has no usable link.
        let story_id = story.id();
        let url = hn_lib::url_open::safe_story_url(&story.story.url, story_id);
        let mut changed = false;
        if matches!(self.live, Live::Idle) {
            let (Some(window), Some(bounds)) = (self.native_window.as_ref(), self.live_bounds)
            else {
                // The window or the pane's rect has not landed yet; the next frame has both.
                return false;
            };
            if let Err(error) = webview::create(window, bounds, &url) {
                self.live = Live::Failed { story_id, error };
                return true;
            }
            self.live = Live::Ready { story_id };
            changed = true;
        }
        let want = webview::ready() && self.live_bounds.is_some()
            && self.settings.is_none() && !self.help;
        if self.live_shown != want {
            if want {
                webview::set_bounds(self.live_bounds.unwrap());
            }
            webview::set_visible(want);
            self.live_shown = want;
            changed = true;
        }
        changed
    }

    /// Take the child off screen, keeping the page: `set_visible` is `WKWebView.setHidden`, so
    /// scroll position and session survive the feed coming back.
    fn hide_live(&mut self) -> bool {
        if !self.live_shown {
            return false;
        }
        webview::set_visible(false);
        self.live_shown = false;
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

    fn load_source_replies(&mut self) {
        let id = self.story.as_ref().and_then(|story| {
            let id = story.source.as_ref()?.id;
            (story.thread.children_of(id).is_empty() && !story.thread.reply_batch(id, false).is_empty()).then_some(id)
        });
        if let Some(id) = id { self.load_replies(id, false); }
    }

    pub(crate) fn choose_links(&mut self) {
        if !self.story.as_mut().is_some_and(|story| story.open_links()) {
            self.toast = Some(Notice { message: "No links in this post or selected comment".to_string(), until: f64::INFINITY });
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

    /// The settings dialog's draft, for its own press handlers.
    pub(crate) fn settings_mut(&mut self) -> Option<&mut Settings> {
        self.settings.as_mut()
    }

    /// Whether the open story's summary failed because the provider rejected the key. That is
    /// the one failure a retry cannot fix, so `M` opens the dialog asking for a new key.
    fn auth_failed(&self) -> bool {
        self.story
            .as_ref()
            .is_some_and(|story| matches!(&story.summary, SummaryState::Failed { auth: true, .. })
                || matches!(&story.article_summary, crate::story::ArticleSummaryState::Failed { auth: true, .. }))
    }

    /// The summary card's settings button, and `M`: the dialog, asking for a key when the
    /// provider rejected the last one.
    pub(crate) fn open_summary_settings(&mut self) {
        self.open_settings(self.auth_failed());
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

    fn land_comment_refresh(&mut self, result: crate::bridge::CommentsRefreshed) -> bool {
        let Some(story) = self.story.as_mut().filter(|story| story.id() == result.story_id) else { return false; };
        let sources = result.comments.as_ref().map(|comments| self.source.capture(comments)).unwrap_or_default();
        story.land_refresh(result, sources);
        true
    }

    /// `M` / `⌘,`: the summary settings dialog, on a copy of the reader's config.
    fn open_settings(&mut self, reenter: bool) {
        let path = if self.headless {
            None
        } else {
            Config::path().ok()
        };
        self.settings = Some(Settings::open(&self.config, path, reenter));
    }

    /// `Cancel`, `Esc` or the close button: the draft is dropped and nothing is written.
    pub(crate) fn close_settings(&mut self) {
        self.settings = None;
    }

    /// `Apply`, and `Regenerate summary`. A save failure keeps the dialog open with the
    /// message; a success closes it, adopts the config and says so.
    pub(crate) fn apply_settings(&mut self, refresh: bool) {
        let Some(settings) = self.settings.as_mut() else {
            return;
        };
        let Ok(config) = settings.apply() else {
            return;
        };
        self.config = config;
        self.settings = None;
        self.toast = Some(Notice {
            message: "Settings saved".to_string(),
            until: f64::INFINITY,
        });
        if !self.configured() {
            for story in self.story.iter_mut().chain(self.readings.values_mut()) {
                story.stop_summaries();
            }
        }
        if refresh {
            if self.story.is_some() {
                self.retry_summary();
            } else {
                self.refresh();
            }
        }
    }

    /// Put the reader back where they left off: the tab, the page, the selected story and the
    /// inspector's width. A headless run has no layout, so this is a no-op there.
    fn restore(&mut self) {
        if let Some(tab) = self.layout.tab {
            self.feed.tab = tab;
        }
        if let Some(page) = self.layout.page.filter(|page| *page > 1) {
            self.feed.display_cursor = (page as usize - 1) * self.feed.page_size as usize;
        }
        self.feed.pending_anchor = self.layout.selected;
        if let Some(width) = self.layout.inspector {
            self.inspector = PaneState::new(width.clamp(INSPECTOR_MIN, INSPECTOR_MAX))
                .range(INSPECTOR_MIN, INSPECTOR_MAX)
                .collapsed();
        }
    }

    /// What the window would write now: its size, the inspector's width, and where the feed is.
    fn snapshot(&self) -> Layout {
        Layout {
            window: self.window_size.get().or(self.layout.window),
            inspector: Some(self.inspector.width()),
            tab: Some(self.feed.tab),
            page: Some(self.feed.stepper()),
            selected: self.feed.selected_id(),
        }
    }

    /// Write `layout`, remembering it so an unchanged window writes nothing.
    fn write_layout(&mut self, layout: Layout, now: f64) {
        let Some(path) = self.layout_path.as_ref() else {
            return;
        };
        if layout.save(path).is_ok() {
            self.layout = layout.clone();
            self.layout_written = layout;
            self.layout_saved_at = now;
        }
    }
}

fn init() -> App {
    let headless = rune::is_headless();
    let source = if headless {
        Source::fake()
    } else {
        Source::live()
    };
    let saved = if headless {
        // The fixture marks four stories saved, so the dot is part of every headless scene.
        // The store is built in `saved_ids()` order, not page order: `bookmarks::toggle_saved`
        // prepends, so a real store is newest-first and the Saved view must match it.
        let page = crate::fixtures::page();
        let store = crate::fixtures::saved_ids()
            .into_iter()
            .filter_map(|id| page.iter().find(|story| story.id == id).cloned())
            .collect();
        SavedView::seeded(store)
    } else {
        SavedView::load()
    };
    // The reader's own settings file, read once. A headless run uses the defaults so a golden
    // never depends on the machine's `summary.json` or on an API key in the environment.
    let config = if headless {
        Config::default()
    } else {
        Config::path()
            .ok()
            .and_then(|path| reader::load_config(&path).ok())
            .unwrap_or_default()
    };
    // The window's own state, read once. A headless run has none: a golden must not depend on
    // the machine's window size, and a golden run must not overwrite the reader's.
    let layout_path = if headless { None } else { Layout::path() };
    let layout = layout_path
        .as_deref()
        .map(Layout::load)
        .unwrap_or_default();
    App {
        feed: FeedModel::new(),
        saved,
        source: Arc::new(source),
        bridge: Bridge::new().expect("a tokio runtime for the feed's requests"),
        measured: Cell::new(None),
        requested: None,
        feed_revision: 0,
        pending_arrival: Some(Arrival::Initial),
        arrival: Arrival::Initial,
        loading_since: None,
        hover: None,
        hints: rune::Live::new(1.0),
        story: None,
        readings: HashMap::new(),
        scroll_pacing: Rc::new(Cell::new(ScrollPacing::Idle)),
        scroll_waker: None,
        inspector: PaneState::new(INSPECTOR_WIDTH)
            .range(INSPECTOR_MIN, INSPECTOR_MAX)
            .collapsed(),
        config,
        settings: None,
        help: false,
        layout_written: layout.clone(),
        layout,
        layout_path,
        layout_saved_at: 0.0,
        window_size: Cell::new(None),
        restored: false,
        pending_external: false,
        native_window: None,
        live: Live::Idle,
        live_bounds: None,
        live_shown: false,
        open_story: OPEN_STORY.get().copied().flatten(),
        toast: None,
        reduced: std::env::var("HN_REDUCED_MOTION").is_ok_and(|value| value == "1"),
        headless,
    }
}

/// One aggregate fill request, then the waker that brings its answer back to `poll`.
fn setup(s: &mut App, waker: Waker) {
    webview::set_waker(waker.clone());
    s.scroll_waker = Some(waker.clone());
    s.bridge.set_waker(waker);
    // The remembered tab and page decide what the first request asks for, so they land before
    // it. The measured grid re-requests if the window is not the default size.
    s.restore();
    s.request(DEFAULT_GRID, Ranking::Fresh);
}

fn poll(s: &mut App, now: f64, tx: &mut Tx) -> bool {
    let mut changed = false;
    if webview::take_back() { changed |= s.story_back(tx); }
    let mut feed_arrived = false;
    let mut summary_arrived = false;
    let mut replies_arrived = false;
    match s.scroll_pacing.get() {
        ScrollPacing::Moved => {
            s.scroll_pacing.set(ScrollPacing::Until(now + SCROLL_PACING_SECONDS));
            tx.wake_after(Duration::ZERO);
        }
        ScrollPacing::Until(until) if now < until => tx.wake_after(Duration::ZERO),
        ScrollPacing::Until(_) => s.scroll_pacing.set(ScrollPacing::Idle),
        ScrollPacing::Idle => {}
    }
    if let Some(filled) = s.bridge.take() {
        let landed = s.feed.apply_fill(filled);
        changed |= landed;
        if landed { feed_arrived = s.accept_arrival(); }
    }
    while let Some(update) = {
        let configured = s.configured();
        let article = s.story.as_ref().is_some_and(|story|
            matches!(story.article_summary, crate::story::ArticleSummaryState::Generating { .. }));
        let discussion = s.story.as_ref().is_some_and(|story| story.summary.is_generating());
        s.bridge.take_story(&s.config, configured && article, configured && discussion)
    } {
        let summary_finished = matches!(&update,
            StoryUpdate::Summary { .. } | StoryUpdate::Discussion { comments: Err(_), .. });
        let visible = s.inspector.is_open() && s.settings.is_none() && !s.help;
        // A landing for a story that is no longer open is dropped by `Story::land_update`.
        let landed = s.story.as_mut().map(|open| {
            let first_result = match &update {
                StoryUpdate::ArticleSummary { summary: Ok(_), .. } => matches!(
                    open.article_summary, crate::story::ArticleSummaryState::Generating { previous: None }),
                StoryUpdate::Summary { summary: Ok(_), .. } => matches!(
                    open.summary, SummaryState::Generating { previous: None }),
                _ => false,
            };
            let show = visible && open.summary_open && open.source.is_none();
            let visible_success = show && matches!(&update, StoryUpdate::Summary { summary: Ok(_), .. });
            let landed = open.land_update(update);
            (landed, landed && first_result && show,
                (landed && summary_finished && !visible_success).then(|| open.summary_notice()).flatten())
        });
        if let Some((landed, arrival, notice)) = landed {
            changed |= landed;
            summary_arrived |= arrival;
            // The card is in the inspector and may be folded or scrolled away, so the summary's
            // outcome is confirmed where the reader is looking.
            if let Some(message) = notice {
                s.toast = Some(Notice {
                    message,
                    until: f64::INFINITY,
                });
            }
        }
    }
    if let Some(result) = s.bridge.take_refresh() {
        changed |= s.land_comment_refresh(result);
    }
    if let Some(filled) = s.bridge.take_replies() {
        if let Some(story) = &mut s.story {
            let before: std::collections::HashSet<_> = match &story.source {
                Some(source) => story.thread.source_rows(&source.nav),
                None => story.thread.rows(),
            }.into_iter().map(|row| row.id).collect();
            let landed = story.land_replies(filled);
            let after = match &story.source {
                Some(source) => story.thread.source_rows(&source.nav),
                None => story.thread.rows(),
            };
            replies_arrived = landed && s.inspector.is_open() && s.settings.is_none() && !s.help
                && after.iter().any(|row| !before.contains(&row.id));
            changed |= landed;
        }
    }
    // `O` / "Open in browser": the real page, in the system browser, via the TUI's safe
    // opener (scheme-checked, HN item-page fallback). Headless runs never open anything.
    if std::mem::take(&mut s.pending_external) {
        s.open_current_external();
    }
    // `--open-story <id>`: open the story once the feed holds it. It waits for the first fill
    // rather than opening something else, and says so if the id is not on this page.
    if let Some(id) = s.open_story.take() {
        if s.feed.stories.iter().any(|story| story.id == id) {
            s.open_id(id);
            changed = true;
        } else if s.feed.status.loading || s.feed.stories.is_empty() {
            s.open_story = Some(id);
        } else {
            s.feed.status.error = Some(format!("--open-story {id}: not on this page"));
            changed = true;
        }
    }
    // The live article pane: build the child view, load the open story's page, and keep it in
    // step with the split. A no-op in a headless run, where the pane is the extracted text.
    changed |= s.sync_live();
    // One request per invalidation: the measured grid is the only thing that moves it. A
    // resize keeps an in-flight fresh request fresh; otherwise it uses the accepted ranking.
    if let Some(grid) = s.measured.get() {
        let need = s.feed.fill(grid.0, grid.1);
        if s.requested.map(|(fill, _)| fill) != Some(need) {
            let ranking = if s.feed.status.loading {
                s.requested.map_or(Ranking::Keep, |(_, ranking)| ranking)
            } else {
                Ranking::Keep
            };
            if !matches!(s.pending_arrival, Some(Arrival::Initial | Arrival::Tab(_))) {
                s.pending_arrival = None;
            }
            s.request(grid, ranking);
            changed = true;
        }
    }
    // A request that never lands must not leave the feed loading forever.
    if s.feed.status.loading {
        let since = *s.loading_since.get_or_insert(now);
        if s.feed.timed_out(now - since, LOAD_TIMEOUT) {
            s.bridge.cancel_feed();
            s.loading_since = None;
            s.pending_arrival = None;
            s.arrival = Arrival::None;
            changed = true;
        } else {
            tx.wake_after(Duration::from_secs_f64(LOAD_TIMEOUT));
        }
    } else {
        s.loading_since = None;
    }
    // The window size the reader left, asked for once. `AppSpec::size` is compile-time, so a
    // restore is a resize; a headless run has no window and no remembered size.
    if !s.headless && !s.restored {
        s.restored = true;
        if let Some(size) = s.layout.window {
            tx.resize_window_instant(size);
            changed = true;
        }
    }
    // The window's own state, written when it moves and at most every [`LAYOUT_SECONDS`].
    if !s.headless {
        let snapshot = s.snapshot();
        if snapshot != s.layout_written {
            let waited = now - s.layout_saved_at;
            if waited >= LAYOUT_SECONDS {
                s.write_layout(snapshot, now);
            } else {
                tx.wake_after(Duration::from_secs_f64(LAYOUT_SECONDS - waited));
            }
        }
    }
    if let Some(notice) = &mut s.toast {
        if notice.until.is_infinite() {
            notice.until = now + TOAST_SECONDS;
            tx.wake_after(Duration::from_secs_f64(TOAST_SECONDS));
        } else if now >= notice.until {
            s.toast = None;
            changed = true;
        }
    }
    if changed {
        if (feed_arrived || summary_arrived || replies_arrived) && !s.reduced {
            tx.animated();
        } else {
            tx.instant();
        }
    }
    changed
}

fn on_key(s: &mut App, ev: &KeyEvent, tx: &mut Tx) -> bool {
    s.keyboard_navigation(ev);
    // The shell's chords work everywhere, including over a dialog, as a menu item's would.
    match keys::window_key(ev) {
        WindowKey::Settings => {
            s.open_summary_settings();
            return true;
        }
        WindowKey::Help => {
            s.help = !s.help;
            return true;
        }
        WindowKey::ToggleSplit => {
            if s.story.is_some() {
                s.inspector.set_open(!s.inspector.is_open());
            }
            return true;
        }
        WindowKey::None => {}
    }
    // A dialog owns the keys while it is open: the feed behind it must not move. `Esc` never
    // reaches here — the runtime dismisses the modal first — and `?` closes the sheet.
    if s.settings.is_some() {
        return false;
    }
    if s.help {
        if keys::story_action(ev) == StoryAction::ToggleHelp {
            s.help = false;
            return true;
        }
        return false;
    }
    if s.story.is_some() && keys::story_action(ev) == StoryAction::ToggleHelp {
        s.help = true;
        return true;
    }
    // `O` (capital) always opens the real page in the system browser — the TUI's
    // `reader_actions.rs:42,74` distinction — never the extracted article split.
    if keys::is_open_external(ev) {
        s.pending_external = true;
        return true;
    }
    if s.story.is_some() {
        return on_story_key(s, ev, tx);
    }
    if matches!(
        keys::parity_key(ev),
        Some(crossterm::event::KeyCode::Left | crossterm::event::KeyCode::Right)
    ) {
        let (_, cols) = s.measured.get().unwrap_or(DEFAULT_GRID);
        let shown = s.feed.stories.len();
        if shown > 0 && !s.feed.status.loading {
            let cols = cols.max(1);
            let per_column = shown.div_ceil(cols);
            let column = s.feed.selected / per_column;
            let right = matches!(ev.key, rune::Key::Right);
            if (right && column + 1 < cols && (column + 1) * per_column < shown)
                || (!right && column > 0)
            {
                let start = if right { (column + 1) * per_column } else { (column - 1) * per_column };
                s.feed.selected = (start + s.feed.selected % per_column).min(shown - 1);
            } else if !right || s.feed.display_cursor + shown < s.feed.id_snapshot.len() {
                let mut action = if right { feed_state::next_page(&s.feed.state(), s.cells()) }
                    else { feed_state::prev_page(&s.feed.state(), s.cells()) };
                if let feed_state::FeedAction::TurnPage { landing, .. } = &mut action {
                    *landing = Some(if right { hn_lib::app::PageLanding::Start } else { hn_lib::app::PageLanding::End });
                    s.apply(action);
                }
            }
            if let Some(id) = s.feed.selected_id() {
                tx.scroll_into_view(format!("story-{id}"));
            }
        }
        return true;
    }
    let cells = s.cells();
    let state = s.feed.state();
    let action = match keys::story_action(ev) {
        StoryAction::None => return false,
        StoryAction::Quit => {
            // The window's own state is written on the way out, whatever the throttle says.
            if !s.headless {
                let snapshot = s.snapshot();
                s.write_layout(snapshot, rune::scene_clock().now);
            }
            tx.quit();
            return true;
        }
        StoryAction::NextStory => feed_state::next_story(&state, cells),
        StoryAction::PrevStory => feed_state::prev_story(&state, cells),
        StoryAction::FirstStory => feed_state::FeedAction::Move { selected: 0 },
        StoryAction::LastStory => feed_state::FeedAction::Move {
            selected: state.stories.len().saturating_sub(1),
        },
        StoryAction::NextPage => feed_state::next_page(&state, cells),
        StoryAction::PrevPage => feed_state::prev_page(&state, cells),
        StoryAction::SetType(tab) => {
            tx.blur();
            feed_state::switch_tab(&state, tab)
        },
        StoryAction::ToggleBookmark => {
            s.toggle_saved();
            return true;
        }
        StoryAction::Refresh => {
            s.refresh();
            return true;
        }
        // `Enter`, `o` and `c` all open the story: the split is where the article and the
        // discussion live, so there is no separate "open in browser" to choose between.
        StoryAction::OpenUrl | StoryAction::ViewComments => {
            s.open_selected();
            return true;
        }
        StoryAction::ToggleHelp => {
            s.help = true;
            return true;
        }
    };
    s.apply(action);
    if let Some(id) = s.feed.selected_id() {
        tx.scroll_into_view(format!("story-{id}"));
    }
    true
}

/// The story split's keys, from `src/reader/interaction.rs:7` through [`keys::story_key`], plus
/// DESIGN.md's thread table.
///
/// The order is the layering, and `Esc` unwinds it in reverse: the link chooser takes keys while
/// it is open, then the source viewer, then the discussion, and only then the feed.
fn on_story_key(s: &mut App, ev: &KeyEvent, tx: &mut Tx) -> bool {
    let key = keys::story_key(ev);
    if matches!(key, StoryKey::Back) { return s.story_back(tx); }
    let Some(story) = s.story.as_mut() else {
        return false;
    };
    // The chooser is a surface over the discussion: while it is open it owns j/k, Enter and Esc.
    if story.links.is_some() {
        match key {
            StoryKey::NextCitation | StoryKey::PrevCitation => return false,
            StoryKey::NextComment => {
                story.next_link();
                return true;
            }
            StoryKey::PrevComment => {
                story.prev_link();
                return true;
            }
            StoryKey::Enter => {
                if let Some(url) = story.take_link() {
                    let _ = tx.open_url(&url);
                }
                return true;
            }
            _ => {}
        }
    }
    // Source navigation shares the thread operations but keeps its own cursor and folds.
    if story.source.is_some() {
        match key {
            StoryKey::NextScreen => {
                tx.scroll_page("source-viewer", 1);
                return true;
            }
            StoryKey::PrevScreen => {
                tx.scroll_page("source-viewer", -1);
                return true;
            }
            StoryKey::NextCitation => {
                story.next_citation();
                tx.instant();
                tx.scroll_to_y("source-viewer", 0.0);
                s.load_source_replies();
                return true;
            }
            StoryKey::PrevCitation => {
                story.prev_citation();
                tx.instant();
                tx.scroll_to_y("source-viewer", 0.0);
                s.load_source_replies();
                return true;
            }
            StoryKey::Links => {
                s.toast = Some(Notice { message: "Open links from the discussion".to_string(), until: f64::INFINITY });
                return true;
            }
            StoryKey::ToggleSummary | StoryKey::Refresh | StoryKey::Settings | StoryKey::ToggleSaved
            | StoryKey::NextSummaryScreen | StoryKey::PrevSummaryScreen => return true,
            _ => {}
        }
    }
    let in_source = story.source.is_some();
    let (root, cursor) = story.with_thread(|thread| (thread.root(), thread.cursor()));
    match key {
        StoryKey::None => false,
        StoryKey::Back => unreachable!("back is handled before borrowing the story"),
        StoryKey::NextCitation => {
            if story.citations().is_empty() { return false; }
            story.next_citation();
            reveal_citation(story, tx, false);
            true
        }
        StoryKey::PrevCitation => {
            if story.citations().is_empty() { return false; }
            story.prev_citation();
            reveal_citation(story, tx, false);
            true
        }
        StoryKey::Enter => {
            // A selected citation is what Enter reads; otherwise it is the thread's own expand,
            // which re-roots on a `continue thread` row.
            if story.open_source() {
                s.load_source_replies();
                return true;
            }
            let id = cursor;
            let can_activate = id.is_some_and(|id| story.thread.has_children(id));
            if let Some(id) = id { s.activate_comment(id); }
            if can_activate {
                if let Some(story) = s.story.as_mut() { thread_moved(story, root, tx); }
            }
            true
        }
        StoryKey::ToggleSummary => {
            story.toggle_summary();
            true
        }
        StoryKey::Refresh => {
            s.refresh_comments();
            true
        }
        StoryKey::Settings => {
            s.open_summary_settings();
            true
        }
        StoryKey::Links => { s.choose_links(); true },
        StoryKey::ToggleSaved => {
            let id = story.id();
            s.toggle_saved_id(id);
            true
        }
        StoryKey::NextComment | StoryKey::PrevComment | StoryKey::NextSibling | StoryKey::PrevSibling
        | StoryKey::Parent | StoryKey::Fold | StoryKey::FirstComment | StoryKey::LastComment
        | StoryKey::PopTrail => {
            if !in_source { story.citation_focus = false; }
            let source_root = story.source.as_ref().map(|source| source.id);
            let loading = story.pending_reply.as_ref().is_some_and(|request| Some(request.parent_id) == cursor);
            let changed = story.with_thread(|thread| match key {
                StoryKey::NextComment => thread.next(),
                StoryKey::PrevComment => thread.prev(),
                StoryKey::NextSibling if !in_source || cursor != root => thread.next_sibling(),
                StoryKey::PrevSibling if !in_source || cursor != root => thread.prev_sibling(),
                StoryKey::Parent if cursor != source_root => thread.to_parent(),
                StoryKey::Fold if matches!(&ev.key, rune::Key::Text(text) if text == "c") && !loading => thread.collapse(),
                StoryKey::Fold => thread.fold(),
                StoryKey::FirstComment => thread.first(),
                StoryKey::LastComment => thread.last(),
                StoryKey::PopTrail if cursor != source_root => thread.pop_trail(),
                _ => false,
            });
            if changed { thread_moved(story, root, tx); }
            true
        }
        StoryKey::Unfold => {
            if !in_source { story.citation_focus = false; }
            let id = cursor;
            let retry = id.is_some_and(|id| story.thread.children_of(id).is_empty()
                && story.thread.reply_batch(id, false).is_empty()
                && !story.thread.reply_batch(id, true).is_empty());
            if story.with_thread(|thread| thread.unfold()) { thread_moved(story, root, tx); }
            if let Some(id) = id { s.load_replies(id, retry); }
            true
        }
        StoryKey::NextScreen => {
            tx.scroll_page("discussion-scroll", 1);
            true
        }
        StoryKey::PrevScreen => {
            tx.scroll_page("discussion-scroll", -1);
            true
        }
        StoryKey::NextSummaryScreen => {
            tx.scroll_page("summary-scroll", 1);
            true
        }
        StoryKey::PrevSummaryScreen => {
            tx.scroll_page("summary-scroll", -1);
            true
        }
        StoryKey::Focus => {
            if !in_source { story.citation_focus = false; }
            story.with_thread(|thread| thread.toggle_focus());
            true
        }
    }
}

fn reveal_citation(story: &Story, tx: &mut Tx, focus: bool) {
    // A replacement can remove every citation while its old source is still open.
    if focus {
        tx.focus("summary-scroll", true);
        if !story.citation_focus { tx.announce("Back to summary", rune::LiveRegion::Polite); }
    }
    if !story.citation_focus { return; }
    let Some(id) = story.citations().get(story.citation).copied() else { return; };
    let Some(index) = story.summary.readable().and_then(|summary| summary.bullets.iter().position(|point| point.sources.contains(&id))) else { return; };
    let key = format!("summary-text-{index}");
    tx.scroll_into_view_in("summary-scroll", key.clone());
    if focus {
        // Rune's inline links are accessibility nodes inside one text element, so the
        // viewport is the focusable return target; the selected reference stays visible.
        if let Some(source) = story.sources.iter().find(|source| source.id == id) {
            tx.announce(format!("Back to summary, comment by {} selected", source.author), rune::LiveRegion::Polite);
        }
    }
}

/// Keep the focused comment in view after a thread transition.
///
/// A transition that re-roots the tree glides the viewport to the new root on `GLIDE` — the
/// rows crossfade in place, so the offset is the only thing that travels. Anything else just
/// reveals the row the cursor landed on.
fn thread_moved(story: &mut Story, before: Option<i32>, tx: &mut Tx) {
    if let Some(source) = &story.source {
        if source.nav.root != before {
            tx.scroll_to_y("source-viewer", 0.0);
        } else if let Some(id) = source.nav.cursor {
            tx.scroll_into_view_in("source-viewer", format!("source-comment-{id}"));
        }
        return;
    }
    if story.thread.root() != before {
        tx.scroll_to("discussion-scroll", 0.0, 0.0);
    } else if let Some(id) = story.thread.cursor() {
        tx.scroll_into_view_in("discussion-scroll", format!("comment-{id}"));
    }
}

fn view(s: &App, cx: &mut Cx<App>) -> El {
    let dark = cx.system_dark();
    if s.reduced {
        cx.reduced_motion(true);
    }
    let reduced = cx.is_reduced_motion();
    let contrast = cx.system_increased_contrast();
    let t = tok::theme(dark);
    let k = Kit::neutral(dark);
    let accent = tok::accent(3, dark);
    // DESIGN.md's focus affordance is amber, not AppKit's blue: the view-wide default, so every
    // control that does not set its own ring gets the same one.
    cx.focus_ring(2.0, 1.0, accent);

    // The window root, not the shell's inset card: the breakpoint is a window width. The rect
    // is also the size `poll` remembers, so the reader's resize survives the next launch.
    let window = cx.layout_with("window", |rect| rect);
    if let Some(rect) = window {
        s.window_size.set(Some((rect[2], rect[3])));
    }
    let wide = window.is_some_and(|rect| rect[2] >= BREAKPOINT);
    let cols = if wide { 2 } else { 1 };
    // The feed's grid is only measurable while the feed is on screen: a story open must not
    // overwrite it with the default and make `poll` re-request the wrong window.
    let feed_height = cx.layout_with("feed-list", |rect| rect[3]).unwrap_or(DEFAULT_FEED_HEIGHT);
    if s.story.is_none() {
        let gap = if cols == 2 { 0.0 } else { 2.0 };
        let rows = ((feed_height - 2.0 * FEED_GUTTER + gap) / (ROW_HEIGHT + gap)).floor().max(1.0) as usize;
        s.measured.set(Some((rows, cols)));
    }

    let mut toolbar = Toolbar::new().leading(feed_tabs(s, &k, t, accent, reduced));
    if s.feed.tab == StoryType::Saved {
        toolbar = toolbar.trailing(
            text(s.saved.path_label())
                .font(type_::LABEL)
                .color(t.text_secondary)
                .truncate_middle(),
        );
    }
    toolbar = toolbar.trailing(brand(t));

    // One window, one `SplitView`: the feed is this window with the inspector collapsed, so the
    // inspector's slot exists before a story opens and its width can spring out from 0.
    // The overlay layer is built before the panes, because a pane can own a surface in it: the
    // discussion's link chooser hangs off a chip inside the inspector.
    let mut overlays = Vec::new();
    let (main, toolbar, inspector) = match &s.story {
        Some(story) => (
            story_main(story, &k, t, s.pane()),
            Toolbar::new().leading(story_view::story_toolbar(
                t,
                accent,
                s.saved.contains(story.id()),
            )).trailing(brand(t)),
            story_view::inspector_pane(story, &k, t, accent, &mut overlays, reduced,
                window.map_or(360.0, |rect| (rect[3] - 310.0).clamp(230.0, 440.0)),
                [s.scroll_observer(), s.scroll_observer(), s.scroll_observer()]),
        ),
        None => (feed_body(s, &k, t, accent, cols, reduced, contrast, feed_height), toolbar, col()),
    };
    let shell = SplitView::new(main)
        .toolbar(toolbar)
        .inspector(&s.inspector, |s: &mut App| &mut s.inspector, inspector)
        .titlebar_inset(cx.titlebar_inset())
        .pane_motion(pane_motion())
        .reduced_motion(reduced)
        .build(&k);

    let mut base = col().key("window").w_full().h_full().fill(t.bg)
        .on_key(|s: &mut App, ev: &KeyEvent, tx: &mut Tx| {
            if ev.key != rune::Key::Tab || s.story.is_none() || s.settings.is_some() || s.help
                || s.story.as_ref().is_some_and(|story| story.links.is_some()) { return false; }
            s.keyboard_navigation(ev);
            let handled = on_story_key(s, ev, tx);
            if handled {
                let target = if s.story.as_ref().is_some_and(|story| story.source.is_some()) { "source-viewer" } else { "summary-scroll" };
                tx.focus(target, true);
            }
            handled
        })
        .child(shell.grow(1.0).min_h(0.0));
    if let Some(story) = &s.story {
        base = base.child(el().w_full().h(1.0).fill(t.separator)).child(story_view::story_footer(story, &k, t, &s.hints, reduced, contrast));
    }
    if let Some(notice) = &s.toast {
        overlays.push(
            ToastRegion::new()
                .toast(
                    Toast::new("toast-saved", notice.message.clone())
                        .lead(saved_dot("toast-dot", accent))
                        .build(&k),
                )
                .build(&k),
        );
    }
    // The dialogs are the last layer, so they sit over the toast and the split.
    if let Some(settings) = &s.settings {
        settings.view(&k, t, accent, true, &mut overlays);
    }
    if s.help {
        help::view(&k, t, accent, true, &mut overlays);
    }
    rune_kit::root(base, overlays)
        .on_key(|s: &mut App, ev: &KeyEvent, _: &mut Tx| { s.keyboard_navigation(ev); false })
        .on_pointer_move_local(|s: &mut App, _, tx| { s.hints.set(0.3); tx.live_only(); })
        .on_wheel(App::pointer_wheel)
        .pointer_buttons([rune::PointerButton::Back])
        .on_background_press_tx(|s: &mut App, tx: &mut Tx| {
            if tx.button() == rune::PointerButton::Back { s.story_back(tx); }
        })
}

fn brand(t: tok::Theme) -> El {
    row().key("brand").role("group").label("Hacker News").gap(6.0).center_y()
        .child(row().size(24.0, 24.0).center().radius(4.0)
            .fill(rune::color::hex("#ff6600").expect("the brand color is valid"))
            .child(text("HN").font(TextStyle::new(10.0, 700.0)).color(Color::WHITE).a11y_hidden()))
        .child(text("Hacker News").font(TextStyle::new(13.0, 500.0)).color(t.text_secondary).a11y_hidden())
}

fn feed_tabs(s: &App, k: &Kit, t: tok::Theme, accent: Color, reduced: bool) -> El {
    let selected = tab_index(s.feed.tab);
    let indicator = col().key("indicator").h(36.0).grid_cell(selected).radius(7.0).fill(k.c.bg_base)
        .box_shadow(rune::Shadow { spread: 0.5, color: k.c.border_strong, ..Default::default() })
        .motion(if reduced { Spring::NONE } else { Spring::new(0.18, 0.0) }).a11y_hidden()
        .child(row().pin(Some(0.0), None, Some(0.0), Some(3.0)).h(2.0).center()
            .child(el().w(16.0).h(2.0).radius(1.0).fill(accent)));
    let mut tabs = row().key("feed-tabs").role("radiogroup").label("Feed").focus_group()
        .equal_columns(4).h(36.0).center_y().radius(7.0).fill(k.c.layer_01).child(indicator);
    for (i, label) in ["Top", "New", "Best", "Saved"].into_iter().enumerate() {
        tabs = tabs.child(row().key(format!("segment{i}")).grid_cell(i).role("radio").label(label)
            .position_in_set(i + 1, 4).focusable(true).focus_selected(i == selected)
            .focus_ring(2.0, 1.0, accent).h(36.0).px(16.0).center().radius(7.0)
            .hover(k.c.hover).press(k.c.pressed)
            .on_press_tx(move |s: &mut App, tx: &mut Tx| {
                // Pointer selection returns arrows to the feed; an accessibility-focused tab keeps them.
                if !tx.by_keys() && !tx.focus_visible() { tx.blur(); }
                let tab = tab_at(i);
                if tab != s.feed.tab { s.apply(feed_state::switch_tab(&s.feed.state(), tab)); }
            })
            .child(text(label).font(type_::TITLE).color(if i == selected { t.text } else { t.text_secondary }).a11y_hidden()));
    }
    tabs
}

/// The split's motion: `MORPH`'s spring, scaled by the pane's width so a narrow window does not
/// feel twitchy, with the tail fade DESIGN.md asks for.
fn pane_motion() -> PaneMotion {
    PaneMotion::new()
        .spring(Spring::new(0.30, 0.16))
        .scale_with_width(480.0, 0.15, 0.3)
        .fade_tail(0.15)
}

/// The main pane while a story is open.
fn story_main(story: &Story, k: &Kit, t: tok::Theme, pane: Pane<'_>) -> El {
    col()
        .key("story-main")
        .w_full()
        .h_full()
        .child(
            story_view::article_pane(story, k, t, pane)
                .grow(1.0)
                .min_h(0.0),
        )
}

fn feed_body(s: &App, k: &Kit, t: tok::Theme, accent: Color, cols: usize, reduced: bool, contrast: bool, height: f32) -> El {
    let mut body = col().w_full().h_full();
    if let Some(error) = s.saved.error() {
        body = body.child(banner(error, t, accent));
    }
    // A failed ranking or window is a state with a retry, never a blank list.
    if let Some(error) = s.feed.status.error.as_deref() {
        body = body.child(feed_error(error, t, accent));
    }
    body.child(
        feed_list(s, t, accent, cols, reduced, height)
            .on_wheel(App::pointer_wheel)
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .scroll_edge_fade(20.0)
            .grow(1.0)
            .min_h(0.0),
    )
    .child(el().w_full().h(1.0).fill(t.separator))
    .child(footer(s, k, t, accent, reduced, contrast))
}

/// Column-major: story `c * rows + r`, so the left column fills first and `j` walks straight
/// down it. One scroll viewport for both columns; every cell keeps its own key, so
/// `Tx::scroll_into_view` reaches a story whichever column it is in.
fn feed_list(s: &App, t: tok::Theme, accent: Color, cols: usize, reduced: bool, height: f32) -> El {
    let shown = s.feed.stories.len();
    let per_column = shown.div_ceil(cols.max(1));
    // Keep the scroll viewport stable; only a completed page turn remounts this group.
    if s.feed.status.loading && s.feed.stories.is_empty() && s.feed.status.error.is_none() {
        return feed_skeleton(s, t, cols, reduced, height);
    }
    let mut transition = Transition::none();
    if !reduced && s.arrival != Arrival::None {
        let enter = match s.arrival {
            Arrival::Initial => Transition::offset(0.0, 12.0),
            Arrival::Tab(direction) | Arrival::Page(direction) => Transition::offset(direction as f32 * 24.0, 0.0),
            Arrival::None => Transition::none(),
        };
        transition = Transition::asymmetric(enter.combined(Transition::opacity()), Transition::none())
            .springs(SpringPair::new(Spring::new(0.22, 0.0), Spring::NONE));
        transition.insertion.opacity = 0.2;
    }
    let mut list = col()
        .key(format!("feed-page-{}", s.feed_revision))
        .transition(transition)
        .instant_changes()
        .w_full()
        .pt(feed_gutter(height, per_column, cols,
            s.feed.tab != StoryType::Saved && per_column >= s.measured.get().unwrap_or(DEFAULT_GRID).0))
        .pb(FEED_GUTTER)
        .gap(if cols == 2 { 0.0 } else { 2.0 });
    if shown == 0 && s.feed.status.error.is_none() {
        let label = if s.feed.tab == StoryType::Saved { "No saved stories yet" } else { "No stories to show" };
        list = list.px(24.0).role("status").label(label)
            .child(text(label).font(type_::BODY).color(t.text_secondary).a11y_hidden());
        return col().key("feed-list").w_full().child(list);
    }
    for r in 0..per_column {
        let mut line = row().key(format!("line-{r}")).w_full().h(ROW_HEIGHT);
        for c in 0..cols {
            let i = c * per_column + r;
            if i >= shown {
                break;
            }
            if c > 0 {
                // The hairline between columns. Rows sit at gap 0 in two-column mode, so the
                // per-row rules join into one continuous divider.
                line = line.child(el().w(1.0).h_full().fill(t.separator));
            }
            line = line.child(story_row(s, i, t, accent, reduced).grow(1.0));
        }
        list = list.child(line);
    }
    col().key("feed-list").w_full().child(list)
}

fn feed_gutter(height: f32, rows: usize, cols: usize, full: bool) -> f32 {
    let gap = if cols == 2 { 0.0 } else { 2.0 };
    let content = rows as f32 * ROW_HEIGHT + rows.saturating_sub(1) as f32 * gap;
    if full { ((height - content) / 2.0).max(FEED_GUTTER) } else { FEED_GUTTER }
}

fn story_row(s: &App, i: usize, t: tok::Theme, accent: Color, reduced: bool) -> El {
    let story = &s.feed.stories[i];
    let id = story.id;
    let rank = s.feed.display_cursor + i + 1;
    let selected = i == s.feed.selected;
    let failed = s.feed.failed.contains(&id);
    let saved = s.saved.contains(id);
    let in_saved_view = s.feed.tab == StoryType::Saved;

    let mut r = row()
        .key(format!("story-{id}"))
        .motion(if reduced { Spring::NONE } else { Spring::new(0.12, 0.0) })
        .instant_changes()
        .transition(Transition::none())
        .role("button")
        .label(format!("{} \u{00b7} {}", story.title, meta(story, failed)))
        .h_full()
        .px(12.0)
        .gap(14.0)
        .center_y()
        .radius(radius::SM)
        .focusable(true)
        .focus_selected(selected)
        // DESIGN.md's focus affordance is amber, not the system blue. `focus_highlight` paints a
        // solid overlay, which would bury the selected row's card fill, so the ring carries it.
        .focus_ring(1.5, 0.0, accent)
        .on_press(move |s: &mut App| {
            // While a page turn is in flight the rows on screen are the page being left:
            // selecting one would select it under the ranks of the page being loaded.
            if s.feed.status.loading {
                return;
            }
            if let Some(index) = s.feed.stories.iter().position(|story| story.id == id) {
                s.feed.selected = index;
                s.open_id(id);
            }
        })
        .on_pointer_move_local(move |s: &mut App, _, tx| {
            if !s.feed.status.loading && let Some(index) = s.feed.stories.iter().position(|story| story.id == id)
                && s.feed.selected != index {
                s.feed.selected = index;
            } else {
                tx.live_only();
            }
        })
        .on_hover_change(move |s: &mut App, inside: bool, _tx: &mut Tx| {
            if inside {
                s.hover = Some(id);
            } else if s.hover == Some(id) {
                s.hover = None;
            }
        });
    r = r.child(
        el().w(2.0)
            .h(26.0)
            .radius(1.0)
            .fill(if selected { accent } else { Color::TRANSPARENT })
            .motion(if reduced { Spring::NONE } else { Spring::new(0.12, 0.0) })
            .transition(Transition::none()),
    );
    // The saved marker is a dot beside the rank, never right-aligned chrome. The Saved view is
    // already a list of saved stories, so its rows carry no dot.
    let mut lead = row().gap(6.0).center_y().child(
        row().w(18.0).justify_end().child(
            text(format!("{rank}"))
                .key(format!("rank-{rank}"))
                .font(type_::LABEL)
                .color(if selected { accent } else { t.text_secondary }),
        ),
    );
    if !in_saved_view {
        let key = format!("saved-dot-{id}");
        lead = lead.child(if saved {
            saved_dot(key, accent)
        } else {
            el().key(key).size(6.0, 6.0)
        });
    }
    r = r.child(lead).child(
        col()
            .grow(1.0)
            .gap(4.0)
            .min_w(0.0)
            .child(
                text(story.title.clone())
                    .key(format!("title-{id}"))
                    .font(type_::BODY)
                    .color(if failed { t.text_secondary } else { t.text })
                    .truncate(),
            )
            .child(
                text(meta(story, failed))
                    .font(type_::LABEL)
                    .color(t.text_secondary),
            ),
    );
    // The right edge is hover-actions only; nothing persists there.
    if s.hover == Some(id) {
        let save = if in_saved_view { "unsave" } else { "save" };
        r = r.child(act(save, accent, t, move |s: &mut App| s.toggle_saved_id(id)));
    }
    if selected {
        r = r.fill(t.card.mix(accent, 0.08));
    }
    r
}

fn meta(story: &HNCLIItem, failed: bool) -> String {
    if failed {
        return "Could not load this story · press r to retry".to_string();
    }
    // The terminal app's wording, singular included.
    let comments = match story.comments {
        Some(1) => "1 comment".to_string(),
        Some(n) => format!("{n} comments"),
        None => "discuss".to_string(),
    };
    format!(
        "{} points  \u{00b7}  by {}  \u{00b7}  {comments}",
        story.score, story.author
    )
}

/// The saved state, everywhere it appears: a 6 pt amber dot. The word lives in the accessible
/// label, not in the layout.
fn saved_dot(key: impl Into<String>, accent: Color) -> El {
    el()
        .key(key)
        .size(6.0, 6.0)
        .radius(3.0)
        .fill(accent)
        .role("status")
        .label("saved")
}

/// A hover action on a row. It is a control in its own right: a press on it runs its own
/// handler, not the row's select, because Rune dispatches a press to the deepest handler.
fn act(label: &str, color: Color, t: tok::Theme, on_press: impl Fn(&mut App) + 'static) -> El {
    row()
        .role("button")
        .label(label)
        .h(20.0)
        .px(7.0)
        .center()
        .radius(4.0)
        .fill(t.control_off)
        .focusable(true)
        .on_press(on_press)
        .child(text(label).font(type_::LABEL).color(color))
}

fn footer(s: &App, k: &Kit, t: tok::Theme, accent: Color, reduced: bool, contrast: bool) -> El {
    let h = |keys: &[&str], label: &str| passive_hint(hint(k, t, keys, label), &s.hints, reduced, contrast);
    let mut f = row()
        .key("feed-footer")
        .motion(Spring::NONE)
        .instant_changes()
        // Match the 18 pt page controls plus vertical padding while a label replaces them.
        .min_h(42.0)
        .w_full()
        .px(24.0)
        .py(12.0)
        .gap(18.0)
        .center_y()
        .child(h(&["j", "k"], "move"))
        .child(h(&["←", "→"], "column / page"))
        .child(h(
            &["s"],
            if s.feed.tab == StoryType::Saved {
                "unsave"
            } else {
                "save"
            },
        ))
        .child(h(&["n", "p"], "page"))
        .child(h(&["1", "4"], "tabs"))
        .child(el().grow(1.0));
    if !s.feed.status.loading && s.feed.exhausted(s.cells()) {
        f = f.child(
            text("No more stories")
                .font(type_::LABEL)
                .color(t.text_secondary),
        );
    }
    if s.feed.status.loading && s.feed.stories.is_empty() && s.feed.status.error.is_none() {
        f = f.child(text(loading_label(s.feed.tab)).font(type_::LABEL).color(t.text_secondary).a11y_hidden());
    } else if s.feed.status.loading && !s.feed.stories.is_empty() {
        let label = if s.requested.is_some_and(|(_, ranking)| ranking == Ranking::Fresh) {
            "Refreshing…"
        } else {
            "Loading page…"
        };
        f = f.child(row().role("status").label(label).busy(true)
            .child(text(label).font(type_::LABEL).color(t.text_secondary).a11y_hidden()));
    } else if s.feed.tab != StoryType::Saved {
        f = f.child(stepper(s.feed.stepper(), t, accent));
    }
    f
}

fn hint(k: &Kit, t: tok::Theme, keys: &[&str], label: &str) -> El {
    row()
        .gap(6.0)
        .center_y()
        .child(Keybind::new(keys).build(k))
        .child(text(label).font(type_::LABEL).color(t.text_secondary))
}

pub(crate) fn passive_hint(hint: El, hints: &rune::Live<f32>, reduced: bool, contrast: bool) -> El {
    if contrast { hint } else {
        hint.live_opacity(hints.smooth(if reduced { Spring::NONE } else { Spring::new(0.20, 0.0) }))
    }
}

/// `‹ N ›` — the current page only. The HN API reports no total, so there is no "of N" to show.
fn stepper(page: u32, t: tok::Theme, accent: Color) -> El {
    row()
        .key("page-stepper")
        .role("group")
        .label(format!("Page {page}"))
        .gap(2.0)
        .center_y()
        .child(chev("\u{2039}", "Previous page", t, |s: &mut App| {
            let cells = s.cells();
            let action = feed_state::prev_page(&s.feed.state(), cells);
            s.apply(action);
        }))
        .child(
            row()
                .w(20.0)
                .center()
                .child(text(format!("{page}")).font(type_::LABEL).color(accent)),
        )
        .child(chev("\u{203a}", "Next page", t, |s: &mut App| {
            let cells = s.cells();
            let action = feed_state::next_page(&s.feed.state(), cells);
            s.apply(action);
        }))
}

/// One arrow of the page stepper. It is a button, not decoration: a press turns the page.
fn chev(glyph: &str, label: &str, t: tok::Theme, on_press: impl Fn(&mut App) + 'static) -> El {
    row()
        .role("button")
        .label(label)
        .size(18.0, 18.0)
        .center()
        .radius(4.0)
        .fill(t.control_off)
        .focusable(true)
        .on_press(on_press)
        .child(text(glyph).font(type_::LABEL).color(t.text_secondary))
}

/// The corrupt-file banner: the list is not silently empty, and Reload is one press away.
fn banner(error: &str, t: tok::Theme, accent: Color) -> El {
    row()
        .key("saved-banner")
        .role("alert")
        .w_full()
        .px(24.0)
        .py(10.0)
        .gap(12.0)
        .center_y()
        .fill(t.card)
        .child(
            text("Saved stories could not be read")
                .font(type_::LABEL)
                .color(t.text),
        )
        .child(
            text(error.to_string())
                .font(type_::LABEL)
                .color(t.text_secondary)
                .truncate_middle(),
        )
        .child(el().grow(1.0))
        .child(
            row()
                .key("saved-reload")
                .role("button")
                .label("Reload")
                .focusable(true)
                .h(20.0)
                .px(8.0)
                .center()
                .radius(4.0)
                .fill(t.control_off)
                .on_press(|s: &mut App| {
                    s.saved.reload();
                    s.refresh();
                })
                .child(text("Reload").font(type_::LABEL).color(accent)),
        )
}

/// The feed's own failure: the ranking or the window could not be loaded. It is a state with a
/// retry, never a blank list.
fn feed_error(error: &str, t: tok::Theme, accent: Color) -> El {
    row()
        .key("feed-error")
        .role("alert")
        .w_full()
        .px(24.0)
        .py(10.0)
        .gap(12.0)
        .center_y()
        .fill(t.card)
        .child(
            text("Could not load the feed")
                .font(type_::LABEL)
                .color(t.text),
        )
        .child(
            text(error.to_string())
                .font(type_::LABEL)
                .color(t.text_secondary)
                .truncate_middle(),
        )
        .child(el().grow(1.0))
        .child(
            row()
                .key("feed-retry")
                .role("button")
                .label("Retry")
                .focusable(true)
                .h(20.0)
                .px(8.0)
                .center()
                .radius(4.0)
                .fill(t.control_off)
                .on_press(|s: &mut App| s.refresh())
                .child(text("Retry").font(type_::LABEL).color(accent)),
        )
}

/// Decorative rows occupy the same viewport and lanes as the feed, with one status owner.
fn feed_skeleton(s: &App, t: tok::Theme, cols: usize, reduced: bool, height: f32) -> El {
    let rows = s.measured.get().unwrap_or(DEFAULT_GRID).0;
    let transition = if reduced { Transition::none() } else {
        let mut transition = Transition::asymmetric(Transition::opacity(), Transition::opacity())
            .springs(SpringPair::new(Spring::new(0.20, 0.0), Spring::new(0.12, 0.0)));
        transition.insertion.opacity = 0.35;
        transition
    };
    let mut skeleton = col().key("feed-placeholder").transition(transition).instant_changes().motion(Spring::NONE).w_full()
        .pt(feed_gutter(height, rows, cols, true)).pb(FEED_GUTTER)
        .gap(if cols == 2 { 0.0 } else { 2.0 }).a11y_hidden();
    for r in 0..rows {
        let mut line = row().w_full().h(ROW_HEIGHT);
        for c in 0..cols {
            if c > 0 { line = line.child(el().w(1.0).h_full().fill(t.separator)); }
            let width = [0.72, 0.88, 0.62, 0.81][(r + c) % 4];
            line = line.child(row().grow(1.0).h_full().px(12.0).gap(14.0).center_y()
                .child(el().w(2.0).h(26.0))
                .child(el().w(18.0).h(12.0).radius(3.0).fill(t.control_off))
                .child(col().grow(1.0).gap(8.0)
                    .child(el().w_frac(width).h(12.0).radius(3.0).fill(t.control_off))
                    .child(el().w_frac(width * 0.56).h(9.0).radius(3.0).fill(t.control_off))));
        }
        skeleton = skeleton.child(line);
    }
    col().key("feed-list").w_full().role("status").label(loading_label(s.feed.tab)).busy(true).child(skeleton)
}

fn loading_label(tab: StoryType) -> String {
    format!("Loading {} stories", match tab { StoryType::Top => "Top", StoryType::New => "New", StoryType::Best => "Best", StoryType::Saved => "Saved" })
}

fn tab_index(tab: StoryType) -> usize {
    match tab {
        StoryType::Top => 0,
        StoryType::New => 1,
        StoryType::Best => 2,
        StoryType::Saved => 3,
    }
}

fn tab_at(index: usize) -> StoryType {
    match index {
        1 => StoryType::New,
        2 => StoryType::Best,
        3 => StoryType::Saved,
        _ => StoryType::Top,
    }
}

fn scene_feed(_s: &mut App) {}

fn scene_feed_saved(s: &mut App) {
    s.toast = Some(Notice {
        message: "Saved · Press 4 for Saved".to_string(),
        until: rune::scene_clock().now + TOAST_SECONDS,
    });
}

fn scene_saved_view(s: &mut App) {
    s.feed.tab = StoryType::Saved;
    // The hover action is the only way to unsave from this view, so the scene shows it.
    s.hover = s.saved.ids().first().copied();
}

fn scene_saved_corrupt(s: &mut App) {
    s.saved = SavedView::corrupt(
        std::path::PathBuf::from("~/.local/share/hn/saved.json"),
        "expected value at line 1 column 2",
    );
    s.feed.tab = StoryType::Saved;
}

fn scene_resize(_s: &mut App) {}

/// The story split, settled: the article, the summary card and the discussion.
fn scene_story(s: &mut App) {
    s.open_story(crate::fixtures::item(
        crate::fixtures::STORY_INDEX,
        crate::fixtures::STORY_ID,
    ));
    if let Some(story) = s.story.as_mut() {
        story.thread = crate::comments::Thread::from_comments(&crate::fixtures::thread());
    }
}

/// The discussion as it opens: the tree at the story's top level, the cursor on the first
/// comment, no re-root, with focus mode on.
fn scene_comments(s: &mut App) {
    scene_story(s);
}

/// A thread you did not start: re-rooted at the comment that hit the depth cap, the cursor on
/// its first reply. The trail carries the four ancestors above it.
fn scene_thread(s: &mut App) {
    scene_story(s);
    if let Some(story) = &mut s.story {
        story.thread.reroot(901000);
        story.thread.select(9010000);
    }
}

/// The citation source viewer: the first citation has replies to inspect.
fn scene_citation(s: &mut App) {
    scene_story(s);
    if let Some(story) = &mut s.story {
        story.next_citation();
        story.open_source();
    }
}

/// Generating: the article and the discussion have landed, the summary has not.
fn scene_summary_generating(s: &mut App) {
    scene_story(s);
    if let Some(story) = &mut s.story {
        story.summary = SummaryState::Generating {
            previous: None,
        };
    }
}

/// The summary settings dialog, open on a draft that already holds a key. The key is a fixture
/// and the field is secure, so the golden proves the mask rather than the value: the string
/// appears nowhere in the tree.
fn scene_settings(s: &mut App) {
    let mut config = Config::default();
    config.api_key = "sk-or-v1-EXAMPLEKEYNOTREAL".to_string();
    s.settings = Some(Settings::open(&config, None, false));
}

/// The keyboard sheet, open: two columns of the key map, generated from `keys::HELP`.
fn scene_help(s: &mut App) {
    s.help = true;
}

/// Run the window.
///
/// `on_window` is where the live pane's child view gets its platform handle: Rune lends it for
/// the duration of the callback, and `AppHandle::update` carries it to the UI thread's state,
/// where `poll` builds the child once the pane has a rect too. A recreated window invalidates
/// the old child, so it is dropped here and rebuilt by the next `poll`.
///
/// Two launch flags exist for the GUI proof in `scripts/verify-live.sh`, because this machine
/// denies osascript assistive access and nothing outside the process can press a key or raise
/// its window:
///
/// - `--open-story <id>` opens that story once the feed holds it.
/// - `--on-top` keeps the window above other apps, so a fullscreen capture is of this window.
pub fn run() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let _ = OPEN_STORY.set(flag_value(&argv, "--open-story").and_then(|id| id.parse().ok()));
    let on_top = argv.iter().any(|arg| arg == "--on-top");
    let mut app = rune::App::new(spec());
    if on_top {
        app = app.window_options(rune::WindowOptions {
            always_on_top: true,
            ..Default::default()
        });
    }
    let handle = app.handle();
    app.on_window(move |window| {
        webview::close();
        let window = window.clone();
        handle.update(move |s: &mut App, _| {
            s.native_window = Some(window);
            s.live = Live::Idle;
            s.live_bounds = None;
            s.live_shown = false;
        });
    })
    .on_close(close)
    .run();
}

/// The value of `--flag value`, if the flag is there.
fn flag_value(argv: &[String], flag: &str) -> Option<String> {
    let index = argv.iter().position(|arg| arg == flag)?;
    argv.get(index + 1).cloned()
}

/// The window is going away: drop the child view on the thread that made it.
fn close(_s: &mut App, _tx: &mut Tx) {
    webview::close();
}

pub fn spec() -> AppSpec<App> {
    AppSpec {
        scenes: &[
            ("feed", scene_feed),
            ("feed-wide", scene_feed),
            ("feed-saved", scene_feed_saved),
            ("feed-saved-view", scene_saved_view),
            ("feed-saved-corrupt", scene_saved_corrupt),
            ("resize", scene_resize),
            ("story", scene_story),
            ("comments", scene_comments),
            ("thread", scene_thread),
            ("citation", scene_citation),
            ("summary-generating", scene_summary_generating),
            ("settings", scene_settings),
            ("help", scene_help),
        ],
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(on_key),
        ..AppSpec::new("Hacker News", (1180.0, 760.0), init, view)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(s: &mut App, key: rune::Key) {
        on_key(
            s,
            &KeyEvent {
                key,
                mods: rune::Mods::default(),
            },
            &mut Tx::default(),
        );
    }

    fn poll_until(s: &mut App, ready: impl Fn(&App) -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while !ready(s) {
            poll(s, 0.0, &mut Tx::default());
            assert!(std::time::Instant::now() < deadline, "story phase did not arrive");
            std::thread::yield_now();
        }
    }

    #[test]
    fn ready_comments_are_not_blocked_by_two_superseded_stories() {
        use std::collections::HashMap;
        use std::sync::Mutex;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::sync::oneshot;

        let (article_one_tx, article_one_rx) = oneshot::channel::<()>();
        let (article_two_tx, article_two_rx) = oneshot::channel::<()>();
        let (summary_one_tx, summary_one_rx) = oneshot::channel::<()>();
        let (summary_two_tx, summary_two_rx) = oneshot::channel::<()>();
        let article_gates = Mutex::new(HashMap::from([(1, article_one_rx), (2, article_two_rx)]));
        let summary_gates = Mutex::new(HashMap::from([(1, summary_one_rx), (2, summary_two_rx)]));
        let articles_started = Arc::new(AtomicUsize::new(0));
        let summaries_started = Arc::new(AtomicUsize::new(0));
        let article_starts = articles_started.clone();
        let summary_starts = summaries_started.clone();
        let mut fake = crate::service::FakeService::fixture();
        fake.article = Arc::new(move |id| {
            let gate = article_gates.lock().unwrap().remove(&id);
            if gate.is_some() { article_starts.fetch_or(1 << (id - 1), Ordering::SeqCst); }
            Box::pin(async move {
                if let Some(gate) = gate { let _ = gate.await; }
                Ok((crate::fixtures::ARTICLE.to_string(), false))
            })
        });
        fake.summary = Arc::new(move |id| {
            let gate = summary_gates.lock().unwrap().remove(&id);
            if gate.is_some() { summary_starts.fetch_or(1 << (id - 1), Ordering::SeqCst); }
            Box::pin(async move {
                if let Some(gate) = gate { let _ = gate.await; }
                Ok(crate::fixtures::summary())
            })
        });
        let mut s = app(false, SavedView::in_memory());
        s.source = Arc::new(Source::Fake(fake));
        s.config.api_key = "fixture".into();
        for id in [1, 2] {
            s.open_story(crate::fixtures::item((id - 1) as usize, id));
            let mask = (1 << id) - 1;
            poll_until(&mut s, |s| s.story.as_ref().unwrap().discussion == Some(Ok(()))
                && articles_started.load(Ordering::SeqCst) & mask == mask
                && summaries_started.load(Ordering::SeqCst) & mask == mask);
        }

        // Both old articles and both old summaries are held while story 3 starts.
        s.open_story(crate::fixtures::item(2, 3));
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while s.story.as_ref().unwrap().discussion.is_none() && std::time::Instant::now() < deadline {
            poll(&mut s, 0.0, &mut Tx::default());
            std::thread::sleep(Duration::from_millis(1));
        }
        let roots_arrived_while_old_work_was_held = s.story.as_ref().unwrap().discussion == Some(Ok(()));
        // Release one stale result and show that the third discussion itself succeeds.
        assert!(summary_one_tx.send(()).is_ok());
        poll_until(&mut s, |s| s.story.as_ref().unwrap().discussion == Some(Ok(())));
        assert_eq!(s.story.as_ref().unwrap().id(), 3);
        let _ = article_one_tx.send(());
        let _ = article_two_tx.send(());
        let _ = summary_two_tx.send(());
        s.bridge.cancel();
        assert!(roots_arrived_while_old_work_was_held,
            "ready comments waited for a superseded article or summary to release a Latest worker");
    }

    #[test]
    fn story_phases_show_comments_while_work_is_pending_and_reject_stale_results() {
        use crate::bridge::SummaryError;
        use crate::story::ArticleState;
        use std::collections::VecDeque;
        use std::sync::{Mutex, mpsc};
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::sync::oneshot;

        let (article_tx, article_rx) = oneshot::channel();
        let article_rx = Arc::new(Mutex::new(Some(article_rx)));
        let (other_article_tx, other_article_rx) = oneshot::channel();
        let other_article_rx = Arc::new(Mutex::new(Some(other_article_rx)));
        let (failure_tx, failure_rx) = oneshot::channel();
        let (old_retry_tx, old_retry_rx) = oneshot::channel();
        let (old_story_tx, old_story_rx) = oneshot::channel();
        let summary_gates = Arc::new(Mutex::new(VecDeque::from([
            Some(failure_rx), Some(old_retry_rx), None, Some(old_story_rx), None,
        ])));
        let summary_started = Arc::new(AtomicUsize::new(0));
        let started = summary_started.clone();
        let (finished_tx, finished_rx) = mpsc::channel();
        let mut fake = crate::service::FakeService::fixture();
        fake.article = Arc::new(move |story_id| {
            let gate = if story_id == 2 { other_article_rx.lock().unwrap().take() }
                else { article_rx.lock().unwrap().take() };
            Box::pin(async move {
                match gate {
                    Some(gate) => gate.await.expect("article gate released"),
                    None => Ok((crate::fixtures::ARTICLE.to_string(), false)),
                }
            })
        });
        fake.summary = Arc::new(move |story_id| {
            let gate = summary_gates.lock().unwrap().pop_front().expect("expected summary request");
            started.fetch_add(1, Ordering::SeqCst);
            let finished = finished_tx.clone();
            Box::pin(async move {
                match gate {
                    Some(gate) => {
                        let result = gate.await.expect("summary gate released");
                        finished.send(story_id).unwrap();
                        result
                    }
                    None => {
                        let mut result = crate::fixtures::summary();
                        result.overview = format!("current summary {story_id}");
                        Ok(result)
                    }
                }
            })
        });
        let mut s = app(false, SavedView::in_memory());
        s.source = Arc::new(Source::Fake(fake));
        s.config.api_key = "fixture".into();
        s.open_story(crate::fixtures::item(0, 1));
        poll_until(&mut s, |s| {
            s.story.as_ref().is_some_and(|story| story.discussion.as_ref().is_some_and(Result::is_ok))
                && summary_started.load(Ordering::SeqCst) == 1
        });
        let story = s.story.as_ref().unwrap();
        assert_eq!(story.article, ArticleState::Loading);
        assert!(matches!(story.summary, SummaryState::Generating { .. }));
        assert!(story.thread.node(901).is_some(), "roots arrive before article and summary");

        // Mount the actual inspector while both gated requests are still pending.
        s.live = Live::Failed { story_id: 1, error: "fixture".into() };
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.frame(0.4);
        assert!(rt.elements().iter().any(|e| e.key.as_deref() == Some("comment-901")));
        rt.update(|s, _| s.activate_comment(901));
        poll_until(&mut rt.state, |s| s.story.as_ref().unwrap().thread.children_of(901).len() == 2);

        assert!(article_tx.send(Ok(("extracted article".to_string(), false))).is_ok());
        poll_until(&mut rt.state, |s| matches!(s.story.as_ref().unwrap().article, ArticleState::Ready { .. }));
        assert!(matches!(rt.state.story.as_ref().unwrap().summary, SummaryState::Generating { .. }));
        assert!(failure_tx.send(Err(SummaryError::new("provider failed", false))).is_ok());
        poll_until(&mut rt.state, |s| s.story.as_ref().unwrap().summary.is_failed());
        let story = rt.state.story.as_ref().unwrap();
        assert!(matches!(&story.article, ArticleState::Ready { text, .. } if text == "extracted article"));
        assert_eq!(story.thread.children_of(901).len(), 2, "summary failure keeps loaded replies");

        // Two retries for the same story: the earlier provider finishes after the newer one.
        rt.state.retry_summary();
        poll_until(&mut rt.state, |_| summary_started.load(Ordering::SeqCst) == 2);
        rt.state.retry_summary();
        poll_until(&mut rt.state, |s| matches!(&s.story.as_ref().unwrap().summary,
            SummaryState::Done(summary) if summary.overview == "current summary 1"));
        assert!(old_retry_tx.send(Err(SummaryError::new("obsolete failure", true))).is_ok());
        assert_eq!(finished_rx.recv_timeout(Duration::from_secs(3)).unwrap(), 1); // first failure
        assert_eq!(finished_rx.recv_timeout(Duration::from_secs(3)).unwrap(), 1); // old retry
        std::thread::sleep(Duration::from_millis(20));
        poll(&mut rt.state, 0.0, &mut Tx::default());
        assert!(matches!(&rt.state.story.as_ref().unwrap().summary,
            SummaryState::Done(summary) if summary.overview == "current summary 1"));

        // A different story also wins over the older story's pending provider result.
        rt.state.retry_summary();
        poll_until(&mut rt.state, |_| summary_started.load(Ordering::SeqCst) == 4);
        rt.state.open_story(crate::fixtures::item(1, 2));
        poll_until(&mut rt.state, |s| matches!(&s.story.as_ref().unwrap().summary,
            SummaryState::Done(summary) if summary.overview == "current summary 2"));
        assert!(matches!(rt.state.story.as_ref().unwrap().article, ArticleState::Loading));
        assert!(matches!(&rt.state.story.as_ref().unwrap().summary,
            SummaryState::Done(_)));
        assert!(other_article_tx.send(Ok(("second article".to_string(), false))).is_ok());
        poll_until(&mut rt.state, |s| matches!(&s.story.as_ref().unwrap().article,
            ArticleState::Ready { .. }));
        let mut stale = crate::fixtures::summary();
        stale.overview = "obsolete story".into();
        assert!(old_story_tx.send(Ok(stale)).is_ok());
        assert_eq!(finished_rx.recv_timeout(Duration::from_secs(3)).unwrap(), 1);
        std::thread::sleep(Duration::from_millis(20));
        poll(&mut rt.state, 0.0, &mut Tx::default());
        let story = rt.state.story.as_ref().unwrap();
        assert_eq!(story.id(), 2);
        assert!(matches!(&story.summary, SummaryState::Done(summary) if summary.overview == "current summary 2"));
        assert!(story.thread.node(901).is_some());
    }

    #[test]
    fn desktop_arrows_follow_rendered_columns_and_clamp_short_columns() {
        let mut s = app(true, SavedView::in_memory());
        s.measured.set(Some((14, 2)));
        s.request((14, 2), Ranking::Fresh);
        for shown in [28, 27, 11, 3] {
            s.feed.stories.truncate(shown);
            let per_column = shown.div_ceil(2);
            for selected in 0..per_column {
                s.feed.selected = selected;
                press(&mut s, rune::Key::Right);
                let right = if selected < per_column && shown > per_column {
                    (selected + per_column).min(shown - 1)
                } else {
                    selected
                };
                assert_eq!(s.feed.selected, right, "right from {selected} of {shown}");
                press(&mut s, rune::Key::Left);
                assert_eq!(
                    s.feed.selected,
                    if right >= per_column {
                        right - per_column
                    } else {
                        right
                    }
                );
                assert_eq!(s.feed.display_cursor, 0);
                assert_eq!(s.feed_revision, 0);
            }
        }
    }

    #[test]
    fn desktop_column_loading_empty_and_modifiers_do_not_page() {
        let mut s = app(true, SavedView::in_memory());
        s.request((11, 1), Ranking::Fresh);
        s.feed.selected = 5;
        press(&mut s, rune::Key::Left);
        assert_eq!(s.feed.selected, 5);
        s.measured.set(Some((11, 2)));
        on_key(
            &mut s,
            &KeyEvent {
                key: rune::Key::Right,
                mods: rune::Mods {
                    ctrl: true,
                    ..Default::default()
                },
            },
            &mut Tx::default(),
        );
        assert_eq!(s.feed.selected, 5);
        s.feed.status.loading = true;
        press(&mut s, rune::Key::Right);
        assert_eq!(s.feed.selected, 5);
        s.feed.status.loading = false;
        s.feed.stories.clear();
        press(&mut s, rune::Key::Right);
        assert_eq!(s.feed.display_cursor, 0);
        assert_eq!(s.feed_revision, 0);
    }

    #[test]
    fn edge_arrows_turn_pages_and_land_at_the_matching_side() {
        for cols in [1, 2] {
            let mut s = app(true, SavedView::in_memory());
            s.measured.set(Some((5, cols)));
            s.request((5, cols), Ranking::Fresh);
            s.feed.selected = s.feed.stories.len() - 1;
            press(&mut s, rune::Key::Right);
            assert_eq!(s.feed.display_cursor, 5 * cols);
            assert_eq!(s.feed.selected, 0);
            assert_eq!(s.arrival, Arrival::Page(1));
            press(&mut s, rune::Key::Left);
            assert_eq!(s.feed.display_cursor, 0);
            assert_eq!(s.feed.selected, 5 * cols - 1);
            assert_eq!(s.arrival, Arrival::Page(-1));
        }
    }

    #[test]
    fn arrow_paging_stops_at_known_ends_and_saved() {
        let mut s = app(true, SavedView::in_memory());
        s.measured.set(Some((5, 2)));
        s.request((5, 2), Ranking::Fresh);
        press(&mut s, rune::Key::Left);
        assert_eq!(s.feed.display_cursor, 0);
        for _ in 0..2 {
            s.feed.selected = s.feed.stories.len() - 1;
            press(&mut s, rune::Key::Right);
        }
        assert_eq!(s.feed.display_cursor, 20);
        s.feed.selected = 9;
        press(&mut s, rune::Key::Right);
        assert_eq!(s.feed.display_cursor, 20, "exact snapshot end stays put");
        press(&mut s, rune::Key::Text("4".into()));
        press(&mut s, rune::Key::Right);
        assert_eq!(s.feed.display_cursor, 0, "Saved never pages");
    }

    #[test]
    fn fully_cached_windows_skip_source_requests_but_refresh_and_partial_windows_fetch() {
        let mut s = app(true, SavedView::in_memory());
        s.request((11, 1), Ranking::Fresh);
        press(&mut s, rune::Key::Text("n".into()));
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        let before = *fake.requests.lock().unwrap();
        press(&mut s, rune::Key::Text("p".into()));
        assert!(!s.feed.status.loading);
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        assert_eq!(*fake.requests.lock().unwrap(), before, "cached return issues zero requests");
        let mut updated = s.feed.stories[0].clone();
        updated.title = "Updated by refresh".into();
        fake.insert_item(updated);
        s.refresh();
        assert_eq!(s.feed.stories[0].title, "Updated by refresh");
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        let refreshed = *fake.requests.lock().unwrap();
        assert!(refreshed.0 > before.0 && refreshed.1 > before.1);
        s.feed.story_cache.remove(&12);
        press(&mut s, rune::Key::Text("n".into()));
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        assert!(fake.requests.lock().unwrap().1 > refreshed.1, "partial cache still fetches");
        s.feed.story_cache.remove(&12);
        fake.fail(12);
        s.request((11, 1), Ranking::Keep);
        assert!(s.feed.failed.contains(&12));
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        let failed = fake.requests.lock().unwrap().1;
        s.request((11, 1), Ranking::Keep);
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        assert!(fake.requests.lock().unwrap().1 > failed, "failure placeholders are not cache hits");
    }

    #[test]
    fn a_cached_window_cancels_only_the_superseded_feed_worker() {
        let mut s = app(true, SavedView::in_memory());
        s.request((11, 1), Ranking::Fresh);
        s.source = Arc::new(Source::Fake(crate::service::FakeService::fixture()
            .with_delay(Duration::from_millis(50))));
        s.headless = false;
        s.request((11, 1), Ranking::Fresh);
        s.open_selected();
        assert!(s.bridge.is_pending());
        assert!(s.bridge.is_story_pending());
        s.request((11, 1), Ranking::Keep);
        assert!(!s.feed.status.loading);
        assert!(!s.bridge.is_pending(), "a held window supersedes the feed worker");
        assert!(s.bridge.is_story_pending(), "feed cache must not cancel the independent story request");
        let title = s.feed.stories[0].title.clone();
        std::thread::sleep(Duration::from_millis(80));
        poll(&mut s, 0.0, &mut Tx::default());
        assert_eq!(s.feed.stories[0].title, title);
        assert!(!s.feed.status.loading);
    }

    #[test]
    fn explicit_paging_replaces_the_group_but_selection_and_refresh_do_not() {
        let mut s = app(true, SavedView::in_memory());
        s.request((11, 1), Ranking::Fresh);
        press(&mut s, rune::Key::Down);
        assert_eq!(s.feed_revision, 0);
        for key in [
            rune::Key::Text("n".into()),
            rune::Key::Text("p".into()),
            rune::Key::Text(" ".into()),
            rune::Key::PageUp,
            rune::Key::PageDown,
        ] {
            let before = s.feed.display_cursor;
            let revision = s.feed_revision;
            press(&mut s, key);
            assert_ne!(s.feed.display_cursor, before);
            assert_eq!(s.feed_revision, revision + 1);
        }
        let revision = s.feed_revision;
        s.refresh();
        assert_eq!(s.feed_revision, revision);
        assert!(s.pending_arrival.is_none());
        s.open_selected();
        assert_eq!(s.feed_revision, 0, "returning from a story must not replay a page fade");
    }

    #[test]
    fn asynchronous_page_arrival_consumes_the_effect_and_refresh_cancels_it() {
        let mut s = app(true, SavedView::in_memory());
        s.request((11, 1), Ranking::Fresh);
        s.headless = false;
        // Keep file/window restoration outside this feed-only test.
        s.restored = true;
        press(&mut s, rune::Key::Text("n".into()));
        assert!(s.pending_arrival.is_some());
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while s.feed.status.loading {
            poll(&mut s, 0.0, &mut Tx::default());
            assert!(std::time::Instant::now() < deadline, "fake fill must land");
            std::thread::yield_now();
        }
        assert_eq!(s.feed.display_cursor, DEFAULT_GRID.0);
        assert_eq!(s.feed_revision, 1);
        assert!(s.pending_arrival.is_none());
        poll(&mut s, 0.0, &mut Tx::default());
        assert_eq!(s.feed_revision, 1);
        press(&mut s, rune::Key::Text("n".into()));
        assert!(s.pending_arrival.is_some());
        press(&mut s, rune::Key::Text("p".into()));
        assert_eq!(s.pending_arrival, Some(Arrival::Page(1)), "rejected back input must not flip the pending forward arrival");
        s.refresh();
        assert!(!s.pending_arrival.is_some(), "refresh supersedes the page effect");
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while s.feed.status.loading {
            poll(&mut s, 0.0, &mut Tx::default());
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(s.feed_revision, 1, "superseding refresh stays instant");
    }

    #[test]
    fn opening_a_story_during_a_page_request_does_not_rearm_the_feed_fade() {
        let mut s = app(true, SavedView::in_memory());
        s.request((11, 1), Ranking::Fresh);
        s.headless = false;
        press(&mut s, rune::Key::Text("n".into()));
        assert!(s.pending_arrival.is_some());
        press(&mut s, rune::Key::Enter);
        assert!(s.story.is_some(), "Enter opens before the page fill is polled");
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while s.feed.status.loading {
            poll(&mut s, 0.0, &mut Tx::default());
            assert!(std::time::Instant::now() < deadline, "fake fill must land");
            std::thread::yield_now();
        }
        assert_eq!(s.feed.display_cursor, DEFAULT_GRID.0, "the page fill still lands");
        press(&mut s, rune::Key::Escape);
        assert!(s.story.is_none());
        assert_eq!(s.feed_revision, 0, "returning to the feed must not replay the fade");
        assert!(s.pending_arrival.is_none());
    }

    #[test]
    fn loading_shell_has_one_busy_accessible_status_and_no_fake_story_controls() {
        let mut s = app(true, SavedView::in_memory());
        s.feed.status.loading = true;
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.frame(0.0);
        let tree = rt.accessibility_tree();
        assert_eq!(tree.nodes.iter().filter(|(_, node)| node.is_busy()).count(), 1);
        assert!(rt.elements().iter().all(|e| !e.key.as_deref().is_some_and(|key| key.starts_with("story-"))));
        let viewport = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("feed-list")).unwrap().rect;
        rt.update(|s, tx| { s.request((11, 1), Ranking::Fresh); tx.instant(); });
        rt.frame(0.0);
        let loaded = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("feed-list")).unwrap().rect;
        assert_eq!(loaded, viewport, "loading and loaded content keep the same viewport");
    }

    #[test]
    fn switching_tabs_fades_the_loading_shell_and_reduced_motion_lands_it() {
        for reduced in [false, true] {
            let mut s = app(true, SavedView::in_memory());
            s.request((11, 1), Ranking::Fresh);
            s.headless = false;
            s.source = Arc::new(Source::Fake(crate::service::FakeService::fixture()
                .with_delay(Duration::from_millis(100))));
            let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
            rt.set_system_reduced_motion(reduced);
            rt.on_key = Some(on_key);
            rt.frame(0.0);
            rt.key(KeyEvent { key: rune::Key::Text("2".into()), mods: Default::default() });
            rt.frame(0.0);
            let opacity = |rt: &rune::Runtime<App>| rt.elements().into_iter()
                .find(|e| e.key.as_deref() == Some("feed-placeholder") && !e.exiting).unwrap().opacity;
            let start = opacity(&rt);
            rt.frame(0.05);
            let at_50 = opacity(&rt);
            rt.frame(0.05);
            let at_100 = opacity(&rt);
            if reduced { assert_eq!((start, at_50, at_100), (1.0, 1.0, 1.0)); }
            else { assert!(start < at_50 && at_50 < at_100 && at_100 < 1.0,
                "the placeholder must still be entering: {start}, {at_50}, {at_100}"); }
            rt.key(KeyEvent { key: rune::Key::Text("1".into()), mods: Default::default() });
            rt.frame(0.0);
            assert!(opacity(&rt) >= at_100, "a rapid reversal keeps the visible placeholder");
            assert_eq!(rt.state.feed.tab, StoryType::Top);
            rt.frame(0.5);
            assert_eq!(opacity(&rt), 1.0);
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while rt.state.bridge.is_pending() { assert!(std::time::Instant::now() < deadline); std::thread::yield_now(); }
            rt.poll = Some(poll);
            while rt.state.feed.status.loading { rt.frame(0.0); assert!(std::time::Instant::now() < deadline); }
            assert_eq!(rt.state.feed.stories[0].id, 1, "only the latest tab's response is shown");
        }
    }

    #[test]
    fn tab_response_can_land_before_or_during_the_loading_entrance() {
        for before_frame in [true, false] {
            let mut s = app(true, SavedView::in_memory());
            s.request((11, 1), Ranking::Fresh);
            s.headless = false;
            let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
            rt.on_key = Some(on_key);
            rt.frame(0.0);
            let key = KeyEvent { key: rune::Key::Text("2".into()), mods: Default::default() };
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            if before_frame {
                rt.update(|s, tx| {
                    on_key(s, &key, tx);
                    while s.bridge.is_pending() { assert!(std::time::Instant::now() < deadline); std::thread::yield_now(); }
                    poll(s, 0.0, tx);
                });
            } else {
                rt.key(key);
                rt.frame(0.05);
                assert!(rt.elements().iter().any(|e| e.key.as_deref() == Some("feed-placeholder") && e.opacity < 1.0));
                while rt.state.bridge.is_pending() { assert!(std::time::Instant::now() < deadline); std::thread::yield_now(); }
                rt.poll = Some(poll);
                while rt.state.feed.status.loading { rt.frame(0.0); assert!(std::time::Instant::now() < deadline); }
            }
            rt.frame(0.05);
            assert_eq!(rt.state.feed.stories[0].id, 101);
            assert!(rt.elements().iter().any(|e| e.key.as_deref() == Some("feed-page-1") && !e.exiting));
            assert!(!rt.elements().iter().any(|e| e.key.as_deref() == Some("feed-placeholder") && !e.exiting));
        }
    }

    #[test]
    fn loading_shell_entrance_survives_a_new_keyed_parent() {
        fn stage(s: &App, cx: &mut Cx<App>) -> El {
            let child = if s.feed.status.loading {
                col().key("new-parent").w_full().child(feed_skeleton(s, tok::theme(cx.system_dark()), 1, false, 760.0))
            } else { col().key("old-parent").w_full() };
            col().w_full().h_full().child(child)
        }
        let mut rt = rune::Runtime::new(app(true, SavedView::in_memory()), stage, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.frame(0.0);
        rt.update(|s, tx| { s.feed.status.loading = true; tx.animated(); });
        rt.frame(0.05);
        let opacity = |rt: &rune::Runtime<App>| rt.elements().into_iter()
            .find(|e| e.key.as_deref() == Some("feed-placeholder") && !e.exiting).unwrap().opacity;
        let at_50 = opacity(&rt);
        rt.frame(0.05);
        let at_100 = opacity(&rt);
        assert!(at_50 < at_100 && at_100 < 1.0, "new parent must carry the entrance: {at_50}, {at_100}");
    }

    #[test]
    fn retained_feed_rows_show_one_busy_status_until_settled_or_timed_out() {
        let mut s = app(true, SavedView::in_memory());
        s.measured.set(Some((11, 1)));
        s.request((11, 1), Ranking::Fresh);
        s.source = Arc::new(Source::Fake(crate::service::FakeService::fixture()
            .with_delay(Duration::from_millis(200))));
        s.headless = false;
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.frame(0.0);
        let before = rt.elements();
        let footer = before.iter().find(|e| e.key.as_deref() == Some("feed-footer")).unwrap().rect;
        let first = before.iter().find(|e| e.key.as_deref() == Some("story-1")).unwrap().rect;
        rt.update(|s, _| s.refresh());
        rt.frame(0.0);
        let pending = rt.elements();
        assert_eq!(pending.iter().find(|e| e.key.as_deref() == Some("feed-footer")).unwrap().rect, footer);
        assert_eq!(pending.iter().find(|e| e.key.as_deref() == Some("story-1")).unwrap().rect, first);
        assert!(pending.iter().any(|e| e.label.as_deref() == Some("Refreshing…")));
        assert_eq!(rt.accessibility_tree().nodes.iter().filter(|(_, node)| node.is_busy()).count(), 1);
        press(&mut rt.state, rune::Key::Text("n".into()));
        assert_eq!(rt.state.feed.display_cursor, 0, "repeated paging is refused during refresh");
        poll(&mut rt.state, 0.0, &mut Tx::default());
        poll(&mut rt.state, LOAD_TIMEOUT, &mut Tx::default());
        assert!(!rt.state.bridge.is_pending(), "timeout cancels the obsolete response");
        assert!(rt.state.feed.status.error.is_some());
        rt.update(|_, tx| tx.instant());
        rt.frame(0.0);
        assert!(!rt.accessibility_tree().nodes.iter().any(|(_, node)| node.is_busy()));

        // A failed refresh settles with the same held rows and no busy status.
        let failing = crate::service::FakeService::fixture();
        failing.fail_ranking(StoryType::Top);
        rt.state.source = Arc::new(Source::Fake(failing));
        rt.state.refresh();
        poll_until(&mut rt.state, |s| !s.feed.status.loading);
        assert!(rt.state.feed.status.error.is_some());
        assert_eq!(rt.state.feed.stories[0].id, 1);

        rt.state.source = Arc::new(Source::Fake(crate::service::FakeService::fixture()
            .with_delay(Duration::from_millis(50))));
        rt.state.refresh();
        poll_until(&mut rt.state, |s| !s.feed.status.loading);
        assert!(rt.state.feed.status.error.is_none());
        press(&mut rt.state, rune::Key::Text("n".into()));
        rt.update(|_, tx| tx.instant());
        rt.frame(0.0);
        assert!(rt.elements().iter().any(|e| e.label.as_deref() == Some("Loading page…")));
        assert_eq!(rt.accessibility_tree().nodes.iter().filter(|(_, node)| node.is_busy()).count(), 1);
        assert_eq!(rt.state.feed.stories[0].id, 1);
        poll_until(&mut rt.state, |s| !s.feed.status.loading);
        assert_eq!(rt.state.feed.stories[0].id, 1 + DEFAULT_GRID.0 as i32);
        press(&mut rt.state, rune::Key::Text("p".into()));
        assert!(!rt.state.feed.status.loading, "cached return settles before a frame can show busy");
        rt.update(|_, tx| tx.instant());
        rt.frame(0.0);
        assert!(!rt.accessibility_tree().nodes.iter().any(|(_, node)| node.is_busy()));
    }

    #[test]
    fn system_reduced_motion_lands_saved_tab_and_returns_keys_to_the_feed() {
        let mut s = app(true, SavedView::in_memory());
        s.request((11, 1), Ranking::Fresh);
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.set_system_reduced_motion(true);
        rt.frame(0.0);
        rt.update(|s, tx| { on_key(s, &KeyEvent { key: rune::Key::Text("4".into()), mods: Default::default() }, tx); });
        rt.frame(0.05);
        let elements = rt.elements();
        let group = elements.iter().find(|e| e.key.as_deref() == Some("feed-page-1")).unwrap();
        assert_eq!(group.opacity, 1.0);
        assert_eq!(group.rect, group.target);
        assert!(!elements.iter().any(|e| e.role == "radio" && e.focused));
        assert!(elements.iter().all(|e| !e.key.as_deref().is_some_and(|key| key.starts_with("story-"))));
    }

    fn route(rt: &mut rune::Runtime<App>, key: rune::Key) {
        rt.key(KeyEvent { key, mods: Default::default() });
        rt.frame(0.3);
    }

    #[test]
    fn feed_hover_selects_row_click_opens_and_save_stays_separate() {
        let mut s = app(true, SavedView::in_memory());
        s.request((11, 1), Ranking::Fresh);
        let ids: Vec<_> = s.feed.stories.iter().take(3).map(|story| story.id).collect();
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.on_key = Some(on_key);
        rt.frame(0.0);
        let [x, y, w, h] = rt.elements().iter().find(|e| e.key.as_deref() == Some(&format!("story-{}", ids[1]))).unwrap().rect;
        rt.pointer_move(x + w / 2.0, y + h / 2.0);
        rt.frame(0.0);
        assert_eq!(rt.state.feed.selected_id(), Some(ids[1]));
        assert!(rt.state.story.is_none());
        route(&mut rt, rune::Key::Down);
        assert_eq!(rt.state.feed.selected_id(), Some(ids[2]));
        rt.update(|_, tx| tx.scroll_to_instant("feed-list", 0.0, 0.0));
        rt.frame(0.0);
        assert_eq!(rt.state.feed.selected_id(), Some(ids[2]), "a still pointer does not override keyboard selection");
        let [x, y, w, h] = rt.elements().iter().find(|e| e.key.as_deref() == Some(&format!("story-{}", ids[0]))).unwrap().rect;
        rt.pointer_move(x + w / 2.0, y + h / 2.0);
        rt.frame(0.0);
        assert_eq!(rt.state.feed.selected_id(), Some(ids[0]));
        let elements = rt.elements();
        assert!(!elements.iter().any(|e| e.label.as_deref() == Some("open")));
        let [x, y, w, h] = elements.iter().find(|e| e.label.as_deref() == Some("save")).unwrap().rect;
        rt.pointer_down(x + w / 2.0, y + h / 2.0);
        rt.pointer_up(x + w / 2.0, y + h / 2.0);
        rt.frame(0.0);
        assert!(rt.state.saved.contains(ids[0]));
        assert!(rt.state.story.is_none(), "Save must not activate its row");
        let [x, y, w, h] = rt.elements().iter().find(|e| e.key.as_deref() == Some(&format!("title-{}", ids[0]))).unwrap().rect;
        rt.pointer_down(x + w / 2.0, y + h / 2.0);
        rt.pointer_up(x + w / 2.0, y + h / 2.0);
        rt.frame(0.0);
        assert_eq!(rt.state.story.as_ref().map(|story| story.id()), Some(ids[0]));
    }

    #[test]
    fn source_keys_keep_discussion_state_and_load_reply_context() {
        let mut s = app(true, SavedView::in_memory());
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        s.load_replies(901, false);
        s.story.as_mut().unwrap().thread.select(901);
        s.story.as_mut().unwrap().thread.fold();
        s.story.as_mut().unwrap().thread.reroot(902);
        let story = s.story.as_ref().unwrap();
        assert!(story.thread.focus());
        let cursor = story.thread.cursor();
        let root = story.thread.root();
        let folded = story.thread.is_folded(901);
        press(&mut s, rune::Key::Tab);
        press(&mut s, rune::Key::Enter);
        assert_eq!(s.story.as_ref().unwrap().shown_source().map(|source| source.id), Some(901));
        assert_eq!(s.story.as_ref().unwrap().thread.children_of(901), vec![9010, 9011]);
        assert_eq!(s.story.as_ref().unwrap().source.as_ref().unwrap().nav.cursor, Some(901));
        press(&mut s, rune::Key::Down);
        assert_eq!(s.story.as_ref().unwrap().source.as_ref().unwrap().nav.cursor, Some(9010));
        press(&mut s, rune::Key::Enter);
        assert!(!s.story.as_ref().unwrap().thread.children_of(9010).is_empty(), "Enter loads the selected reply branch");
        press(&mut s, rune::Key::Text("j".into()));
        assert_eq!(s.story.as_ref().unwrap().source.as_ref().unwrap().nav.cursor, Some(90100));
        press(&mut s, rune::Key::Text("c".into()));
        assert_eq!(s.story.as_ref().unwrap().source.as_ref().unwrap().nav.cursor, Some(9010));
        assert!(s.story.as_ref().unwrap().source.as_ref().unwrap().nav.folded.contains(&9010));
        let story = s.story.as_ref().unwrap();
        assert_eq!((story.thread.cursor(), story.thread.root(), story.thread.is_folded(901)), (cursor, root, folded));
        press(&mut s, rune::Key::Escape);
        assert!(s.story.as_ref().unwrap().source.is_none());
        press(&mut s, rune::Key::Escape);
        assert!(s.story.is_none(), "default focus mode does not consume back");
    }

    #[test]
    fn pointer_back_unwinds_source_then_story_and_preserves_discussion() {
        let mut s = app(true, SavedView::in_memory());
        s.request((11, 1), Ranking::Fresh);
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        s.load_replies(901, false);
        let thread = &mut s.story.as_mut().unwrap().thread;
        thread.select(901);
        thread.fold();
        let before = (thread.cursor(), thread.root(), thread.is_folded(901));
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.frame(0.0);
        rt.update(|s, _| s.open_citation(0));
        rt.frame(0.3);
        let button = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("story-back")).unwrap();
        assert_eq!(button.label.as_deref(), Some("Back to discussion"));
        assert!(button.focusable && button.rect[2] >= 36.0 && button.rect[3] >= 36.0);
        let [x, y, w, h] = button.rect;
        rt.pointer_move(x + w / 2.0, y + h / 2.0);
        rt.frame(0.1);
        assert!(rt.elements().iter().find(|e| e.key.as_deref() == Some("story-back")).unwrap().hover > 0.0);
        rt.pointer_down(x + w / 2.0, y + h / 2.0);
        rt.pointer_up(x + w / 2.0, y + h / 2.0);
        rt.frame(0.3);
        let story = rt.state.story.as_ref().unwrap();
        assert!(story.source.is_none() && story.citation_focus);
        assert_eq!((story.thread.cursor(), story.thread.root(), story.thread.is_folded(901)), before);
        assert!(rt.elements().iter().any(|e| e.key.as_deref() == Some("summary-scroll") && e.focused));
        assert_eq!(rt.elements().iter().find(|e| e.key.as_deref() == Some("story-back")).unwrap().label.as_deref(), Some("Back to feed"));

        rt.update(|s, _| s.open_citation(0));
        rt.frame(0.3);
        let [x, y, w, h] = rt.elements().iter().find(|e| e.key.as_deref() == Some("source-comment-901")).unwrap().visible;
        let (x, y) = (x + w / 2.0, y + h / 2.0);
        for button in [rune::PointerButton::Forward, rune::PointerButton::Middle, rune::PointerButton::Secondary] {
            rt.button_down(x, y, button);
            rt.button_up(x, y, button);
            rt.frame(0.0);
            assert!(rt.state.story.as_ref().unwrap().source.is_some());
        }
        rt.button_down(x, y, rune::PointerButton::Back);
        rt.button_up(x, y, rune::PointerButton::Back);
        rt.frame(0.3);
        assert!(rt.state.story.as_ref().unwrap().source.is_none());
        let [x, y, w, h] = rt.elements().iter().find(|e| e.key.as_deref() == Some("article-open-original")).unwrap().rect;
        rt.button_down(x + w / 2.0, y + h / 2.0, rune::PointerButton::Back);
        rt.button_up(x + w / 2.0, y + h / 2.0, rune::PointerButton::Back);
        rt.frame(0.3);
        assert!(rt.state.story.is_none());
        rt.update(|s, _| s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID)));
        rt.frame(0.3);
        let [x, y, w, h] = rt.elements().iter().find(|e| e.key.as_deref() == Some("comment-disclosure-901")).unwrap().visible;
        rt.button_down(x + w / 2.0, y + h / 2.0, rune::PointerButton::Back);
        rt.button_up(x + w / 2.0, y + h / 2.0, rune::PointerButton::Back);
        rt.frame(0.3);
        assert!(rt.state.story.is_none(), "Back over a disclosure returns to the feed");
    }

    #[test]
    fn passive_hints_follow_pointer_and_keyboard_without_dimming_actions() {
        for (reduced, contrast) in [(false, false), (true, false), (false, true)] {
            let mut s = app(true, SavedView::in_memory());
            s.request((11, 1), Ranking::Fresh);
            let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
            rt.set_system_reduced_motion(reduced);
            rt.set_system_increased_contrast(contrast);
            rt.on_key = Some(on_key);
            rt.frame(0.0);
            let hint = |rt: &rune::Runtime<App>| rt.elements().into_iter()
                .find(|e| e.text.as_deref() == Some("move") && e.path.contains("footer")).unwrap().opacity;
            rt.pointer_move(100.0, 100.0);
            rt.frame(0.05);
            if contrast { assert_eq!(hint(&rt), 1.0); }
            else if reduced { assert!((hint(&rt) - 0.3).abs() < 0.01); }
            else { assert!(hint(&rt) < 1.0 && hint(&rt) > 0.3); }
            rt.key(KeyEvent { key: rune::Key::Down, mods: Default::default() });
            rt.frame(0.0);
            assert_eq!(hint(&rt), 1.0, "keyboard navigation immediately restores hints");
            assert_eq!(rt.state.feed.selected, 1);
            rt.pointer_down(10.0, 10.0);
            rt.pointer_up(10.0, 10.0);
            rt.frame(0.5);
            if !contrast { assert!((hint(&rt) - 0.3).abs() < 0.01, "a stationary click counts as pointer use"); }

            rt.update(|s, _| {
                s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
                s.story.as_mut().unwrap().article_summary = crate::story::ArticleSummaryState::Done("Scrollable summary. ".repeat(150));
            });
            rt.frame(1.0);
            rt.key(KeyEvent { key: rune::Key::Text("j".into()), mods: Default::default() });
            rt.frame(0.0);
            assert_eq!(hint(&rt), 1.0, "discussion navigation restores hints");
            let elements = rt.elements();
            let [x, y, w, h] = elements.iter().find(|e| e.key.as_deref() == Some("summary-scroll")).unwrap().rect;
            let before = elements.iter().find(|e| e.key.as_deref() == Some("article-summary")).unwrap().rect[1];
            rt.wheel_xy(x + w / 2.0, y + h / 2.0, 0.0, -80.0);
            rt.frame(0.5);
            let after = rt.elements();
            assert!(after.iter().find(|e| e.key.as_deref() == Some("article-summary")).unwrap().rect[1] < before);
            if !contrast { assert!((hint(&rt) - 0.3).abs() < 0.01, "wheel use dims the passive hints"); }
            for key in ["story-back", "article-open-original"] {
                assert_eq!(after.iter().find(|e| e.key.as_deref() == Some(key)).unwrap().opacity, 1.0);
            }
            rt.update(|_, tx| tx.focus("summary-scroll", true));
            rt.frame(0.0);
            rt.key(KeyEvent { key: rune::Key::Tab, mods: Default::default() });
            rt.frame(0.0);
            assert_eq!(hint(&rt), 1.0, "citation Tab restores hints");
            rt.wheel_xy(x + w / 2.0, y + h / 2.0, 0.0, 80.0);
            rt.frame(0.5);
            if !contrast { assert!((hint(&rt) - 0.3).abs() < 0.01); }
            rt.update(|_, tx| tx.focus("summary-scroll", true));
            rt.frame(0.0);
            rt.key(KeyEvent { key: rune::Key::PageUp, mods: Default::default() });
            rt.frame(0.0);
            assert_eq!(hint(&rt), 1.0, "a viewport-consumed Page Up restores hints");
        }
    }

    #[test]
    fn source_selection_context_and_pointer_tab_follow_the_visible_citation() {
        let mut s = app(true, SavedView::in_memory());
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        if let crate::story::SummaryState::Done(summary) = &mut s.story.as_mut().unwrap().summary {
            summary.bullets[1].sources.push(901);
        }
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.on_key = Some(on_key);
        rt.frame(0.0);
        route(&mut rt, rune::Key::Tab);
        route(&mut rt, rune::Key::Enter);
        let before = rt.elements();
        assert!(before.iter().any(|e| e.key.as_deref() == Some("source-summary-0")));
        assert!(before.iter().any(|e| e.key.as_deref() == Some("source-summary-1")), "all matching claims are shown");
        assert!(before.iter().any(|e| e.key.as_deref() == Some("source-comment-901") && e.item_selected));
        route(&mut rt, rune::Key::Down);
        assert!(rt.elements().iter().any(|e| e.key.as_deref() == Some("source-comment-9010") && e.item_selected));
        let [x, y, w, h] = rt.elements().iter().find(|e| e.key.as_deref() == Some("source-comment-9010")).unwrap().rect;
        rt.pointer_down(x + w / 2.0, y + h / 2.0);
        rt.pointer_up(x + w / 2.0, y + h / 2.0);
        rt.frame(0.0);
        route(&mut rt, rune::Key::Enter);
        assert!(!rt.state.story.as_ref().unwrap().thread.children_of(9010).is_empty(), "Enter activates the pointed reply");
        route(&mut rt, rune::Key::Tab);
        let story = rt.state.story.as_ref().unwrap();
        assert_eq!(story.shown_source().map(|source| source.id), Some(902));
        assert_eq!(story.source.as_ref().unwrap().nav.cursor, Some(902));
        let after = rt.elements();
        assert!(after.iter().any(|e| e.key.as_deref() == Some("source-summary-0")));
        assert!(!after.iter().any(|e| e.key.as_deref() == Some("source-summary-1")));
    }

    #[test]
    fn captured_source_remains_selected_when_the_live_comment_is_gone() {
        let mut s = app(true, SavedView::in_memory());
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        s.story.as_mut().unwrap().thread = crate::comments::Thread::empty();
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.on_key = Some(on_key);
        rt.frame(0.0);
        route(&mut rt, rune::Key::Tab);
        route(&mut rt, rune::Key::Enter);
        let rows = rt.elements();
        assert!(rows.iter().any(|e| e.key.as_deref() == Some("source-comment-901") && e.item_selected));
        assert!(rows.iter().any(|e| e.key.as_deref() == Some("source-comment-text")));
        route(&mut rt, rune::Key::Down);
        assert_eq!(rt.state.story.as_ref().unwrap().source.as_ref().unwrap().nav.cursor, Some(901));
    }

    #[test]
    fn source_selection_survives_a_refreshed_reply_becoming_unavailable() {
        for reroot in [false, true] {
            let mut s = app(true, SavedView::in_memory());
            s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
            s.open_citation(0);
            press(&mut s, rune::Key::Text("G".into()));
            assert_eq!(s.story.as_ref().unwrap().source.as_ref().unwrap().nav.cursor, Some(9011));
            if reroot { s.story.as_mut().unwrap().with_thread(|thread| { thread.reroot(9011); }); }
            press(&mut s, rune::Key::Text("f".into()));
            let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
            fake.comments.lock().unwrap().iter_mut().find(|comment| comment.id == 9011).unwrap().deleted = true;
            s.refresh_comments();
            let source = s.story.as_ref().unwrap().source.as_ref().unwrap();
            assert_eq!(source.nav.cursor, Some(if reroot { 901 } else { 9010 }));
            assert!(!source.nav.focus, "refresh preserves the chosen focus mode");
            press(&mut s, rune::Key::Up);
            assert_eq!(s.story.as_ref().unwrap().source.as_ref().unwrap().nav.cursor, Some(901));
        }
    }

    #[test]
    fn discussion_c_collapses_the_nearest_open_ancestor() {
        let mut s = app(true, SavedView::in_memory());
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        s.load_replies(901, false);
        s.story.as_mut().unwrap().thread.select(9011);
        press(&mut s, rune::Key::Text("c".into()));
        let thread = &s.story.as_ref().unwrap().thread;
        assert_eq!(thread.cursor(), Some(901));
        assert!(thread.is_folded(901));
        assert!(thread.rows().iter().any(|row| row.id == 901));
        assert!(!thread.rows().iter().any(|row| row.id == 9011));
    }

    #[test]
    fn pointer_movement_selects_in_focus_mode_without_scroll_or_keyboard_churn() {
        let mut s = app(true, SavedView::in_memory());
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        s.load_replies(901, false);
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.on_key = Some(on_key);
        rt.frame(0.0);
        rt.update(|_, tx| tx.scroll_into_view_in("discussion-scroll", "comment-9010"));
        rt.frame(0.5);
        let [x, y, w, h] = rt.elements().iter().find(|e| e.key.as_deref() == Some("comment-9010")).unwrap().visible;
        assert!(h > 2.0 && w > 2.0);
        rt.pointer_move(x + w / 2.0, y + h / 2.0);
        rt.frame(0.5);
        assert_eq!(rt.state.story.as_ref().unwrap().thread.cursor(), Some(9010));
        route(&mut rt, rune::Key::Down);
        assert_eq!(rt.state.story.as_ref().unwrap().thread.cursor(), Some(9011));
        rt.update(|_, tx| tx.scroll_to_instant("discussion-scroll", 0.0, 0.0));
        rt.frame(0.5);
        assert_eq!(rt.state.story.as_ref().unwrap().thread.cursor(), Some(9011), "scrolling under a still pointer does not select");
        route(&mut rt, rune::Key::Text("f".into()));
        let [x, y, w, h] = rt.elements().iter().find(|e| e.key.as_deref() == Some("comment-901")).unwrap().visible;
        rt.pointer_move(x + w / 2.0, y + h / 2.0);
        rt.frame(0.5);
        assert_eq!(rt.state.story.as_ref().unwrap().thread.cursor(), Some(9011), "hover selection follows focus mode");
    }

    #[test]
    fn hovering_across_a_root_reply_boundary_keeps_rows_under_the_pointer() {
        let mut s = app(true, SavedView::in_memory());
        scene_comments(&mut s);
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.frame(0.0);
        let row = |rt: &rune::Runtime<App>, key: &str| rt.elements().into_iter()
            .find(|e| e.key.as_deref() == Some(key) && !e.exiting).unwrap();
        let before_root = row(&rt, "comment-901");
        let before_reply = row(&rt, "comment-9010");
        let x = before_reply.rect[0] + before_reply.rect[2] / 2.0;
        let boundary = before_reply.rect[1];
        rt.pointer_move(x, before_root.rect[1] + before_root.rect[3] - 1.0);
        rt.frame(0.05);
        for dy in [1.0, 2.0, 1.0, 2.0] {
            rt.pointer_move(x, boundary + dy);
            rt.frame(0.05);
            assert_eq!(rt.state.story.as_ref().unwrap().thread.cursor(), Some(9010), "1 pt movement must stay on the reply");
            let root = row(&rt, "comment-901");
            let reply = row(&rt, "comment-9010");
            assert_eq!(root.rect, before_root.rect, "hover must not move the root row");
            assert_eq!(reply.rect, before_reply.rect, "hover must not move the reply row");
            assert_eq!(reply.scrolled, before_reply.scrolled, "hover must not scroll the discussion");
        }
    }

    #[test]
    fn hovering_between_equal_depths_with_different_ancestors_keeps_geometry() {
        let mut s = app(true, SavedView::in_memory());
        scene_comments(&mut s);
        let comments: Vec<_> = crate::fixtures::thread().into_iter()
            .filter(|c| [901, 9010, 903, 9030].contains(&c.id)).collect();
        s.story.as_mut().unwrap().thread = crate::comments::Thread::from_comments(&comments);
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.frame(0.0);
        let row = |rt: &rune::Runtime<App>, key: &str| rt.elements().into_iter()
            .find(|e| e.key.as_deref() == Some(key) && !e.exiting).unwrap();
        let first = row(&rt, "comment-9010").visible;
        rt.pointer_move(first[0] + first[2] / 2.0, first[1] + first[3] / 2.0);
        rt.frame(0.5);
        let before = row(&rt, "comment-9030");
        let [x, y, w, h] = before.visible;
        assert!(w > 0.0 && h > 0.0);
        rt.pointer_move(x + w / 2.0, y + h / 2.0);
        rt.frame(0.05);
        assert_eq!(rt.state.story.as_ref().unwrap().thread.cursor(), Some(9030));
        assert_eq!(row(&rt, "comment-9030").rect, before.rect);
        assert_eq!(row(&rt, "comment-9030").scrolled, before.scrolled);
    }

    #[test]
    fn disclosure_edge_click_folds_without_slow_row_reflow() {
        for (source, reduced) in [(false, false), (true, false), (false, true)] {
            let mut s = app(true, SavedView::in_memory());
            scene_comments(&mut s);
            if source { s.open_citation(0); }
            let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
            rt.set_system_reduced_motion(reduced);
            rt.frame(0.0);
            let elements = rt.elements();
            let button = elements.iter().find(|e| e.key.as_deref() == Some("comment-disclosure-901") && !e.inert).unwrap();
            assert_eq!((button.rect[2], button.rect[3]), (28.0, 28.0));
            let [x, y, _, _] = button.rect;
            rt.pointer_down(x + 1.0, y + 1.0);
            rt.pointer_up(x + 1.0, y + 1.0);
            rt.frame(0.0);
            let story = rt.state.story.as_ref().unwrap();
            assert!(if source { story.source.as_ref().unwrap().nav.folded.contains(&901) } else { story.thread.is_folded(901) });
            let reply = if source { "source-comment-9010" } else { "comment-9010" };
            assert!(!rt.elements().iter().any(|e| e.key.as_deref() == Some(reply)));
            let next = if source { "source-comment-901" } else { "comment-902" };
            let y0 = rt.elements().iter().find(|e| e.key.as_deref() == Some(next) && !e.inert).unwrap().rect[1];
            rt.frame(0.15);
            let y1 = rt.elements().iter().find(|e| e.key.as_deref() == Some(next) && !e.inert).unwrap().rect[1];
            assert!((y0 - y1).abs() < 0.5, "folded rows must not drift after the response: source={source} reduced={reduced} {y0} -> {y1}");
        }
    }

    #[test]
    fn arriving_replies_fade_once_and_respect_reduced_motion() {
        for reduced in [false, true] {
            let mut s = app(true, SavedView::in_memory());
            s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
            s.headless = false;
            s.open_citation(0);
            let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
            rt.set_system_reduced_motion(reduced);
            rt.frame(0.0);
            rt.poll = Some(poll);
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while rt.state.story.as_ref().unwrap().pending_reply.is_some() {
                rt.frame(0.0);
                assert!(std::time::Instant::now() < deadline);
                std::thread::yield_now();
            }
            rt.frame(0.05);
            let opacity = rt.elements().iter().find(|e| e.key.as_deref() == Some("source-comment-9010")).unwrap().opacity;
            if reduced { assert_eq!(opacity, 1.0); }
            else { assert!(opacity > 0.0 && opacity < 1.0, "reply opacity at 50ms: {opacity}"); }
            rt.frame(1.0);
            rt.update(|_, tx| tx.scroll_to_instant("source-viewer", 0.0, 10000.0));
            rt.frame(0.0);
            rt.update(|_, tx| tx.scroll_to_instant("source-viewer", 0.0, 0.0));
            rt.frame(0.0);
            assert_eq!(rt.elements().iter().find(|e| e.key.as_deref() == Some("source-comment-9010")).unwrap().opacity, 1.0,
                "scrolling back does not replay the arrival");
        }
    }

    #[test]
    #[ignore = "Metal-backed reply arrival and summary overflow capture"]
    fn render_reply_and_summary_revision() {
        fn reply() -> App {
            let mut s = app(true, SavedView::in_memory());
            s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
            s.headless = false;
            s.open_citation(0);
            s
        }
        fn reduced() -> App { let mut s = reply(); s.reduced = true; s }
        fn overflow() -> App {
            let mut s = app(true, SavedView::in_memory());
            s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
            s.story.as_mut().unwrap().article_summary = crate::story::ArticleSummaryState::Done(
                "The article describes the design and its consequences for readers. ".repeat(14));
            s
        }
        fn delayed_poll(s: &mut App, now: f64, tx: &mut Tx) -> bool {
            if now < 0.2 { return false; }
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            let mut changed = false;
            while s.story.as_ref().unwrap().pending_reply.is_some() {
                changed |= poll(s, now, tx);
                assert!(std::time::Instant::now() < deadline);
                std::thread::yield_now();
            }
            changed | poll(s, now, tx)
        }
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/citation-nav-review");
        std::fs::create_dir_all(&out).unwrap();
        for (name, init, at, is_reply) in [
            ("reply-normal-50", reply as fn() -> App, "0.25", true),
            ("reply-normal-100", reply, "0.30", true),
            ("reply-reduced-50", reduced, "0.25", true),
            ("summary-overflow", overflow, "0.5", false),
        ] {
            let stem = out.join(name);
            let args = rune::tooling::Args::parse_from(vec!["citation-revision".into(),
                "--size".into(), "1180x760".into(), "--scale".into(), "2".into(),
                "--theme".into(), "dark".into(), "--at".into(), at.into(),
                "--snap".into(), stem.with_extension("png").display().to_string(),
                "--tree".into(), stem.with_extension("json").display().to_string()]);
            let mut spec = AppSpec::new("Hacker News", (1180.0, 760.0), init, view);
            if is_reply { spec.poll = Some(delayed_poll); }
            rune::tooling::headless(rune::App::new(spec), args);
        }
    }

    #[test]
    fn discussion_selection_after_source_return_gives_enter_back_to_the_comment() {
        let mut s = app(true, SavedView::in_memory());
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.on_key = Some(on_key);
        rt.frame(0.0);
        route(&mut rt, rune::Key::Tab);
        route(&mut rt, rune::Key::Enter);
        route(&mut rt, rune::Key::Escape);
        route(&mut rt, rune::Key::Down);
        route(&mut rt, rune::Key::Enter);
        let story = rt.state.story.as_ref().unwrap();
        assert!(story.source.is_none(), "Enter belongs to the selected discussion comment");
        assert!(!story.thread.children_of(9010).is_empty(), "Enter loads the selected comment's replies");
        let [x, y, w, h] = rt.elements().iter().find(|e| e.key.as_deref() == Some("comment-9010")).unwrap().rect;
        rt.pointer_down(x + w / 2.0, y + h / 2.0);
        rt.pointer_up(x + w / 2.0, y + h / 2.0);
        rt.frame(0.0);
        route(&mut rt, rune::Key::Enter);
        assert!(rt.state.story.as_ref().unwrap().thread.is_folded(9010), "Enter after clicking folds the comment");
        assert!(rt.elements().iter().any(|e| e.key.as_deref() == Some("comment-text-9010")), "the selected comment remains readable");
    }

    #[test]
    fn inline_citation_keyboard_opens_and_returns_to_its_reference() {
        let mut s = app(true, SavedView::in_memory());
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.on_key = Some(on_key);
        rt.frame(0.0);
        let cite = rt.elements().iter().find(|e| e.key.as_deref() == Some("cite-0")).unwrap().clone();
        assert!(cite.focusable && cite.role == "button" && cite.label.as_deref() == Some("Read comment by tptacek"));
        route(&mut rt, rune::Key::Tab);
        route(&mut rt, rune::Key::Enter);
        assert_eq!(rt.state.story.as_ref().unwrap().shown_source().map(|source| source.id), Some(901));
        assert!(rt.elements().iter().find(|e| e.key.as_deref() == Some("discussion-scroll")).unwrap().inert);
        assert!(!rt.accessibility_json().to_string().contains("Summary content"), "the hidden inspector leaves the accessibility tree");
        route(&mut rt, rune::Key::Escape);
        assert!(rt.state.story.as_ref().unwrap().source.is_none());
        assert!(rt.state.story.as_ref().unwrap().citation_focus);
        assert_eq!(rt.state.story.as_ref().unwrap().citation, 0);
        assert!(rt.elements().iter().any(|e| e.key.as_deref() == Some("summary-scroll") && e.focused), "native focus returns to the summary");
        route(&mut rt, rune::Key::Enter);
        assert_eq!(rt.state.story.as_ref().unwrap().shown_source().map(|source| source.id), Some(901), "Enter reopens the returned citation");
    }

    #[test]
    fn question_mark_toggles_help_in_discussion_summary_and_source() {
        let mut s = app(true, SavedView::in_memory());
        scene_comments(&mut s);
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.on_key = Some(on_key);
        rt.frame(0.0);
        let question = || KeyEvent { key: rune::Key::Text("?".into()), mods: rune::Mods { shift: true, ..Default::default() } };
        for surface in ["discussion", "summary", "source"] {
            match surface {
                "discussion" => {
                    let [x, y, w, h] = rt.elements().iter().find(|e| e.key.as_deref() == Some("comment-901")).unwrap().visible;
                    rt.pointer_down(x + w / 2.0, y + h / 2.0);
                    rt.pointer_up(x + w / 2.0, y + h / 2.0);
                    rt.frame(0.0);
                }
                "summary" => {
                    let [x, y, w, h] = rt.elements().iter().find(|e| e.key.as_deref() == Some("article-summary")).unwrap().visible;
                    rt.pointer_down(x + w / 2.0, y + h / 2.0);
                    rt.pointer_up(x + w / 2.0, y + h / 2.0);
                    rt.frame(0.0);
                    assert!(rt.elements().iter().any(|e| e.key.as_deref() == Some("summary-scroll") && e.focused));
                }
                _ => { rt.update(|s, _| s.open_citation(0)); rt.frame(0.3); }
            }
            rt.key(question()); rt.frame(0.3);
            assert!(rt.state.help, "help opens from {surface}");
            assert!(rt.elements().iter().any(|e| e.key.as_deref() == Some("help-dialog") && !e.inert));
            rt.key(question()); rt.frame(0.3);
            assert!(!rt.state.help, "help closes from {surface}");
            assert_eq!(rt.state.story.as_ref().unwrap().source.is_some(), surface == "source");
        }
        rt.update(|s, _| s.open_summary_settings()); rt.frame(0.3);
        rt.key(question()); rt.frame(0.3);
        assert!(rt.state.settings.is_some() && !rt.state.help, "settings retains its input");
    }

    #[test]
    fn o_activates_reading_selection_and_shift_o_queues_browser() {
        let mut feed = app(true, SavedView::in_memory());
        feed.request((11, 1), Ranking::Fresh);
        let mut feed = rune::Runtime::new(feed, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        feed.on_key = Some(on_key);
        feed.frame(0.0);
        route(&mut feed, rune::Key::Text("o".into()));
        assert!(feed.state.story.is_some() && !feed.state.pending_external);

        let mut s = app(true, SavedView::in_memory());
        scene_comments(&mut s);
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.on_key = Some(on_key);
        rt.frame(0.0);
        route(&mut rt, rune::Key::Text("o".into()));
        assert!(rt.state.story.as_ref().unwrap().thread.is_folded(901));
        route(&mut rt, rune::Key::Text("o".into()));
        assert!(!rt.state.story.as_ref().unwrap().thread.is_folded(901));
        route(&mut rt, rune::Key::Tab);
        route(&mut rt, rune::Key::Text("o".into()));
        assert_eq!(rt.state.story.as_ref().unwrap().shown_source().map(|source| source.id), Some(901));
        route(&mut rt, rune::Key::Text("o".into()));
        assert!(rt.state.story.as_ref().unwrap().source.as_ref().unwrap().nav.folded.contains(&901));
        rt.key(KeyEvent { key: rune::Key::Text("O".into()), mods: rune::Mods { shift: true, ..Default::default() } });
        rt.frame(0.0);
        assert!(rt.state.pending_external && rt.state.story.as_ref().unwrap().source.is_some());
    }

    #[test]
    fn link_chooser_keeps_tab_away_from_summary_citations() {
        let mut s = app(true, SavedView::in_memory());
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        s.story.as_mut().unwrap().thread.select(905);
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.on_key = Some(on_key);
        rt.frame(0.0);
        route(&mut rt, rune::Key::Text("L".into()));
        assert!(rt.state.story.as_ref().unwrap().links.is_some());
        let [x, y, w, h] = rt.elements().iter().find(|e| e.key.as_deref() == Some("link-0")).unwrap().visible;
        rt.pointer_move(x + w / 2.0, y + h / 2.0);
        rt.frame(0.3);
        assert_eq!(rt.state.hints.get(), 0.3);
        route(&mut rt, rune::Key::Tab);
        assert_eq!(rt.state.hints.get(), 1.0, "Tab in the chooser restores hints");
        let story = rt.state.story.as_ref().unwrap();
        assert!(story.links.is_some());
        assert!(!story.citation_focus, "the chooser owns Tab, not the summary behind it");
        assert!(!rt.elements().iter().any(|e| e.key.as_deref() == Some("summary-scroll") && e.focused));
    }

    #[test]
    fn mouse_back_respects_the_chooser_and_modal_scope() {
        let mut s = app(true, SavedView::in_memory());
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        s.story.as_mut().unwrap().thread.select(905);
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.frame(0.0);
        rt.update(|s, _| s.choose_links());
        rt.frame(0.3);
        let [x, y, w, h] = rt.elements().iter().find(|e| e.key.as_deref() == Some("link-0")).unwrap().visible;
        rt.button_down(x + w / 2.0, y + h / 2.0, rune::PointerButton::Back);
        rt.button_up(x + w / 2.0, y + h / 2.0, rune::PointerButton::Back);
        rt.frame(0.3);
        assert!(rt.state.story.as_ref().unwrap().links.is_none());
        rt.update(|s, _| s.open_summary_settings());
        rt.frame(0.3);
        let [x, y, w, h] = rt.elements().iter().find(|e| e.key.as_deref() == Some("settings-dialog")).unwrap().rect;
        rt.button_down(x + w / 2.0, y + h / 2.0, rune::PointerButton::Back);
        rt.button_up(x + w / 2.0, y + h / 2.0, rune::PointerButton::Back);
        rt.frame(0.0);
        assert!(rt.state.settings.is_some() && rt.state.story.is_some(), "the modal keeps background navigation blocked");
    }

    #[test]
    fn clicking_load_replies_releases_citation_enter_ownership() {
        let mut s = app(true, SavedView::in_memory());
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        s.story.as_mut().unwrap().thread.toggle_focus();
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.on_key = Some(on_key);
        rt.frame(0.0);
        route(&mut rt, rune::Key::Tab);
        let [x, y, w, h] = rt.elements().iter().find(|e| e.key.as_deref() == Some("comment-load-901-false")).unwrap().visible;
        assert!(w > 0.0 && h > 0.0);
        rt.pointer_down(x + w / 2.0, y + h / 2.0);
        rt.pointer_up(x + w / 2.0, y + h / 2.0);
        rt.frame(0.0);
        assert!(!rt.state.story.as_ref().unwrap().citation_focus);
        route(&mut rt, rune::Key::Enter);
        assert!(rt.state.story.as_ref().unwrap().source.is_none());
    }

    #[test]
    fn clicking_summary_keeps_tab_and_shift_tab_on_citations() {
        let mut s = app(true, SavedView::in_memory());
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.on_key = Some(on_key);
        rt.frame(0.0);
        let [x, y, w, h] = rt.elements().iter().find(|e| e.key.as_deref() == Some("article-summary")).unwrap().rect;
        rt.pointer_down(x + w / 2.0, y + h / 2.0);
        rt.pointer_up(x + w / 2.0, y + h / 2.0);
        rt.frame(0.0);
        assert!(rt.elements().iter().any(|e| e.key.as_deref() == Some("summary-scroll") && e.focused));
        route(&mut rt, rune::Key::Tab);
        assert!(rt.state.story.as_ref().unwrap().citation_focus, "Tab after a pointer click must select a citation");
        assert_eq!(rt.state.story.as_ref().unwrap().citation, 0);
        route(&mut rt, rune::Key::Tab);
        assert_eq!(rt.state.story.as_ref().unwrap().citation, 1);
        rt.key(KeyEvent { key: rune::Key::Tab, mods: rune::Mods { shift: true, ..Default::default() } });
        rt.frame(0.3);
        assert_eq!(rt.state.story.as_ref().unwrap().citation, 0);
        route(&mut rt, rune::Key::Enter);
        assert_eq!(rt.state.story.as_ref().unwrap().shown_source().map(|source| source.id), Some(901));
        route(&mut rt, rune::Key::Escape);
        route(&mut rt, rune::Key::Tab);
        assert_eq!(rt.state.story.as_ref().unwrap().citation, 1, "Tab must continue after returning from a source");
        route(&mut rt, rune::Key::Enter);
        assert_eq!(rt.state.story.as_ref().unwrap().shown_source().map(|source| source.id), Some(902));
    }

    #[test]
    fn summary_overflow_remains_keyboard_scrollable_and_leaves_discussion_room() {
        let mut s = app(true, SavedView::in_memory());
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        s.story.as_mut().unwrap().article_summary = crate::story::ArticleSummaryState::Done("A long article synopsis. ".repeat(100));
        let mut rt = rune::Runtime::new(s, view, (1040.0, 640.0), rune::Env::new(true, 1.0));
        rt.on_key = Some(on_key);
        rt.frame(0.0);
        let before = rt.elements();
        let summary = before.iter().find(|e| e.key.as_deref() == Some("summary-scroll")).unwrap();
        let discussion = before.iter().find(|e| e.key.as_deref() == Some("discussion-scroll")).unwrap();
        assert!(summary.focusable && summary.rect[3] <= 274.0);
        assert!(discussion.rect[3] > 200.0, "discussion remains usable: {:?}", discussion.rect);
        assert!(before.iter().find(|e| e.key.as_deref() == Some("summary-more")).unwrap().opacity > 0.9,
            "overflow is signalled without covering the content");
        let more = before.iter().find(|e| e.key.as_deref() == Some("summary-more")).unwrap();
        assert!(more.rect[1] >= summary.rect[1] + summary.rect[3] - 1.0);
        rt.update(|s, _| { s.story.as_mut().unwrap().thread.next(); });
        rt.frame(0.0);
        assert!(rt.elements().iter().find(|e| e.key.as_deref() == Some("summary-more")).unwrap().opacity > 0.9,
            "rebuilding the view preserves the overflow cue");
        let y = before.iter().find(|e| e.key.as_deref() == Some("article-summary")).unwrap().rect[1];
        route(&mut rt, rune::Key::Text("J".into()));
        let moved = rt.elements().iter().find(|e| e.key.as_deref() == Some("article-summary")).unwrap().rect[1];
        assert!(moved < y, "Shift+J scrolls the summary");
        rt.update(|_, tx| tx.focus("summary-scroll", true));
        rt.frame(0.0);
        route(&mut rt, rune::Key::PageUp);
        let restored = rt.elements().iter().find(|e| e.key.as_deref() == Some("article-summary")).unwrap().rect[1];
        assert!(restored > moved, "Page Up in the summary scrolls that viewport");
        rt.update(|_, tx| tx.scroll_to_instant("summary-scroll", 0.0, f32::MAX));
        rt.frame(0.0);
        assert!(rt.elements().iter().find(|e| e.key.as_deref() == Some("summary-more")).unwrap().opacity < 0.1,
            "the cue disappears at the end");
    }

    #[test]
    fn source_return_keeps_the_visible_passage_when_replies_arrive_above_it() {
        let mut s = app(true, SavedView::in_memory());
        if let Source::Fake(fake) = s.source.as_ref() {
            let mut comments = fake.comments.lock().unwrap();
            let template = comments.iter().find(|comment| comment.id == 902).unwrap().clone();
            comments.extend((10000..10020).map(|id| { let mut comment = template.clone(); comment.id = id; comment }));
        }
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.frame(0.0);
        rt.update(|_, tx| tx.scroll_to_instant("discussion-scroll", 0.0, 700.0));
        rt.frame(0.0);
        let before = rt.elements();
        let anchor = before.iter().find(|e| e.key.as_deref().is_some_and(|key| key.starts_with("comment-100")) && e.visible[3] > 30.0).unwrap();
        let key = anchor.key.clone().unwrap();
        let y = anchor.rect[1];
        rt.update(|s, _| s.open_citation(0));
        rt.frame(0.0);
        let [x, button_y, w, h] = rt.elements().iter().find(|e| e.key.as_deref() == Some("story-back")).unwrap().rect;
        rt.pointer_down(x + w / 2.0, button_y + h / 2.0);
        rt.pointer_up(x + w / 2.0, button_y + h / 2.0);
        rt.frame(0.3);
        let after = rt.elements();
        let restored = after.iter().find(|e| e.key.as_deref() == Some(&key)).unwrap();
        assert!((restored.rect[1] - y).abs() < 2.0, "same passage returns at the same position");
    }

    #[test]
    fn numeric_tab_shortcuts_return_arrow_navigation_to_the_feed() {
        for (key, tab) in [("1", StoryType::Top), ("2", StoryType::New), ("3", StoryType::Best), ("4", StoryType::Saved)] {
            let mut s = app(true, saved_store());
            s.request((11, 1), Ranking::Fresh);
            let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
            rt.on_key = Some(on_key);
            rt.frame(0.0);
            route(&mut rt, rune::Key::Tab); // Deliberately begin with the radio group focused.
            route(&mut rt, rune::Key::Text(key.into()));
            route(&mut rt, rune::Key::Down);
            assert_eq!(rt.state.feed.tab, tab);
            assert_eq!(rt.state.feed.selected, 1, "{key} then Down moves within the chosen feed");
            route(&mut rt, rune::Key::Up);
            assert_eq!(rt.state.feed.tab, tab);
            assert_eq!(rt.state.feed.selected, 0);
        }
    }

    #[test]
    fn full_feed_has_balanced_gutters_and_pointer_tabs_return_navigation_to_rows() {
        for size in [(800.0, 640.0), (1180.0, 760.0), (1600.0, 900.0)] {
            let mut s = app(true, saved_store());
            s.request(DEFAULT_GRID, Ranking::Fresh);
            let mut rt = rune::Runtime::new(s, view, size, rune::Env::new(true, 1.0));
            rt.on_key = Some(on_key);
            rt.frame(0.0);
            for _ in 0..2 {
                rt.update(|s, tx| { s.request(s.measured.get().unwrap_or(DEFAULT_GRID), Ranking::Fresh); tx.instant(); });
                rt.frame(0.0);
            }
            let elements = rt.elements();
            let viewport = elements.iter().find(|e| e.key.as_deref() == Some("feed-list")).unwrap().rect;
            let first = elements.iter().find(|e| e.key.as_deref() == Some("story-1")).unwrap().rect;
            let (rows, cols) = rt.state.measured.get().unwrap();
            let last_id = rt.state.feed.stories[(rows - 1).min(rt.state.feed.stories.len() - 1)].id;
            let last = elements.iter().find(|e| e.key.as_deref() == Some(&format!("story-{last_id}"))).unwrap().rect;
            let top = first[1] - viewport[1];
            let bottom = viewport[1] + viewport[3] - last[1] - last[3];
            assert!(top >= FEED_GUTTER && (top - bottom).abs() < 1.0, "{size:?}: {top} vs {bottom}");
            assert_eq!(cols, if size.0 >= BREAKPOINT { 2 } else { 1 });
            let brand = elements.iter().find(|e| e.key.as_deref() == Some("brand")).unwrap();
            assert_eq!(brand.label.as_deref(), Some("Hacker News"));
            assert!(!brand.focusable);
            let new = elements.iter().find(|e| e.key.as_deref() == Some("segment1")).unwrap();
            assert_eq!((new.role, new.rect[3]), ("radio", 36.0));
            let [x, y, w, h] = new.rect;
            rt.key(KeyEvent { key: rune::Key::Tab, mods: Default::default() });
            rt.frame(0.0);
            rt.pointer_down(x + w / 2.0, y + h / 2.0);
            rt.pointer_up(x + w / 2.0, y + h / 2.0);
            rt.frame(0.0);
            rt.key(KeyEvent { key: rune::Key::Down, mods: Default::default() });
            rt.frame(0.0);
            assert_eq!((rt.state.feed.tab, rt.state.feed.selected), (StoryType::New, 1));
            rt.key(KeyEvent { key: rune::Key::Text("4".into()), mods: Default::default() });
            rt.frame(0.0);
            let saved = rt.elements();
            let viewport = saved.iter().find(|e| e.key.as_deref() == Some("feed-list")).unwrap().rect;
            let first = saved.iter().find(|e| e.key.as_deref() == Some("story-11")).unwrap().rect;
            assert!((first[1] - viewport[1] - FEED_GUTTER).abs() < 0.5, "short Saved stays top-aligned");
            rt.state.headless = false;
            rt.key(KeyEvent { key: rune::Key::Text("1".into()), mods: Default::default() });
            rt.frame(0.0);
            let loading = rt.elements();
            let viewport = loading.iter().find(|e| e.key.as_deref() == Some("feed-list")).unwrap().rect;
            let line = loading.iter().find(|e| e.path.ends_with("feed-placeholder/#0")).unwrap().rect;
            assert!((line[1] - viewport[1] - feed_gutter(viewport[3], rows, cols, true)).abs() < 0.5);
            rt.state.headless = true;
            rt.poll = Some(poll);
            rt.set_viewport(if cols == 1 { (1600.0, 900.0) } else { (800.0, 640.0) }, 1.0);
            for _ in 0..4 { rt.frame(0.25); }
            let (rows, new_cols) = rt.state.measured.get().unwrap();
            assert_ne!(cols, new_cols, "resize crosses the column breakpoint");
            let elements = rt.elements();
            let viewport = elements.iter().find(|e| e.key.as_deref() == Some("feed-list")).unwrap().rect;
            let first = elements.iter().find(|e| e.key.as_deref() == Some("story-1")).unwrap().rect;
            let last_id = rt.state.feed.stories[rows - 1].id;
            let last = elements.iter().find(|e| e.key.as_deref() == Some(&format!("story-{last_id}"))).unwrap().rect;
            assert!(((first[1] - viewport[1]) - (viewport[1] + viewport[3] - last[1] - last[3])).abs() < 1.0,
                "resized full pages retain balanced gutters: {size:?} {rows} {new_cols} {viewport:?} {first:?} {last:?}");
        }
    }

    #[test]
    fn intentionally_focused_tab_radios_keep_their_arrow_navigation() {
        let mut s = app(true, SavedView::in_memory());
        s.request((11, 1), Ranking::Fresh);
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.on_key = Some(on_key);
        rt.frame(0.0);
        route(&mut rt, rune::Key::Tab);
        route(&mut rt, rune::Key::Down);
        assert_eq!(rt.state.feed.tab, StoryType::New);
        assert!(rt.elements().iter().any(|e| e.key.as_deref() == Some("segment1") && e.focused));
        rt.update(|s, _| { s.open_summary_settings(); });
        rt.frame(0.0);
        route(&mut rt, rune::Key::Text("3".into()));
        assert_eq!(rt.state.feed.tab, StoryType::New, "dialog input must not switch feeds");
        assert!(rt.state.settings.is_some());
    }

    #[test]
    fn accessibility_click_keeps_tab_group_keyboard_navigation() {
        let mut s = app(true, SavedView::in_memory());
        s.request(DEFAULT_GRID, Ranking::Fresh);
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.on_key = Some(on_key);
        rt.frame(0.0);
        assert!(rt.accessibility_action("segment1", rune::accesskit::Action::Focus, None));
        rt.frame(0.0);
        assert!(rt.accessibility_action("segment1", rune::accesskit::Action::Click, None));
        rt.frame(0.0);
        assert_eq!(rt.state.feed.tab, StoryType::New);
        assert!(rt.elements().iter().any(|e| e.key.as_deref() == Some("segment1") && e.focused));
        route(&mut rt, rune::Key::Down);
        assert_eq!(rt.state.feed.tab, StoryType::Best);
    }

    #[test]
    fn arrows_during_a_pending_shortcut_do_not_navigate_the_tab_radios() {
        let mut s = app(true, SavedView::in_memory());
        s.request((11, 1), Ranking::Fresh);
        s.source = Arc::new(Source::Fake(crate::service::FakeService::fixture().with_delay(Duration::from_millis(50))));
        s.headless = false;
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.on_key = Some(on_key);
        rt.frame(0.0);
        route(&mut rt, rune::Key::Text("2".into()));
        route(&mut rt, rune::Key::Down);
        assert_eq!(rt.state.feed.tab, StoryType::New);
        assert!(rt.state.feed.status.loading, "worker delivery is held until pending input is checked");
        assert_eq!(rt.state.feed.selected, 0);
        rt.poll = Some(poll);
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while rt.state.feed.status.loading {
            std::thread::yield_now();
            rt.frame(0.0);
            assert!(std::time::Instant::now() < deadline);
        }
        route(&mut rt, rune::Key::Down);
        assert_eq!(rt.state.feed.tab, StoryType::New);
        assert_eq!(rt.state.feed.selected, 1);
    }

    /// Explicit renderer fixture: keeps GPU setup out of the ordinary unit-test suite.
    #[test]
    #[ignore = "Run explicitly with Metal access for loading visual/AX verification"]
    fn render_loading_shell_fixtures() {
        fn loading() -> App {
            let mut s = app(true, SavedView::in_memory());
            s.feed.status.loading = true;
            s
        }
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/ux-motion/rune-experiments/loading");
        std::fs::create_dir_all(&out).unwrap();
        for (size, name) in [("1180x760", "narrow"), ("1600x900", "wide")] {
            for theme in ["dark", "light"] {
                let stem = out.join(format!("{name}-{theme}"));
                let args = rune::tooling::Args::parse_from(vec!["loading-fixture".into(),
                    "--size".into(), size.into(), "--theme".into(), theme.into(),
                    "--at".into(), "0.1".into(), "--snap".into(), stem.with_extension("png").display().to_string(),
                    "--tree".into(), stem.with_extension("json").display().to_string()]);
                let spec = AppSpec::new("Hacker News", (1180.0, 760.0), loading, view);
                rune::tooling::headless(rune::App::new(spec), args);
                let tree = std::fs::read_to_string(stem.with_extension("json")).unwrap();
                assert!(tree.contains("Loading Top stories"));
                assert!(!tree.contains("story-1"), "skeleton must not invent interactive stories");
            }
        }
    }

    #[test]
    fn tab_arrivals_include_immediate_saved_and_latest_async_tab_only() {
        let mut s = app(true, SavedView::in_memory());
        s.request((11, 1), Ranking::Fresh);
        press(&mut s, rune::Key::Text("4".into()));
        assert_eq!(s.arrival, Arrival::Tab(1));
        assert!(!s.feed.status.loading);
        assert!(s.feed.stories.is_empty());
        let saved_revision = s.feed_revision;
        s.headless = false;
        s.source = Arc::new(Source::Fake(crate::service::FakeService::fixture()
            .with_delay(Duration::from_millis(20))));
        press(&mut s, rune::Key::Text("2".into()));
        press(&mut s, rune::Key::Text("3".into()));
        settle_feed(&mut s);
        assert_eq!(s.feed.stories[0].id, 201);
        assert_eq!(s.arrival, Arrival::Tab(1));
        assert_eq!(s.feed_revision, saved_revision + 1, "only the latest accepted tab arrives");
    }

    #[test]
    fn new_tab_resets_timeout_but_its_resize_preserves_deadline_and_arrival() {
        let mut s = app(true, SavedView::in_memory());
        s.request((11, 1), Ranking::Fresh);
        s.headless = false;
        s.source = Arc::new(Source::Fake(crate::service::FakeService::fixture()
            .with_delay(Duration::from_millis(100))));
        s.loading_since = Some(0.0);
        press(&mut s, rune::Key::Text("2".into()));
        poll(&mut s, 9.9, &mut Tx::default());
        assert_eq!(s.loading_since, Some(9.9));
        s.measured.set(Some((10, 1)));
        poll(&mut s, 10.1, &mut Tx::default());
        assert_eq!(s.loading_since, Some(9.9));
        assert_eq!(s.pending_arrival, Some(Arrival::Tab(1)));
        press(&mut s, rune::Key::Text("3".into()));
        poll(&mut s, 19.8, &mut Tx::default());
        assert_eq!(s.loading_since, Some(19.8));
        assert!(s.feed.status.loading && s.feed.status.error.is_none());
    }

    #[test]
    fn retry_clears_feed_error_and_settled_empty_is_distinct_from_loading() {
        let mut s = app(true, SavedView::in_memory());
        s.request((11, 1), Ranking::Fresh);
        if let Source::Fake(fake) = s.source.as_ref() { fake.fail_ranking(StoryType::New); }
        press(&mut s, rune::Key::Text("2".into()));
        assert!(s.feed.status.error.is_some());
        s.source = Arc::new(Source::Fake(crate::service::FakeService::empty()
            .with_delay(Duration::from_millis(10))));
        s.headless = false;
        s.refresh();
        assert!(s.feed.status.loading && s.feed.status.error.is_none());
        assert_eq!(s.arrival, Arrival::None, "retry must not replay stale tab motion");
        settle_feed(&mut s);
        assert!(!s.feed.status.loading && s.feed.status.error.is_none());
        assert!(s.feed.stories.is_empty());
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.frame(0.0);
        assert!(rt.elements().iter().any(|e| e.label.as_deref() == Some("No stories to show")));
        assert!(!rt.accessibility_tree().nodes.iter().any(|(_, node)| node.is_busy()));
    }

    #[test]
    #[ignore = "Explicit Metal-backed AppSpec workflow and motion-cause integration"]
    fn render_feed_arrival_workflows() {
        fn loaded() -> App {
            let mut s = app(true, SavedView::in_memory());
            s.request((11, 1), Ranking::Fresh);
            s.headless = false;
            s
        }
        fn loading() -> App {
            let mut s = app(false, SavedView::in_memory());
            s.feed.status.loading = true;
            s
        }
        fn reduced() -> App { let mut s = loaded(); s.reduced = true; s }
        fn reduced_loading() -> App { let mut s = loading(); s.reduced = true; s }
        fn saved() -> App { let mut s = loaded(); s.headless = true; s }
        fn wait_worker(s: &App) {
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while s.bridge.is_pending() {
                assert!(std::time::Instant::now() < deadline, "fixture worker must finish");
                std::thread::yield_now();
            }
        }
        fn initial(s: &mut App, now: f64, tx: &mut Tx) -> bool {
            if now < 0.2 { return false; }
            if s.requested.is_none() {
                s.request(s.measured.get().unwrap_or(DEFAULT_GRID), Ranking::Fresh);
                wait_worker(s);
                return true;
            }
            poll(s, now, tx)
        }
        fn page(s: &mut App, now: f64, tx: &mut Tx) -> bool {
            if now < 0.2 { return false; }
            if s.feed_revision == 0 && !s.feed.status.loading {
                on_key(s, &KeyEvent { key: rune::Key::Text("n".into()), mods: Default::default() }, tx);
                wait_worker(s);
                return true;
            }
            poll(s, now, tx)
        }
        fn tab(s: &mut App, now: f64, tx: &mut Tx) -> bool {
            if now < 0.2 { return false; }
            if s.feed.tab == StoryType::Top {
                on_key(s, &KeyEvent { key: rune::Key::Text("2".into()), mods: Default::default() }, tx);
                wait_worker(s);
                return true;
            }
            poll(s, now, tx)
        }
        fn tab_loading(s: &mut App, now: f64, tx: &mut Tx) -> bool {
            if now < 0.2 { return false; }
            if s.feed.tab == StoryType::Top {
                on_key(s, &KeyEvent { key: rune::Key::Text("2".into()), mods: Default::default() }, tx);
                wait_worker(s);
                tx.animated();
                return true;
            }
            if now < 0.8 { return false; }
            poll(s, now, tx)
        }
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/ux-motion/skeleton-handoff/arrivals");
        std::fs::create_dir_all(&out).unwrap();
        for (name, init, poller, group, reduced) in [
            ("initial", loading as fn() -> App, initial as fn(&mut App, f64, &mut Tx) -> bool, "feed-page-0", false),
            ("tab", loaded, tab, "feed-page-1", false),
            ("page", loaded, page, "feed-page-1", false),
            ("saved-empty", saved, poll, "feed-page-1", false),
            ("reduced-page", reduced, page, "feed-page-1", true),
            ("reduced-initial", reduced_loading, initial, "feed-page-0", true),
            ("tab-loading", loaded, tab_loading, "feed-placeholder", false),
            ("reduced-tab-loading", reduced, tab_loading, "feed-placeholder", true),
        ] {
            let captures = if name == "tab-loading" {
                vec![("1180x760", "dark", "0.25"), ("1180x760", "dark", "0.3"),
                     ("1180x760", "dark", "0.6"), ("1600x900", "light", "0.3")]
            } else if name == "tab" {
                vec![("1180x760", "dark", "0.25"), ("1180x760", "dark", "0.3"),
                     ("1180x760", "light", "0.25"), ("1180x760", "light", "0.3"),
                     ("1600x900", "dark", "0.25"), ("1600x900", "dark", "0.3"),
                     ("1600x900", "light", "0.25"), ("1600x900", "light", "0.3")]
            } else { vec![("1180x760", "dark", "0.3")] };
            for (size, theme, at) in captures {
            let stem = out.join(format!("{name}-{size}-{theme}-{}ms", (at.parse::<f64>().unwrap() * 1000.0) as usize));
            let mut argv = vec!["arrival-fixture".into(), "--size".into(), size.into(),
                "--theme".into(), theme.into(), "--at".into(), at.into(),
                "--snap".into(), stem.with_extension("png").display().to_string(),
                "--tree".into(), stem.with_extension("json").display().to_string(),
                "--trace".into(), stem.with_extension("jsonl").display().to_string()];
            if name == "saved-empty" {
                let script = out.join("saved-input.json");
                std::fs::write(&script, r#"[{"t":0.2,"type":"key","name":"4"}]"#).unwrap();
                argv.extend(["--script".into(), script.display().to_string()]);
            }
            let args = rune::tooling::Args::parse_from(argv);
            let mut spec = AppSpec::new("Hacker News", (1180.0, 760.0), init, view);
            spec.poll = Some(poller);
            spec.on_key = Some(on_key);
            rune::tooling::headless(rune::App::new(spec), args);
            let mut names = std::collections::HashMap::new();
            let mut moving = false;
            let mut fading_placeholder = false;
            let mut overlapping = false;
            let mut placeholder_rect = None;
            let mut viewport_rect = None;
            for line in std::fs::read_to_string(stem.with_extension("jsonl")).unwrap().lines() {
                let frame: serde_json::Value = serde_json::from_str(line).unwrap();
                for node in frame["m"].as_array().into_iter().flatten() {
                    names.insert(node["id"].as_u64().unwrap(), node["nm"].as_str().unwrap().to_string());
                }
                let mut incoming_partial = false;
                let mut outgoing_partial = false;
                for e in frame["e"].as_array().into_iter().flatten() {
                    let path = names.get(&e[0].as_u64().unwrap());
                    if path.is_some_and(|path| path.rsplit('/').next() == Some("feed-list")) {
                        let rect: Vec<_> = e.as_array().unwrap()[1..5].to_vec();
                        if let Some(before) = &viewport_rect { assert_eq!(&rect, before, "handoff must keep viewport geometry"); }
                        else { viewport_rect = Some(rect); }
                    }
                    if path.is_some_and(|path| path.rsplit('/').next() == Some("feed-placeholder")) {
                        let rect: Vec<_> = e.as_array().unwrap()[1..5].to_vec();
                        if let Some(before) = &placeholder_rect { assert_eq!(&rect, before, "outgoing placeholder must not travel"); }
                        else { placeholder_rect = Some(rect); }
                        let opacity = e[5].as_f64().unwrap();
                        outgoing_partial = opacity > 0.0 && opacity < 0.99;
                        fading_placeholder |= outgoing_partial;
                    }
                    if path.is_some_and(|path| path.rsplit('/').next() == Some(group)) {
                        let opacity = e[5].as_f64().unwrap();
                        incoming_partial = opacity > 0.2 && opacity < 0.99;
                        moving |= incoming_partial;
                    }
                }
                overlapping |= incoming_partial && outgoing_partial;
            }
            assert_eq!(moving, !reduced, "{name}: actual AppSpec async arrival motion");
            if name == "initial" || name == "tab" {
                assert!(fading_placeholder && overlapping, "{name}: outgoing placeholder and incoming content must share the opacity handoff");
            } else if name == "reduced-initial" {
                assert!(!fading_placeholder, "reduced motion removes the placeholder immediately");
            }
            }
        }
    }

    fn settle_feed(s: &mut App) {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while s.feed.status.loading {
            poll(s, 0.0, &mut Tx::default());
            assert!(std::time::Instant::now() < deadline, "fake feed must settle");
            std::thread::yield_now();
        }
    }

    #[test]
    fn tab_switch_resize_keeps_the_new_tabs_fresh_request() {
        let mut s = app(true, SavedView::in_memory());
        s.request((11, 1), Ranking::Fresh);
        s.source = Arc::new(Source::Fake(crate::service::FakeService::fixture()
            .with_delay(Duration::from_millis(50))));
        s.headless = false;
        press(&mut s, rune::Key::Text("2".into()));
        s.measured.set(Some((10, 1)));
        poll(&mut s, 0.0, &mut Tx::default());
        assert!(s.feed.status.loading, "resize must not settle the previous tab's cache");
        settle_feed(&mut s);
        assert_eq!(s.feed.tab, StoryType::New);
        assert_eq!(s.feed.stories.len(), 10);
        assert_eq!(s.feed.stories[0].id, 101);
        assert!(s.feed.id_snapshot.iter().all(|id| *id >= 101));
    }

    #[test]
    fn refresh_resize_keeps_fresh_ranking_and_updated_items() {
        let mut s = app(true, SavedView::in_memory());
        s.request((11, 1), Ranking::Fresh);
        let fake = crate::service::FakeService::fixture().with_delay(Duration::from_millis(50));
        fake.set_ranking(StoryType::Top, vec![9, 8, 7, 6]);
        let mut item = crate::fixtures::item(8, 9);
        item.title = "Fresh title after resize".into();
        fake.insert_item(item);
        s.source = Arc::new(Source::Fake(fake));
        s.headless = false;
        s.refresh();
        s.measured.set(Some((10, 1)));
        poll(&mut s, 0.0, &mut Tx::default());
        assert!(s.feed.status.loading, "resize must not cancel an explicit refresh");
        assert_eq!(s.feed.stories[0].id, 1, "held rows stay until the fresh window lands");
        settle_feed(&mut s);
        assert_eq!(s.feed.id_snapshot, vec![9, 8, 7, 6]);
        assert_eq!(s.feed.stories[0].title, "Fresh title after resize");
    }

    #[test]
    fn failed_tab_load_then_resize_never_reuses_the_previous_tabs_snapshot() {
        let mut s = app(true, SavedView::in_memory());
        s.request((11, 1), Ranking::Fresh);
        let fake = crate::service::FakeService::fixture().with_delay(Duration::from_millis(50));
        fake.fail_ranking(StoryType::New);
        s.source = Arc::new(Source::Fake(fake));
        s.headless = false;
        press(&mut s, rune::Key::Text("2".into()));
        settle_feed(&mut s);
        assert!(s.feed.status.error.is_some());
        s.measured.set(Some((10, 1)));
        poll(&mut s, 0.0, &mut Tx::default());
        settle_feed(&mut s);
        assert_eq!(s.feed.tab, StoryType::New);
        assert!(s.feed.stories.is_empty(), "a failed New load must not display cached Top rows");
        assert!(s.feed.status.error.is_some());
        assert!(s.feed.stories_for.is_none());
    }

    /// The inspector the window builds: 543 points, clamped to 360..=640, collapsed.
    fn inspector() -> PaneState {
        PaneState::new(INSPECTOR_WIDTH)
            .range(INSPECTOR_MIN, INSPECTOR_MAX)
            .collapsed()
    }

    fn long_discussion() -> App {
        let mut s = app(true, SavedView::in_memory());
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        let template = crate::fixtures::comments().remove(0);
        *fake.comments.lock().unwrap() = (10000..10244).map(|id| {
            let mut c = template.clone(); c.id = id; c.child_ids.clear();
            c.text = format!("Comment {id}: {}", "A mixed height paragraph. ".repeat(if id % 3 == 0 { 18 } else { 2 })); c
        }).collect();
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        s
    }

    #[test]
    fn discussion_scroll_keeps_frames_paced_until_it_is_still() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let mut app = long_discussion();
        assert_eq!(app.story.as_ref().unwrap().thread.len(), 244, "the synchronous fixture loaded its comments");
        let wakes = Arc::new(AtomicUsize::new(0));
        let wake_count = Arc::clone(&wakes);
        app.scroll_waker = Some(Arc::new(move || { wake_count.fetch_add(1, Ordering::Relaxed); }));
        let mut rt = rune::Runtime::new(app, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.poll = Some(poll);
        for _ in 0..120 { rt.frame(1.0 / 60.0); }
        assert_eq!(rt.state.scroll_pacing.get(), ScrollPacing::Idle, "initial layout settles");
        assert_eq!(wakes.load(Ordering::Relaxed), 0, "initial offset is not a scroll");

        let viewport = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("discussion-scroll")).unwrap().rect;
        let point = (viewport[0] + viewport[2] / 2.0, viewport[1] + viewport[3] / 2.0);
        let scroll_y = |rt: &rune::Runtime<App>| rt.elements().iter()
            .find(|e| e.key.as_deref() == Some("discussion-header")).unwrap().scrolled[1];
        let before = scroll_y(&rt);
        let commits = rt.stats.commits;
        rt.wheel(point.0, point.1, -5.0);
        rt.frame(1.0 / 120.0);
        assert!(scroll_y(&rt) > before, "the real discussion viewport scrolled");
        assert_eq!(rt.state.scroll_pacing.get(), ScrollPacing::Moved);
        assert_eq!(wakes.load(Ordering::Relaxed), 1, "the first movement wakes poll");
        assert_eq!(rt.stats.commits, commits, "the moving offset callback does not rebuild the view");
        rt.frame(1.0 / 120.0);
        let ScrollPacing::Until(first_deadline) = rt.state.scroll_pacing.get() else { panic!("scroll starts pacing") };
        assert!(rt.wake_deadline().is_some(), "pacing asks for another frame");
        assert_eq!(rt.stats.commits, commits, "the offset callback does not rebuild the view");

        rt.wheel(point.0, point.1, -5.0);
        rt.frame(1.0 / 120.0);
        rt.frame(1.0 / 120.0);
        assert!(matches!(rt.state.scroll_pacing.get(), ScrollPacing::Until(until) if until > first_deadline), "further scrolling extends pacing");
        assert_eq!(wakes.load(Ordering::Relaxed), 1, "an active cadence needs no extra wake");
        for _ in 0..20 { rt.frame(1.0 / 120.0); }
        assert_eq!(rt.state.scroll_pacing.get(), ScrollPacing::Idle, "pacing ends after the scroll rests");
        assert!(rt.wake_deadline().is_none(), "the idle window has no pacing wake");
        assert!(!rt.live_pending(), "the idle window has no live work pending");

        rt.wheel(point.0, point.1, -5.0);
        rt.frame(1.0 / 120.0);
        assert_eq!(wakes.load(Ordering::Relaxed), 2, "a later gesture wakes poll again");
        for _ in 0..20 { rt.frame(1.0 / 120.0); }
        assert_eq!(rt.state.scroll_pacing.get(), ScrollPacing::Idle);
        assert!(rt.wake_deadline().is_none() && !rt.live_pending(), "the window returns to idle again");
    }

    #[test]
    fn summary_and_source_scroll_return_to_idle() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        for source in [false, true] {
            let mut s = app(true, SavedView::in_memory());
            scene_story(&mut s);
            let story = s.story.as_mut().unwrap();
            if source {
                story.sources[0].text = "A captured comment with enough text to scroll. ".repeat(120);
                s.open_citation(0);
            } else {
                story.article_summary = crate::story::ArticleSummaryState::Done("A long article summary. ".repeat(120));
            }
            let wakes = Arc::new(AtomicUsize::new(0));
            let count = Arc::clone(&wakes);
            s.scroll_waker = Some(Arc::new(move || { count.fetch_add(1, Ordering::Relaxed); }));
            let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
            rt.poll = Some(poll);
            for _ in 0..120 { rt.frame(1.0 / 60.0); }
            assert_eq!(rt.state.scroll_pacing.get(), ScrollPacing::Idle);
            let viewport = if source { "source-viewer" } else { "summary-scroll" };
            let content = if source { "source-content" } else { "article-summary" };
            let elements = rt.elements();
            let [x, y, w, h] = elements.iter().find(|e| e.key.as_deref() == Some(viewport)).unwrap().rect;
            let before = elements.iter().find(|e| e.key.as_deref() == Some(content)).unwrap().rect[1];
            rt.wheel(x + w / 2.0, y + h / 2.0, -10.0);
            rt.frame(1.0 / 120.0);
            let after = rt.elements().iter().find(|e| e.key.as_deref() == Some(content)).unwrap().rect[1];
            assert!(after < before, "{viewport} must actually scroll");
            assert_eq!(rt.state.scroll_pacing.get(), ScrollPacing::Moved);
            assert!(wakes.load(Ordering::Relaxed) > 0, "{viewport} wakes the frame clock");
            for _ in 0..20 { rt.frame(1.0 / 120.0); }
            assert_eq!(rt.state.scroll_pacing.get(), ScrollPacing::Idle);
            assert!(rt.wake_deadline().is_none() && !rt.live_pending(), "{viewport} returns to idle");
        }
    }

    fn gated_summary_fixture() -> (App, tokio::sync::oneshot::Sender<()>, tokio::sync::oneshot::Sender<()>) {
        use std::sync::Mutex;
        use tokio::sync::oneshot;

        let (article_tx, article_rx) = oneshot::channel::<()>();
        let (discussion_tx, discussion_rx) = oneshot::channel::<()>();
        let article_rx = Mutex::new(Some(article_rx));
        let discussion_rx = Mutex::new(Some(discussion_rx));
        let mut fake = crate::service::FakeService::fixture();
        fake.article_summary = Arc::new(move |_| {
            let gate = article_rx.lock().unwrap().take().unwrap();
            Box::pin(async move {
                gate.await.unwrap();
                Ok(hn_lib::reader::Article { text: "The article summary arrived.".into() })
            })
        });
        fake.summary = Arc::new(move |_| {
            let gate = discussion_rx.lock().unwrap().take().unwrap();
            Box::pin(async move { gate.await.unwrap(); Ok(crate::fixtures::summary()) })
        });
        let mut s = app(false, SavedView::in_memory());
        s.source = Arc::new(Source::Fake(fake));
        s.config.api_key = "fixture".into();
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        poll_until(&mut s, |s| {
            let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
            *fake.article_summary_requests.lock().unwrap() == 1 && *fake.summary_requests.lock().unwrap() == 1
        });
        s.live = Live::Failed { story_id: crate::fixtures::STORY_ID, error: "fixture".into() };
        (s, article_tx, discussion_tx)
    }

    #[test]
    fn independent_summary_arrivals_move_only_the_ready_section() {
        for reduced in [false, true] {
            let (s, article_tx, discussion_tx) = gated_summary_fixture();
            let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
            rt.poll = Some(poll);
            rt.set_system_reduced_motion(reduced);
            rt.frame(0.0);

            let element = |rt: &rune::Runtime<App>, key: &str| rt.elements().into_iter()
                .find(|e| e.key.as_deref() == Some(key) && !e.exiting).unwrap();
            let wait_for = |rt: &mut rune::Runtime<App>, article: bool| {
                let deadline = std::time::Instant::now() + Duration::from_secs(3);
                while if article {
                    !matches!(rt.state.story.as_ref().unwrap().article_summary, crate::story::ArticleSummaryState::Done(_))
                } else {
                    !matches!(rt.state.story.as_ref().unwrap().summary, SummaryState::Done(_))
                } {
                    rt.frame(0.0);
                    assert!(std::time::Instant::now() < deadline, "summary result did not land");
                    std::thread::yield_now();
                }
            };
            article_tx.send(()).unwrap();
            wait_for(&mut rt, true);
            rt.frame(0.05);
            let article_50 = element(&rt, "article-summary-result");
            let discussion = element(&rt, "discussion-summary-result");
            rt.frame(0.05);
            let article_100 = element(&rt, "article-summary-result");
            if reduced {
                assert_eq!((article_50.opacity, article_100.opacity), (1.0, 1.0));
                assert_eq!(article_50.rect, article_50.target);
            } else {
                assert!(0.4 < article_50.opacity && article_50.opacity < article_100.opacity && article_100.opacity < 1.0,
                    "article at 50/100ms: {} {}", article_50.opacity, article_100.opacity);
                assert!(discussion.opacity < 1.0, "pending discussion must not enter with the article");
            }
            assert!(!rt.elements().iter().any(|e| e.exiting && e.text.is_some() && e.opacity > 0.001
                && e.path.contains("summary-scroll")), "outgoing loading text must not overlap");

            discussion_tx.send(()).unwrap();
            wait_for(&mut rt, false);
            assert!(rt.state.toast.is_none(), "a visible discussion result needs no toast");
            rt.frame(0.05);
            let discussion_50 = element(&rt, "discussion-summary-result");
            rt.frame(0.05);
            let discussion_100 = element(&rt, "discussion-summary-result");
            if reduced {
                assert_eq!((discussion_50.opacity, discussion_100.opacity), (1.0, 1.0));
                assert_eq!(discussion_50.rect, discussion_50.target);
            } else {
                assert!(0.4 < discussion_50.opacity && discussion_50.opacity < discussion_100.opacity && discussion_100.opacity < 1.0,
                    "discussion at 50/100ms: {} {}", discussion_50.opacity, discussion_100.opacity);
            }
        }
    }

    #[test]
    fn returning_to_a_replacement_without_citations_restores_summary_focus() {
        let mut s = app(true, SavedView::in_memory());
        scene_story(&mut s);
        s.open_citation(0);
        let story = s.story.as_mut().unwrap();
        story.retry(true);
        let mut replacement = crate::fixtures::summary();
        for point in &mut replacement.points { point.sources.clear(); }
        story.land_update(StoryUpdate::Summary {
            story_id: story.id(), summary: Ok(replacement), sources: story.sources.clone(),
        });
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.on_key = Some(on_key);
        rt.frame(0.0);
        route(&mut rt, rune::Key::Escape);
        assert!(rt.state.story.as_ref().unwrap().source.is_none());
        assert!(rt.elements().iter().any(|e| e.key.as_deref() == Some("summary-scroll") && e.focused));
    }

    #[test]
    fn a_folded_summary_still_announces_its_result() {
        let (mut s, article, discussion) = gated_summary_fixture();
        s.story.as_mut().unwrap().summary_open = false;
        discussion.send(()).unwrap();
        poll_until(&mut s, |s| matches!(s.story.as_ref().unwrap().summary, SummaryState::Done(_)));
        assert_eq!(s.toast.as_ref().map(|notice| notice.message.as_str()), Some("Discussion summary ready"));
        drop(s);
        drop(article);
    }

    #[test]
    #[ignore = "Explicit Metal-backed captures of the actual async summary handoff"]
    fn render_summary_arrival_frames() {
        use std::cell::RefCell;
        use tokio::sync::oneshot::Sender;

        thread_local! {
            static GATES: RefCell<[Option<Sender<()>>; 2]> = const { RefCell::new([None, None]) };
        }
        fn initial() -> App {
            let (s, article, discussion) = gated_summary_fixture();
            GATES.with(|gates| *gates.borrow_mut() = [Some(article), Some(discussion)]);
            s
        }
        fn reduced() -> App { let mut s = initial(); s.reduced = true; s }
        fn gated_poll(s: &mut App, now: f64, tx: &mut Tx) -> bool {
            let gate = GATES.with(|gates| {
                let mut gates = gates.borrow_mut();
                if now >= 0.7 && gates[1].is_some() { gates[1].take().map(|g| (g, false)) }
                else if now >= 0.2 && gates[0].is_some() { gates[0].take().map(|g| (g, true)) }
                else { None }
            });
            if let Some((gate, article)) = gate {
                gate.send(()).unwrap();
                let deadline = std::time::Instant::now() + Duration::from_secs(3);
                let mut changed = false;
                loop {
                    changed |= poll(s, now, tx);
                    let story = s.story.as_ref().unwrap();
                    let done = if article { matches!(story.article_summary, crate::story::ArticleSummaryState::Done(_)) }
                        else { matches!(story.summary, SummaryState::Done(_)) };
                    if done { return changed; }
                    assert!(std::time::Instant::now() < deadline, "gated result did not land");
                    std::thread::yield_now();
                }
            }
            poll(s, now, tx)
        }
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/ux-motion/reading-arrivals");
        std::fs::create_dir_all(&out).unwrap();
        for (name, init, is_reduced) in [("normal", initial as fn() -> App, false), ("reduced", reduced, true)] {
            for theme in ["dark", "light"] {
                for (section, at) in [("article", "0.25"), ("article", "0.30"), ("discussion", "0.75"), ("discussion", "0.80")] {
                    let stem = out.join(format!("{section}-{name}-{theme}-{}", at.replace('.', "")));
                    let args = rune::tooling::Args::parse_from(vec!["reading-arrival".into(),
                        "--size".into(), "1180x760".into(), "--scale".into(), "1".into(),
                        "--theme".into(), theme.into(), "--at".into(), at.into(),
                        "--snap".into(), stem.with_extension("png").display().to_string(),
                        "--tree".into(), stem.with_extension("json").display().to_string(),
                        "--trace".into(), stem.with_extension("jsonl").display().to_string()]);
                    let mut spec = AppSpec::new("Hacker News", (1180.0, 760.0), init, view);
                    spec.poll = Some(gated_poll);
                    rune::tooling::headless(rune::App::new(spec), args);
                    let tree: serde_json::Value = serde_json::from_slice(&std::fs::read(stem.with_extension("json")).unwrap()).unwrap();
                    let key = format!("{section}-summary-result");
                    let node = tree["elements"].as_array().unwrap().iter().find(|e| e["key"] == key).unwrap();
                    let opacity = node["opacity"].as_f64().unwrap();
                    if is_reduced { assert_eq!(opacity, 1.0); }
                    else { assert!(opacity > 0.4 && opacity < 1.0, "{section} {at}: {opacity}"); }
                }
            }
        }
    }

    #[test]
    fn citation_navigation_moves_as_one_surface_and_preserves_discussion() {
        for reduced in [false, true] {
            let mut s = app(true, SavedView::in_memory());
            scene_story(&mut s);
            let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
            rt.on_key = Some(on_key);
            rt.set_system_reduced_motion(reduced);
            rt.frame(0.0);
            route(&mut rt, rune::Key::Tab);
            rt.key(KeyEvent { key: rune::Key::Enter, mods: Default::default() });
            rt.frame(0.0);
            rt.frame(0.05);
            let find = |rt: &rune::Runtime<App>, key: &str| rt.elements().into_iter()
                .find(|e| e.key.as_deref() == Some(key) && !e.exiting).unwrap();
            let source_50 = find(&rt, "source-viewer");
            let hidden = find(&rt, "inspector");
            assert_eq!(hidden.opacity, 0.0);
            assert!(hidden.inert);
            rt.frame(0.05);
            let source_100 = find(&rt, "source-viewer");
            if reduced {
                assert_eq!(source_50.opacity, 1.0);
                assert_eq!(source_50.rect, source_50.target);
            } else {
                assert!(source_50.rect[0] > source_100.rect[0] && source_100.rect[0] > source_100.target[0] + 1.0,
                    "source should still move at 100ms: {} {} {}", source_50.rect[0], source_100.rect[0], source_100.target[0]);
                assert!(source_50.opacity < source_100.opacity && source_100.opacity < 1.0);
            }
            rt.key(KeyEvent { key: rune::Key::Escape, mods: Default::default() });
            rt.frame(0.0);
            rt.frame(0.05);
            let back_50 = find(&rt, "inspector");
            rt.frame(0.05);
            let back_100 = find(&rt, "inspector");
            assert!(!rt.elements().iter().any(|e| e.key.as_deref() == Some("source-viewer") && e.opacity > 0.001));
            if reduced {
                assert_eq!(back_50.opacity, 1.0);
                assert_eq!(back_50.rect, back_50.target);
            } else {
                assert!(back_50.rect[0] < back_100.rect[0] && back_100.rect[0] < back_100.target[0] - 1.0);
                assert!(back_50.opacity < back_100.opacity && back_100.opacity < 1.0);
            }
        }
    }

    #[test]
    fn unavailable_reply_group_retries_from_pointer_and_keyboard() {
        for activation in [None, Some(rune::Key::Enter), Some(rune::Key::Right)] {
            let mut s = app(true, SavedView::in_memory());
            let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
            let mut parent = crate::fixtures::comments().remove(0);
            parent.deleted = true;
            parent.child_ids = vec![9010];
            let child = crate::fixtures::thread().into_iter().find(|c| c.id == 9010).unwrap();
            *fake.comments.lock().unwrap() = vec![parent, child];
            s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
            s.story.as_mut().unwrap().thread.merge_replies(901, &[9010], Err("Offline".into()));
            let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
            rt.on_key = Some(on_key);
            rt.frame(0.0);
            let elements = rt.elements();
            let control = elements.iter().find(|e| e.key.as_deref() == Some("comment-group-901")).unwrap();
            assert_eq!(control.label.as_deref(), Some("Retry replies: 1 reply"));
            assert!(!elements.iter().any(|e| e.key.as_deref() == Some("comment-load-901-true")));
            if let Some(key) = activation {
                route(&mut rt, key);
            } else {
                let [x, y, w, h] = control.rect;
                rt.pointer_down(x + w / 2.0, y + h / 2.0);
                rt.pointer_up(x + w / 2.0, y + h / 2.0);
                rt.frame(0.3);
            }
            let story = rt.state.story.as_ref().unwrap();
            assert_eq!(story.thread.children_of(901), &[9010]);
            assert!(story.thread.reply_batch(901, true).is_empty());
        }
    }

    #[test]
    fn keyboard_return_to_root_then_pointer_selection_keeps_the_visible_row() {
        for reduced in [false, true] {
            let mut s = app(true, SavedView::in_memory());
            scene_comments(&mut s);
            let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
            rt.set_system_reduced_motion(reduced);
            rt.on_key = Some(on_key);
            rt.poll = Some(poll);
            rt.frame(0.0);
            route(&mut rt, rune::Key::Text("j".into()));
            route(&mut rt, rune::Key::Text("u".into()));
            assert_eq!(rt.state.story.as_ref().unwrap().thread.cursor(), Some(901));
            let root = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("comment-901")).unwrap();
            assert!(root.visible[3] > 0.0, "the returned parent is revealed");
            let reply = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("comment-9010")).unwrap();
            let [x, y, w, h] = reply.visible;
            rt.pointer_move(x + w / 2.0, y + h / 2.0);
            rt.frame(0.05);
            rt.frame(0.1);
            assert_eq!(rt.state.story.as_ref().unwrap().thread.cursor(), Some(9010));
            let after = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("comment-9010")).unwrap();
            assert_eq!(after.scrolled, reply.scrolled, "the pointer must not trigger a stale reveal");
        }
    }

    #[test]
    fn discussion_boundary_key_does_not_pull_the_reader_back() {
        let mut rt = rune::Runtime::new(long_discussion(), view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.on_key = Some(on_key);
        rt.poll = Some(poll);
        rt.frame(0.0);
        rt.update(|_, tx| tx.scroll_to_y("discussion-scroll", 1000.0));
        for _ in 0..40 { rt.frame(1.0 / 60.0); }
        let y = |rt: &rune::Runtime<App>| rt.elements().iter()
            .find(|e| e.key.as_deref() == Some("discussion-header")).unwrap().scrolled[1];
        let before = y(&rt);
        assert!(before > 0.0, "the reader scrolled away from the first comment");
        route(&mut rt, rune::Key::Text("k".into()));
        for _ in 0..20 { rt.frame(1.0 / 60.0); }
        assert!((y(&rt) - before).abs() < 0.5, "a boundary key must not reveal the unchanged cursor");
    }

    #[test]
    #[ignore = "Explicit Metal-backed long-discussion mounting and reveal verification"]
    fn render_long_discussion_workflows() {
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/ux-motion/comments");
        std::fs::create_dir_all(&out).unwrap();
        fn reduced() -> App { let mut s = long_discussion(); s.reduced = true; s }
        for (name, script_text, selected) in [("top", "[]", 10000), ("bottom", r#"[{"t":0.2,"type":"key","name":"G"}]"#, 10243),
            ("resize-reduced", r#"[{"t":0.1,"type":"resize","dw":400,"dh":140,"dur":0.1},{"t":0.3,"type":"key","name":"G"}]"#, 10243)] {
            let script = out.join(format!("{name}-script.json"));
            std::fs::write(&script, script_text).unwrap();
            let stem = out.join(name);
            let args = rune::tooling::Args::parse_from(vec!["comments-fixture".into(),
                "--size".into(), "1180x760".into(), "--theme".into(), "dark".into(),
                "--script".into(), script.display().to_string(), "--at".into(), "0.8".into(),
                "--snap".into(), stem.with_extension("png").display().to_string(),
                "--tree".into(), stem.with_extension("json").display().to_string()]);
            let mut spec = AppSpec::new("Hacker News", (1180.0,760.0), if name == "resize-reduced" { reduced } else { long_discussion }, view);
            spec.on_key = Some(on_key);
            rune::tooling::headless(rune::App::new(spec), args);
            let tree: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(stem.with_extension("json")).unwrap()).unwrap();
            let elements = tree["elements"].as_array().unwrap();
            let rows: Vec<_> = elements.iter().filter(|e| e["key"].as_str().is_some_and(|key| key.strip_prefix("comment-").is_some_and(|id| id.parse::<i32>().is_ok()))).collect();
            assert!(rows.len() < 40, "244 mixed-height rows must not all mount: {}", rows.len());
            assert!(rows.iter().any(|e| e["key"] == format!("comment-{selected}")), "keyboard reveal mounts the selected offscreen row");
            let absent = if selected == 10000 { 10243 } else { 10000 };
            assert!(!elements.iter().any(|e| e["key"] == format!("comment-text-{absent}")), "offscreen text is not mounted");
        }
    }

    #[test]
    #[ignore = "Explicit Metal-backed continuous discussion scroll profile"]
    fn render_continuous_discussion_scroll() {
        use std::io::BufRead;

        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/ux-motion/comments");
        std::fs::create_dir_all(&out).unwrap();
        let script = out.join("continuous-scroll-script.json");
        std::fs::write(&script, r#"[
            {"t":0.5,"type":"scroll","key":"discussion-scroll","dy":-1800,"dur":1.4},
            {"t":2.3,"type":"scroll","key":"discussion-scroll","dy":3000,"dur":1.4},
            {"t":4.1,"type":"scroll","key":"discussion-scroll","dy":-1800,"dur":1.4},
            {"t":5.9,"type":"scroll","key":"discussion-scroll","dy":3000,"dur":1.4}
        ]"#).unwrap();
        let stem = out.join("continuous-scroll");
        let trace = stem.with_extension("trace.jsonl");
        let args = rune::tooling::Args::parse_from(vec!["comments-scroll".into(),
            "--size".into(), "1180x760".into(), "--theme".into(), "dark".into(),
            "--script".into(), script.display().to_string(), "--at".into(), "7.8".into(),
            "--snap".into(), stem.with_extension("png").display().to_string(),
            "--tree".into(), stem.with_extension("json").display().to_string(),
            "--trace".into(), trace.display().to_string()]);
        let mut spec = AppSpec::new("Hacker News", (1180.0, 760.0), long_discussion, view);
        spec.on_key = Some(on_key);
        rune::tooling::headless(rune::App::new(spec), args);

        let mut discussion = None;
        let mut reached = [false; 4];
        for line in std::io::BufReader::new(std::fs::File::open(trace).unwrap()).lines() {
            let frame: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
            for meta in frame["m"].as_array().into_iter().flatten() {
                if meta["nm"].as_str().is_some_and(|name| name.ends_with("/discussion-scroll/discussion")) {
                    discussion = meta["id"].as_u64();
                }
            }
            for offset in frame["sc"].as_array().into_iter().flatten() {
                if offset[0].as_u64() != discussion || discussion.is_none() { continue; }
                let (Some(t), Some(y)) = (frame["t"].as_f64(), offset[2].as_f64()) else { continue; };
                if (500.0..=2000.0).contains(&t) && y > 1000.0 { reached[0] = true; }
                if (2300.0..=3800.0).contains(&t) && y < 2.0 { reached[1] = true; }
                if (4100.0..=5600.0).contains(&t) && y > 1000.0 { reached[2] = true; }
                if (5900.0..=7400.0).contains(&t) && y < 2.0 { reached[3] = true; }
            }
        }
        assert_eq!(reached, [true; 4], "both cold and warm swipes must move down and back up");
    }

    #[test]
    fn native_story_ownership_resets_changed_ids_and_retains_same_story() {
        let mut s = app(true, SavedView::in_memory());
        let a = crate::fixtures::item(0, 1);
        s.open_story(a.clone());
        s.live = Live::Ready { story_id: 1 };
        s.live_bounds = Some([10.0, 20.0, 400.0, 500.0]);
        s.live_shown = true;
        s.close_story();
        assert!(!s.live_shown, "leaving hides the child synchronously");
        assert_eq!(s.live, Live::Ready { story_id: 1 });
        s.open_story(a.clone());
        assert_eq!(s.live, Live::Ready { story_id: 1 });
        assert_eq!(s.live_bounds, None, "remounted region must supply fresh bounds");
        s.live_rect([30.0, 40.0, 500.0, 600.0]);
        assert_eq!(s.live_bounds, Some([30.0, 40.0, 500.0, 600.0]));
        s.open_story(a.clone());
        assert!(s.live_bounds.is_some(), "same mounted identity retains valid layout");
        let mut b = a; b.id = 2;
        s.live_shown = true;
        s.open_story(b.clone());
        assert_eq!(s.live, Live::Idle, "different IDs reset even with the same URL");
        assert!(!s.live_shown);
        assert_eq!(s.live_bounds, None);
        s.live = Live::Failed { story_id: 2, error: "No host".into() };
        s.open_story(b);
        assert!(matches!(s.live, Live::Failed { story_id: 2, .. }));
        s.open_story(crate::fixtures::item(2, 3));
        assert_eq!(s.live, Live::Idle, "a new story can retry child construction");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn uncommitted_native_article_has_an_accessible_placeholder_and_current_layout_key() {
        let mut s = app(true, SavedView::in_memory());
        s.open_story(crate::fixtures::item(0, 1));
        s.headless = false;
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.frame(0.0);
        rt.frame(0.0);
        let elements = rt.elements();
        let region = elements.iter().find(|e| e.key.as_deref() == Some("article-live-1")).unwrap();
        assert!(region.rect[2] > 0.0 && region.rect[3] > 0.0);
        let status = elements.iter().find(|e| e.key.as_deref() == Some("article-loading")).unwrap();
        assert_eq!(status.role, "status");
        assert_eq!(rt.accessibility_tree().nodes.iter().filter(|(_, node)| node.is_busy()).count(), 1);
        assert!(rt.elements().iter().any(|e| e.key.as_deref() == Some("article-open-original") && e.label.as_deref() == Some("Open in browser")));
        assert!(rt.state.live_bounds.is_some(), "loading preserves the native layout reporter");
    }

    #[test]
    fn replies_use_actual_child_ids_and_never_change_the_summary_capture() {
        let mut s = app(true, SavedView::in_memory());
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        let story = s.story.as_ref().unwrap();
        assert_eq!(story.thread.len(), 5);
        let sources: Vec<_> = story.sources.iter().map(|s| (s.id, s.text.clone())).collect();
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        assert!(fake.comment_requests.lock().unwrap().is_empty(), "no reply prefetch");
        let summaries = *fake.summary_requests.lock().unwrap();
        s.activate_comment(901);
        assert_eq!(s.story.as_ref().unwrap().thread.children_of(901), &[9010, 9011]);
        s.activate_comment(9010);
        let story = s.story.as_ref().unwrap();
        assert_eq!(story.thread.children_of(9010), &[90100, 90101]);
        assert_eq!(story.thread.tree_depth(90100), Some(2));
        assert_eq!(story.sources.iter().map(|s| (s.id, s.text.clone())).collect::<Vec<_>>(), sources, "lazy replies do not alter citation identity");
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        assert_eq!(*fake.comment_requests.lock().unwrap(), vec![vec![9010, 9011], vec![90100, 90101]]);
        assert_eq!(*fake.summary_requests.lock().unwrap(), summaries);
    }

    #[test]
    fn every_root_is_captured_for_summary_and_successful_empty_discussion_is_distinct() {
        let mut s = app(true, SavedView::in_memory());
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        let template = crate::fixtures::comments().remove(0);
        *fake.comments.lock().unwrap() = (10000..10244).map(|id| {
            let mut c = template.clone(); c.id = id; c.child_ids.clear(); c
        }).collect();
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        let story = s.story.as_ref().unwrap();
        assert_eq!(story.thread.roots(), 244);
        assert_eq!(story.sources.len(), 244, "summary capture has no arbitrary root cap");
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        assert!(fake.comment_requests.lock().unwrap().is_empty());
        assert_eq!(*fake.summary_requests.lock().unwrap(), 1);
        fake.comments.lock().unwrap().clear();
        s.refresh_comments();
        let story = s.story.as_ref().unwrap();
        assert_eq!(story.discussion, Some(Ok(())));
        assert_eq!(story.thread.roots(), 244, "refresh retains previously loaded roots");
        assert_eq!(story.sources.len(), 244, "refresh preserves summary citations");
        let mut empty = app(true, SavedView::in_memory());
        let Source::Fake(fake) = empty.source.as_ref() else { unreachable!() };
        fake.comments.lock().unwrap().clear();
        empty.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        let story = empty.story.as_ref().unwrap();
        assert_eq!(story.discussion, Some(Ok(())));
        assert!(story.thread.is_empty());
        assert!(story.sources.is_empty());
    }

    #[test]
    fn refresh_adds_roots_and_replies_without_replacing_summary_or_reading_position() {
        let mut s = app(true, SavedView::in_memory());
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        s.activate_comment(901);
        let story = s.story.as_mut().unwrap();
        assert!(story.thread.reroot(901));
        assert!(story.thread.select(9010));
        assert!(story.thread.fold());
        let before = story.summary.readable().unwrap().clone();
        let sources: Vec<_> = story.sources.iter().map(|s| (s.id, s.text.clone())).collect();
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        let discussions = *fake.summary_requests.lock().unwrap();
        let articles = *fake.article_summary_requests.lock().unwrap();
        let mut new_root = crate::fixtures::comments().remove(0);
        new_root.id = 987654;
        new_root.child_ids.clear();
        let mut comments = fake.comments.lock().unwrap();
        comments.iter_mut().find(|c| c.id == 901).unwrap().child_ids.push(987655);
        comments.push(new_root);
        drop(comments);
        s.refresh_comments();
        let story = s.story.as_ref().unwrap();
        assert!(story.thread.node(987654).is_some());
        assert_eq!(story.thread.children_of(901), &[9010, 9011]);
        assert!(story.thread.reply_batch(901, false).contains(&987655));
        assert_eq!(story.thread.cursor(), Some(9010));
        assert_eq!(story.thread.root(), Some(901));
        assert!(story.thread.focus());
        assert!(story.thread.is_folded(9010));
        assert_eq!(story.summary.readable(), Some(&before));
        assert_eq!(story.sources.iter().map(|s| (s.id, s.text.clone())).collect::<Vec<_>>(), sources);
        assert!(story.current_sources.iter().any(|s| s.id == 987654));
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        assert_eq!(*fake.summary_requests.lock().unwrap(), discussions);
        assert_eq!(*fake.article_summary_requests.lock().unwrap(), articles);
        s.close_story();
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        assert_eq!(*fake.summary_requests.lock().unwrap(), discussions);
        assert_eq!(*fake.article_summary_requests.lock().unwrap(), articles);
        let story = s.story.as_ref().unwrap();
        assert_eq!(story.summary.readable(), Some(&before));
        assert_eq!(story.sources.iter().map(|s| (s.id, s.text.clone())).collect::<Vec<_>>(), sources);
        assert!(story.current_sources.iter().any(|s| s.id == 987654));
        s.retry_summary();
        let story = s.story.as_ref().unwrap();
        assert_eq!(story.thread.cursor(), Some(9010));
        assert_eq!(story.thread.children_of(901), &[9010, 9011]);
        assert!(matches!(story.article_summary, crate::story::ArticleSummaryState::Done(_)));
        assert!(matches!(story.summary, SummaryState::Done(_)));
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        assert_eq!(*fake.summary_requests.lock().unwrap(), discussions + 1);
        assert_eq!(*fake.article_summary_requests.lock().unwrap(), articles + 1);
    }

    #[test]
    fn failed_comment_refresh_leaves_the_existing_tree_and_summaries_readable() {
        let mut s = app(true, SavedView::in_memory());
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        s.activate_comment(901);
        let before = s.story.as_ref().unwrap().summary.readable().unwrap().clone();
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        let calls = *fake.summary_requests.lock().unwrap();
        fake.failing_comments.lock().unwrap().insert(crate::fixtures::STORY_ID);
        s.refresh_comments();
        let story = s.story.as_ref().unwrap();
        assert!(!story.refreshing_comments);
        assert!(story.refresh_error.is_some());
        assert_eq!(story.thread.children_of(901), &[9010, 9011]);
        assert_eq!(story.summary.readable(), Some(&before));
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        assert_eq!(*fake.summary_requests.lock().unwrap(), calls);
    }

    #[test]
    fn refresh_updates_loaded_nested_replies_and_keeps_new_children_lazy() {
        let mut s = app(true, SavedView::in_memory());
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        s.activate_comment(901);
        s.activate_comment(9010);
        let story = s.story.as_mut().unwrap();
        assert!(story.thread.select(90100));
        assert!(story.thread.fold());
        let pending = story.request_replies(90100, false).unwrap();
        let before = story.summary.readable().unwrap().clone();
        let sources: Vec<_> = story.sources.iter().map(|source| (source.id, source.text.clone())).collect();
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        let summaries = *fake.summary_requests.lock().unwrap();
        let mut comments = fake.comments.lock().unwrap();
        let nested = comments.iter_mut().find(|comment| comment.id == 90100).unwrap();
        nested.text = "Updated nested reply".into();
        nested.child_ids.push(987655);
        drop(comments);

        s.refresh_comments();
        let story = s.story.as_mut().unwrap();
        assert_eq!(story.thread.node(90100).unwrap().text, "Updated nested reply");
        assert_eq!(story.pending_reply.as_ref(), Some(&pending));
        assert_eq!(story.thread.cursor(), Some(90100));
        assert!(story.thread.is_folded(90100));
        assert_eq!(story.summary.readable(), Some(&before));
        assert_eq!(story.sources.iter().map(|source| (source.id, source.text.clone())).collect::<Vec<_>>(), sources);
        let replies = crate::fixtures::replies(&pending.ids);
        assert!(story.land_replies(crate::bridge::ReplyFilled { request: pending, comments: Ok(replies) }));
        assert_eq!(story.thread.children_of(90100), &[901000]);
        assert_eq!(story.thread.reply_batch(90100, false), vec![987655]);
        assert!(story.thread.node(987655).is_none());
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        let requests = fake.comment_requests.lock().unwrap();
        let refreshed = requests.last().unwrap();
        assert!(refreshed.contains(&90100));
        assert!(!refreshed.contains(&901000) && !refreshed.contains(&987655), "unloaded replies stay lazy");
        assert_eq!(*fake.summary_requests.lock().unwrap(), summaries);
    }

    #[test]
    fn partial_comment_refresh_keeps_failed_nodes_and_applies_successes() {
        let mut s = app(true, SavedView::in_memory());
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        s.activate_comment(901);
        let original = s.story.as_ref().unwrap().thread.node(9010).unwrap().text.clone();
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        {
            let mut comments = fake.comments.lock().unwrap();
            comments.iter_mut().find(|comment| comment.id == 9010).unwrap().text = "Unavailable update".into();
            comments.iter_mut().find(|comment| comment.id == 9011).unwrap().text = "Available update".into();
        }
        fake.failing_comments.lock().unwrap().insert(9010);
        s.refresh_comments();
        let story = s.story.as_ref().unwrap();
        assert_eq!(story.thread.node(9010).unwrap().text, original);
        assert_eq!(story.thread.node(9011).unwrap().text, "Available update");
        assert!(story.refresh_error.as_deref().unwrap().contains("1 loaded reply"));

        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        fake.failing_comments.lock().unwrap().clear();
        fake.failing_comments.lock().unwrap().insert(crate::fixtures::STORY_ID);
        s.refresh_comments();
        let story = s.story.as_ref().unwrap();
        assert_eq!(story.thread.node(9010).unwrap().text, "Unavailable update", "loaded replies can refresh even when roots fail");
        assert!(story.refresh_error.is_some(), "the root failure remains visible");
        assert_eq!(story.discussion, Some(Ok(())));
    }

    #[test]
    fn article_summary_arrives_before_discussion_and_survives_a_refresh_during_generation() {
        use std::sync::Mutex;
        use tokio::sync::oneshot;
        let (release, gate) = oneshot::channel();
        let gate = Mutex::new(Some(gate));
        let mut fake = crate::service::FakeService::fixture();
        fake.summary = Arc::new(move |_| {
            let gate = gate.lock().unwrap().take().unwrap();
            Box::pin(async move { let _ = gate.await; Ok(crate::fixtures::summary()) })
        });
        let mut s = app(false, SavedView::in_memory());
        s.source = Arc::new(Source::Fake(fake));
        s.config.api_key = "fixture".into();
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        poll_until(&mut s, |s| matches!(s.story.as_ref().unwrap().article_summary, crate::story::ArticleSummaryState::Done(_))
            && matches!(s.story.as_ref().unwrap().summary, SummaryState::Generating { .. }));
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        let mut new_root = crate::fixtures::comments().remove(0);
        new_root.id = 987654;
        new_root.child_ids.clear();
        fake.comments.lock().unwrap().push(new_root);
        s.refresh_comments();
        poll_until(&mut s, |s| s.story.as_ref().unwrap().thread.node(987654).is_some());
        release.send(()).unwrap();
        poll_until(&mut s, |s| matches!(s.story.as_ref().unwrap().summary, SummaryState::Done(_)));
        let story = s.story.as_ref().unwrap();
        assert!(story.thread.node(987654).is_some());
        assert!(story.current_sources.iter().any(|source| source.id == 987654));
        assert!(!story.sources.iter().any(|source| source.id == 987654), "summary retains its original capture");
    }

    #[test]
    fn reopening_reuses_pending_and_completed_summaries_across_stories() {
        use std::sync::{Mutex, atomic::{AtomicUsize, Ordering}};
        use tokio::sync::oneshot;
        let (article_release, article_gate) = oneshot::channel();
        let (discussion_release, discussion_gate) = oneshot::channel();
        let article_gate = Mutex::new(Some(article_gate));
        let discussion_gate = Mutex::new(Some(discussion_gate));
        let captures = Arc::new(AtomicUsize::new(0));
        let mut fake = crate::service::FakeService::fixture();
        let captured = captures.clone();
        fake.article = Arc::new(move |_| {
            captured.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(("captured".into(), false)) })
        });
        fake.article_summary = Arc::new(move |id| {
            let gate = (id == 1).then(|| article_gate.lock().unwrap().take().unwrap());
            Box::pin(async move {
                if let Some(gate) = gate { gate.await.unwrap(); }
                Ok(hn_lib::reader::Article { text: format!("article {id}") })
            })
        });
        fake.summary = Arc::new(move |id| {
            let gate = (id == 1).then(|| discussion_gate.lock().unwrap().take().unwrap());
            Box::pin(async move {
                if let Some(gate) = gate { gate.await.unwrap(); }
                let mut summary = crate::fixtures::summary();
                summary.overview = format!("summary {id}");
                Ok(summary)
            })
        });
        let mut s = app(false, SavedView::in_memory());
        s.source = Arc::new(Source::Fake(fake));
        s.config.api_key = "fixture".into();
        let a = crate::fixtures::item(0, 1);
        let b = crate::fixtures::item(1, 2);
        s.open_story(a.clone());
        poll_until(&mut s, |s| {
            let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
            *fake.article_summary_requests.lock().unwrap() == 1 && *fake.summary_requests.lock().unwrap() == 1
        });
        let dir = temp_dir("settings-during-paid-work");
        let mut settings = Settings::open(&s.config, Some(dir.join("summary.json")), false);
        settings.set_key("new-key"); settings.set_profile(hn_lib::reader::Profile::Thorough);
        s.settings = Some(settings); s.apply_settings(false);
        assert!(s.settings.is_none() && s.config.key() == "new-key");
        std::fs::remove_dir_all(dir).unwrap();
        s.close_story();
        s.open_story(a.clone());
        assert!(s.story.as_ref().unwrap().summary.is_generating());
        s.open_story(b.clone());
        poll_until(&mut s, |s| !s.bridge.is_story_pending());
        article_release.send(()).unwrap();
        discussion_release.send(()).unwrap();
        s.open_story(a.clone());
        poll_until(&mut s, |s| !s.bridge.is_story_pending());
        let story = s.story.as_ref().unwrap();
        assert!(matches!(&story.article_summary, crate::story::ArticleSummaryState::Done(text) if text == "article 1"));
        assert!(matches!(&story.summary, SummaryState::Done(summary) if summary.overview == "summary 1"));
        s.open_story(b);
        assert!(matches!(&s.story.as_ref().unwrap().summary, SummaryState::Done(summary) if summary.overview == "summary 2"));
        s.close_story();
        s.open_story(a);
        assert!(!s.bridge.is_story_pending(), "completed readings do not start another request");
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        assert_eq!((*fake.article_summary_requests.lock().unwrap(), *fake.summary_requests.lock().unwrap()), (2, 2));
        assert_eq!(captures.load(Ordering::SeqCst), 2, "cached opens do not recapture articles");
    }

    #[test]
    fn parked_capture_waits_for_reopen_and_respects_clear_then_restore() {
        use std::sync::{Mutex, mpsc};
        use tokio::sync::oneshot;
        for clear in [false, true] {
            let (capture_release, capture_gate) = oneshot::channel();
            let capture_gate = Mutex::new(Some(capture_gate));
            let (captured, captured_rx) = mpsc::channel();
            let (summary_release, summary_gate) = oneshot::channel();
            let summary_gate = Mutex::new(Some(summary_gate));
            let mut fake = crate::service::FakeService::fixture();
            fake.article = Arc::new(move |_| {
                let gate = capture_gate.lock().unwrap().take().unwrap();
                let captured = captured.clone();
                Box::pin(async move { gate.await.unwrap(); captured.send(()).unwrap(); Ok(("captured".into(), false)) })
            });
            fake.summary = Arc::new(move |_| {
                let gate = summary_gate.lock().unwrap().take();
                Box::pin(async move { if let Some(gate) = gate { gate.await.unwrap(); } Ok(crate::fixtures::summary()) })
            });
            let mut s = app(false, SavedView::in_memory());
            s.source = Arc::new(Source::Fake(fake));
            s.config.api_key = "old".into();
            let item = crate::fixtures::item(0, 1);
            s.open_story(item.clone());
            poll_until(&mut s, |s| {
                let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
                *fake.summary_requests.lock().unwrap() == 1
            });
            s.close_story();
            if clear {
                let dir = temp_dir("parked-clear-restore");
                for key in ["", "new"] {
                    let mut settings = Settings::open(&s.config, Some(dir.join("summary.json")), false);
                    settings.set_key(key);
                    s.settings = Some(settings);
                    s.apply_settings(false);
                    assert!(s.settings.is_none());
                }
                std::fs::remove_dir_all(dir).unwrap();
            }
            capture_release.send(()).unwrap();
            captured_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            summary_release.send(()).unwrap();
            for _ in 0..5 { poll(&mut s, 0.0, &mut Tx::default()); }
            let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
            assert_eq!(*fake.article_summary_requests.lock().unwrap(), 0, "parked captures do not start a provider");
            s.open_story(item);
            poll_until(&mut s, |s| !s.bridge.is_story_pending());
            let story = s.story.as_ref().unwrap();
            assert!(matches!(story.article, crate::story::ArticleState::Ready { .. }));
            if clear {
                assert_eq!(story.article_summary, crate::story::ArticleSummaryState::Unconfigured);
                assert_eq!(story.summary, SummaryState::Unconfigured);
                let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
                assert_eq!((*fake.article_summary_requests.lock().unwrap(), *fake.summary_requests.lock().unwrap()), (0, 1));
                s.retry_summary();
                poll_until(&mut s, |s| !s.bridge.is_story_pending());
            } else {
                assert!(matches!(story.article_summary, crate::story::ArticleSummaryState::Done(_)));
                assert!(matches!(story.summary, SummaryState::Done(_)));
            }
            let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
            assert_eq!((*fake.article_summary_requests.lock().unwrap(), *fake.summary_requests.lock().unwrap()),
                if clear { (1, 2) } else { (1, 1) });
        }
    }

    #[test]
    fn clearing_a_parked_retry_preserves_both_previous_summaries() {
        use std::sync::Mutex;
        use tokio::sync::oneshot;
        let mut s = app(false, SavedView::in_memory());
        s.config.api_key = "old".into();
        let item = crate::fixtures::item(0, 1);
        s.open_story(item.clone());
        poll_until(&mut s, |s| !s.bridge.is_story_pending());
        let story = s.story.as_ref().unwrap();
        let article = story.article_summary.readable().unwrap().to_owned();
        let discussion = story.summary.readable().unwrap().clone();
        let sources: Vec<_> = story.sources.iter().map(|source| (source.id, source.text.clone())).collect();
        let (article_release, article_gate) = oneshot::channel();
        let (discussion_release, discussion_gate) = oneshot::channel();
        let article_gate = Mutex::new(Some(article_gate));
        let discussion_gate = Mutex::new(Some(discussion_gate));
        let Source::Fake(fake) = Arc::get_mut(&mut s.source).unwrap() else { unreachable!() };
        fake.article_summary = Arc::new(move |_| {
            let gate = article_gate.lock().unwrap().take().unwrap();
            Box::pin(async move { gate.await.unwrap(); Ok(hn_lib::reader::Article { text: "obsolete".into() }) })
        });
        fake.summary = Arc::new(move |_| {
            let gate = discussion_gate.lock().unwrap().take().unwrap();
            Box::pin(async move { gate.await.unwrap(); let mut summary = crate::fixtures::summary(); summary.overview = "obsolete".into(); Ok(summary) })
        });
        s.retry_summary();
        poll_until(&mut s, |s| {
            let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
            *fake.article_summary_requests.lock().unwrap() == 2 && *fake.summary_requests.lock().unwrap() == 2
        });
        s.close_story();
        let dir = temp_dir("parked-clear-previous");
        for key in ["", "new"] {
            let mut settings = Settings::open(&s.config, Some(dir.join("summary.json")), false);
            settings.set_key(key);
            s.settings = Some(settings);
            s.apply_settings(false);
            assert!(s.settings.is_none());
        }
        std::fs::remove_dir_all(dir).unwrap();
        article_release.send(()).unwrap();
        discussion_release.send(()).unwrap();
        s.open_story(item);
        poll_until(&mut s, |s| !s.bridge.is_story_pending());
        let story = s.story.as_ref().unwrap();
        assert_eq!(story.article_summary.readable(), Some(article.as_str()));
        assert_eq!(story.summary.readable(), Some(&discussion));
        assert_eq!(story.sources.iter().map(|source| (source.id, source.text.clone())).collect::<Vec<_>>(), sources);
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        assert_eq!((*fake.article_summary_requests.lock().unwrap(), *fake.summary_requests.lock().unwrap()), (2, 2));
    }

    #[test]
    fn clearing_the_key_blocks_old_results_and_deferred_provider_calls() {
        use std::sync::Mutex;
        use tokio::sync::oneshot;
        for regenerate in [false, true] {
            let (article_release, article_gate) = oneshot::channel();
            let article_gate = Mutex::new(Some(article_gate));
            let (summary_release, summary_gate) = oneshot::channel();
            let summary_gate = Mutex::new(Some(summary_gate));
            let mut fake = crate::service::FakeService::fixture();
            fake.article = Arc::new(move |_| {
                let gate = article_gate.lock().unwrap().take().unwrap();
                Box::pin(async move { let _ = gate.await; Ok(("captured".into(), false)) })
            });
            fake.summary = Arc::new(move |_| {
                let gate = summary_gate.lock().unwrap().take().unwrap();
                Box::pin(async move { let _ = gate.await; Ok(crate::fixtures::summary()) })
            });
            let mut s = app(false, SavedView::in_memory());
            s.source = Arc::new(Source::Fake(fake));
            s.config.api_key = "fixture".into();
            s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
            poll_until(&mut s, |s| {
                let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
                *fake.summary_requests.lock().unwrap() == 1
            });
            let dir = temp_dir("clear-key-during-capture");
            let mut settings = Settings::open(&s.config, Some(dir.join("hn/summary.json")), false);
            settings.set_key("");
            s.settings = Some(settings);
            s.apply_settings(regenerate);
            assert!(s.settings.is_none(), "the actual Apply path must save and close");
            assert!(!s.configured());
            std::fs::remove_dir_all(dir).unwrap();
            article_release.send(()).unwrap();
            poll_until(&mut s, |s| matches!(s.story.as_ref().unwrap().article, crate::story::ArticleState::Ready { .. }));
            let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
            assert_eq!(*fake.article_summary_requests.lock().unwrap(), 0);
            summary_release.send(()).unwrap();
            poll_until(&mut s, |s| !s.bridge.is_story_pending());
            let story = s.story.as_ref().unwrap();
            assert_eq!(story.summary, SummaryState::Unconfigured);
            assert_eq!(story.article_summary, crate::story::ArticleSummaryState::Unconfigured);
            assert!(!story.thread.is_empty());
        }
    }

    #[test]
    fn discussion_failure_is_explicit_retryable_and_does_not_generate_an_empty_summary() {
        let mut s = app(true, SavedView::in_memory());
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        fake.failing_comments.lock().unwrap().insert(crate::fixtures::STORY_ID);
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        assert!(matches!(s.story.as_ref().unwrap().discussion, Some(Err(_))));
        assert_eq!(s.story.as_ref().unwrap().summary_notice().as_deref(), Some("Discussion unavailable · press r to retry comments"));
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        assert_eq!(*fake.summary_requests.lock().unwrap(), 0);
        fake.failing_comments.lock().unwrap().clear();
        s.retry_summary();
        assert_eq!(s.story.as_ref().unwrap().discussion, Some(Ok(())));
        assert_eq!(s.story.as_ref().unwrap().thread.roots(), 5);
    }

    #[test]
    fn pending_replies_stay_folded_and_a_replaced_story_cannot_accept_them() {
        let mut s = app(true, SavedView::in_memory());
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        s.headless = false;
        s.source = Arc::new(Source::Fake(crate::service::FakeService::fixture().with_delay(std::time::Duration::from_millis(25))));
        s.activate_comment(901);
        s.refresh_comments();
        assert!(s.story.as_ref().unwrap().pending_reply.is_some() && s.story.as_ref().unwrap().refreshing_comments);
        s.close_story();
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        assert!(s.story.as_ref().unwrap().pending_reply.is_none() && !s.story.as_ref().unwrap().refreshing_comments);
        s.activate_comment(901);
        s.refresh_comments();
        let request = s.story.as_ref().unwrap().pending_reply.clone().unwrap();
        s.activate_comment(901);
        assert!(s.story.as_ref().unwrap().thread.is_folded(901), "second activation folds pending branch");
        s.activate_comment(901);
        assert!(!s.story.as_ref().unwrap().thread.is_folded(901));
        press(&mut s, rune::Key::Text("c".into()));
        assert!(s.story.as_ref().unwrap().thread.is_folded(901), "c folds the branch while replies are loading");
        let mut tx = Tx::default();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while (s.story.as_ref().unwrap().pending_reply.is_some() || s.story.as_ref().unwrap().refreshing_comments)
            && std::time::Instant::now() < deadline {
            poll(&mut s, 0.0, &mut tx); std::thread::yield_now();
        }
        assert!(s.story.as_ref().unwrap().pending_reply.is_none());
        assert!(s.story.as_ref().unwrap().thread.is_folded(901));
        assert_eq!(s.story.as_ref().unwrap().thread.children_of(901).len(), 2);
        s.close_story();
        s.headless = true;
        s.open_story(crate::fixtures::item(0, 1));
        assert!(!s.story.as_mut().unwrap().land_replies(crate::bridge::ReplyFilled { request, comments: Ok(crate::fixtures::replies(&[9010,9011])) }));
        assert_eq!(s.story.as_ref().unwrap().thread.len(), 5);
    }

    /// An app built the way `init` builds one, without the machine's files or the network.
    /// `headless` runs the requests to completion, so a test sees the settled feed.
    fn app(headless: bool, saved: SavedView) -> App {
        App {
            feed: FeedModel::new(),
            saved,
            source: Arc::new(Source::fake()),
            bridge: Bridge::new().expect("a tokio runtime for the feed's requests"),
            measured: Cell::new(None),
            requested: None,
            feed_revision: 0,
            pending_arrival: Some(Arrival::Initial),
            arrival: Arrival::Initial,
            loading_since: None,
            hover: None,
            hints: rune::Live::new(1.0),
            story: None,
            readings: HashMap::new(),
            scroll_pacing: Rc::new(Cell::new(ScrollPacing::Idle)),
            scroll_waker: None,
            inspector: inspector(),
            config: Config::default(),
            settings: None,
            help: false,
            layout: Layout::default(),
            layout_path: None,
            pending_external: false,
            native_window: None,
            live: Live::Idle,
            live_bounds: None,
            live_shown: false,
            open_story: None,
            layout_written: Layout::default(),
            layout_saved_at: 0.0,
            window_size: Cell::new(None),
            restored: true,
            toast: None,
            reduced: false,
            headless,
        }
    }

    fn saved_store() -> SavedView {
        let store = [11, 6, 2, 1]
            .into_iter()
            .enumerate()
            .map(|(index, id)| crate::fixtures::item(index, id))
            .collect();
        SavedView::seeded(store)
    }

    #[test]
    fn the_saved_view_renders_from_the_store_and_never_asks_the_source() {
        let mut s = app(false, saved_store());
        s.feed.tab = StoryType::Saved;
        s.request((11, 1), Ranking::Fresh);
        assert!(!s.bridge.is_pending(), "no request went out");
        assert_eq!(
            s.feed.stories.len(),
            4,
            "every saved story, not a viewport of them"
        );
        assert_eq!(
            s.feed
                .stories
                .iter()
                .map(|story| story.id)
                .collect::<Vec<_>>(),
            vec![11, 6, 2, 1],
            "save-time order"
        );
        assert!(!s.feed.status.loading, "the store is local: nothing is in flight");
    }

    #[test]
    fn initial_saved_fill_preserves_its_hover_unsave_action() {
        let mut s = app(true, saved_store());
        s.feed.tab = StoryType::Saved;
        s.hover = Some(11);
        s.request((11, 1), Ranking::Fresh);
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.frame(0.0);
        assert!(rt.elements().iter().any(|e| e.role == "button"
            && e.label.as_deref() == Some("unsave") && e.path.contains("story-11")));
    }

    #[test]
    fn a_page_turn_pages_against_the_snapshot_and_only_a_refresh_re_ranks() {
        let mut s = app(true, SavedView::in_memory());
        s.request((11, 1), Ranking::Fresh);
        let snapshot: Vec<i32> = (1..=30).collect();
        assert_eq!(s.feed.id_snapshot, snapshot);
        // The ranking changes upstream, as it does between two refreshes.
        if let Source::Fake(fake) = s.source.as_ref() {
            fake.set_ranking(StoryType::Top, vec![9, 8, 7]);
        }
        // A page turn keeps the ranks the reader was shown.
        s.feed.selected = 10;
        s.apply(feed_state::next_story(&s.feed.state(), 11));
        assert_eq!(s.feed.id_snapshot, snapshot, "a page turn does not re-snapshot");
        assert_eq!(s.feed.stories[0].id, 12, "rank 12 of the snapshot it was shown");
        // A refresh is the one action that takes a new ranking.
        s.refresh();
        assert_eq!(s.feed.id_snapshot, vec![9, 8, 7]);
    }

    #[test]
    fn a_failed_ranking_reaches_the_feed_error_state() {
        let mut s = app(true, SavedView::in_memory());
        s.request((11, 1), Ranking::Fresh);
        let before = s.feed.stories.len();
        if let Source::Fake(fake) = s.source.as_ref() {
            fake.fail_ranking(StoryType::Top);
        }
        s.refresh();
        assert_eq!(
            s.feed.status.error.as_deref(),
            Some("the ranking request failed"),
            "the failure is a state the view renders with a retry"
        );
        assert_eq!(s.feed.stories.len(), before, "the feed is not blanked");
        assert!(!s.feed.status.loading);
    }

    #[test]
    fn a_hover_action_saves_the_row_it_belongs_to_not_the_selected_one() {
        let mut s = app(true, SavedView::in_memory());
        s.request((11, 1), Ranking::Fresh);
        s.feed.selected = 0;
        // The hover action on rank 3 saves rank 3, not the selected row.
        s.toggle_saved_id(3);
        assert_eq!(s.saved.ids(), vec![3]);
        assert_eq!(s.feed.selected, 0, "the selection did not move");
        s.toggle_saved_id(3);
        assert!(s.saved.ids().is_empty(), "the same action unsaves it");
    }

    #[test]
    fn a_hover_action_opens_the_row_it_belongs_to() {
        let mut s = app(true, SavedView::in_memory());
        s.request((11, 1), Ranking::Fresh);
        s.feed.selected = 0;
        s.open_id(4);
        assert_eq!(s.story.as_ref().map(|story| story.id()), Some(4));
    }

    #[test]
    fn a_store_that_could_not_be_read_does_not_confirm_a_save() {
        let mut s = app(
            true,
            SavedView::corrupt(
                std::path::PathBuf::from("/nope/hn/saved.json"),
                "expected value at line 1 column 2",
            ),
        );
        s.request((11, 1), Ranking::Fresh);
        s.toggle_saved_id(1);
        assert!(s.saved.ids().is_empty(), "nothing was saved");
        assert!(
            s.toast
                .as_ref()
                .unwrap()
                .message
                .contains("unavailable"),
            "the toast says so instead of confirming: {:?}",
            s.toast.as_ref().map(|notice| &notice.message)
        );
    }

    #[test]
    fn the_feed_is_the_same_window_with_the_inspector_collapsed() {
        let mut pane = inspector();
        assert!(!pane.is_open());
        assert_eq!(pane.shown_width(), 0.0, "the split takes no width in the feed");
        // Opening a story is what opens it, and the width it springs out to is the 54/46 split.
        pane.set_open(true);
        assert_eq!(pane.shown_width(), INSPECTOR_WIDTH);
        // `Esc` collapses it again without losing the width the reader chose.
        pane.set_open(false);
        assert_eq!(pane.shown_width(), 0.0);
        pane.set_open(true);
        assert_eq!(pane.width(), INSPECTOR_WIDTH);
    }

    #[test]
    fn the_grip_clamps_and_a_double_click_resets() {
        let mut pane = inspector();
        pane.set_open(true);
        pane.drag_to(10_000.0);
        assert_eq!(pane.width(), INSPECTOR_MAX, "dragged past the maximum");
        pane.drag_to(400.0);
        assert_eq!(pane.width(), 400.0);
        // Below half the minimum it collapses, keeping the width it had, as AppKit's do.
        pane.drag_to(INSPECTOR_MIN * 0.5 - 1.0);
        assert!(!pane.is_open());
        assert_eq!(pane.width(), 400.0);
        // Dragging back out opens it again; double-clicking the grip returns it to 54/46.
        pane.drag_to(420.0);
        assert!(pane.is_open());
        pane.reset_width();
        assert_eq!(pane.width(), INSPECTOR_WIDTH);
    }

    /// A directory of this process's own, so two test binaries never share a file.
    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("hn-desktop-{name}-{}", std::process::id()))
    }

    #[test]
    fn apply_settings_writes_the_config_and_closes_the_dialog() {
        let dir = temp_dir("apply");
        let path = dir.join("hn/summary.json");
        let mut s = app(true, SavedView::in_memory());
        let mut settings = Settings::open(&s.config, Some(path.clone()), false);
        settings.set_key("sk-or-v1-not-a-real-key");
        settings.set_profile(reader::Profile::Thorough);
        s.settings = Some(settings);

        s.apply_settings(false);

        assert!(s.settings.is_none(), "Apply closes the dialog");
        assert_eq!(s.config.api_key, "sk-or-v1-not-a-real-key");
        assert_eq!(s.config.profile, reader::Profile::Thorough);
        assert_eq!(
            s.toast.as_ref().map(|notice| notice.message.as_str()),
            Some("Settings saved"),
            "the save is confirmed in the toast region"
        );
        // The file is the one the terminal app reads: same writer, same path.
        let saved = reader::load_config(&path).expect("the file Apply wrote");
        assert_eq!(saved.api_key, "sk-or-v1-not-a-real-key");
        assert_eq!(saved.profile, reader::Profile::Thorough);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cancelled_dialog_writes_nothing() {
        let dir = temp_dir("cancel");
        let path = dir.join("hn/summary.json");
        let mut s = app(true, SavedView::in_memory());
        let mut settings = Settings::open(&s.config, Some(path.clone()), false);
        settings.set_key("sk-or-v1-not-a-real-key");
        s.settings = Some(settings);

        s.close_settings();

        assert!(s.settings.is_none());
        assert!(s.config.api_key.is_empty(), "the draft was dropped");
        assert!(!path.exists(), "Cancel writes no file");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restore_puts_the_reader_back_where_they_left_off() {
        let mut s = app(true, SavedView::in_memory());
        s.layout = Layout {
            window: Some((1280.0, 820.0)),
            inspector: Some(512.0),
            tab: Some(StoryType::Best),
            page: Some(3),
            selected: Some(42),
        };

        s.restore();

        assert_eq!(s.feed.tab, StoryType::Best);
        assert_eq!(
            s.feed.display_cursor,
            2 * s.feed.page_size as usize,
            "page 3 is the third window of the page size"
        );
        assert_eq!(s.feed.pending_anchor, Some(42), "the feed re-anchors on it");
        assert_eq!(s.inspector.width(), 512.0, "the grip's width came back");
    }

    #[test]
    fn the_layout_file_round_trips_the_window_and_the_feed() {
        let dir = temp_dir("layout");
        let path = dir.join("hn/desktop.json");
        let mut s = app(true, SavedView::in_memory());
        s.request((11, 1), Ranking::Fresh);
        s.layout_path = Some(path.clone());
        s.window_size.set(Some((1280.0, 820.0)));
        s.inspector = PaneState::new(512.0)
            .range(INSPECTOR_MIN, INSPECTOR_MAX)
            .collapsed();
        s.feed.tab = StoryType::Best;
        s.feed.selected = 4;

        let snapshot = s.snapshot();
        assert_eq!(snapshot.window, Some((1280.0, 820.0)));
        assert_eq!(snapshot.selected, Some(5), "the selected story's id");
        s.write_layout(snapshot.clone(), 0.0);
        assert_eq!(Layout::load(&path), snapshot, "the file holds what was drawn");

        // A second window starts from the file, not from the defaults.
        let mut next = app(true, SavedView::in_memory());
        next.layout = Layout::load(&path);
        next.restore();
        assert_eq!(next.feed.tab, StoryType::Best);
        assert_eq!(next.feed.pending_anchor, Some(5));
        assert_eq!(next.inspector.width(), 512.0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
