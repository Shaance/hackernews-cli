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
use rune_kit::{Button, ButtonVariant, ControlSize, Keybind, Kit, PaneState, SplitView, Toast, ToastRegion, Toolbar};

use crate::bookmarks::SavedView;
use crate::bridge::{Bridge, feed::Request};
use crate::story_io::{StoryRequest, StoryUpdate};
use crate::feed::{FeedModel, Fill, Filled};
use crate::help;
use crate::keys::{self, StoryKey, WindowKey};
use crate::layout::{Layout, ZOOM_STEPS};
use crate::service::Source;
use crate::settings::Settings;
use crate::story::{Story, SummaryState};
use crate::story_view::{self, Pane};
use crate::webview;

/// One feed row, in points. The measured row count is the viewport height over this.
const ROW_HEIGHT: f32 = 56.0;
/// Window width at which the feed becomes two columns (DESIGN.md's table).
const BREAKPOINT: f32 = 1200.0;
/// The grid the first request asks for: the 1180×760 default window, one column.
const DEFAULT_GRID: (usize, usize) = (11, 1);
const FEED_GUTTER: f32 = 8.0;
const DEFAULT_FEED_HEIGHT: f32 = 661.0;
/// How long a transient notice stays.
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
/// The inspector's default width and range in window points, independent of UI zoom.
const INSPECTOR_WIDTH: f32 = 543.0;
const INSPECTOR_MIN: f32 = 360.0;
const INSPECTOR_MAX: f32 = 640.0;

/// A transient notice. `until` is on the frame clock, so `poll` expires it.
struct Notice {
    message: String,
    until: f64,
    show_dot: bool,
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
    /// The newest fill, ranking policy and grid, so resize preserves in-flight work.
    requested: Option<(Fill, Ranking, (usize, usize))>,
    /// Accepted tab/page changes replace this group; refresh and resize keep its identity.
    feed_revision: usize,
    pending_arrival: Option<Arrival>,
    arrival: Arrival,
    /// When the in-flight request started, on the frame clock, for [`LOAD_TIMEOUT`].
    loading_since: Option<f64>,
    /// The row under the pointer, for the hover actions.
    hover: Option<i32>,
    /// Whether the last selection came from the keyboard.
    pub(crate) keyboard_selection: bool,
    /// Passive shortcut hints yield visual emphasis to pointer use and return on navigation.
    hints: rune::Live<f32>,
    /// The open story, when the split is showing one. `None` is the feed.
    story: Option<Story>,
    /// The selected row and viewport's unscrolled vertical geometry.
    visible_comment: Cell<Option<(i32, bool, [f32; 2], [f32; 2])>>,
    comment_offsets: [Rc<Cell<f32>>; 2],
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
    /// The supported zoom applied to the Rune view and live page.
    zoom: f32,
    /// What the window remembers between runs, and where it writes it. A headless run has
    /// neither: a golden must not depend on the machine's window size.
    layout: Layout,
    layout_path: Option<PathBuf>,
    /// The layout as last written, so `poll` writes only when something moved.
    layout_written: Layout,
    /// When the layout was last written, on the frame clock, for [`LAYOUT_SECONDS`].
    layout_saved_at: f64,
    /// The window size in window points. Written by `view`, read by `poll`.
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
    /// The live region's last reported rect, converted to window points for the child.
    live_bounds: Option<[f32; 4]>,
    /// Whether the child is currently shown, so `poll` only calls `set_visible` on a change.
    live_shown: bool,
    /// `--open-story <id>`, until the feed holds that story and it opens.
    open_story: Option<i32>,
}

impl App {
    fn window_action(&mut self, action: WindowKey, tx: &mut Tx) -> bool {
        match action {
            WindowKey::Settings => self.open_summary_settings(),
            WindowKey::Help => self.help = !self.help,
            WindowKey::ToggleSplit => if self.story.is_some() {
                self.inspector.set_open(!self.inspector.is_open());
            },
            WindowKey::ZoomIn | WindowKey::ZoomOut | WindowKey::ActualSize => {
                let old_zoom = self.zoom;
                let index = ZOOM_STEPS.iter().position(|&step| step == self.zoom)
                    .expect("zoom is always a supported step");
                self.zoom = match action {
                    WindowKey::ZoomIn => ZOOM_STEPS[(index + 1).min(ZOOM_STEPS.len() - 1)],
                    WindowKey::ZoomOut => ZOOM_STEPS[index.saturating_sub(1)],
                    WindowKey::ActualSize => 1.0,
                    _ => unreachable!(),
                };
                if self.zoom != old_zoom {
                    let pane = self.inspector;
                    self.inspector = PaneState::new(pane.default_width() * old_zoom / self.zoom)
                        .range(INSPECTOR_MIN / self.zoom, INSPECTOR_MAX / self.zoom)
                        .with_width(pane.width() * old_zoom / self.zoom);
                    self.inspector.set_open(pane.is_open());
                }
                webview::set_zoom(self.zoom);
                // An unchanged view-point rect does not trigger on_layout when zoom changes.
                if let Some(rect) = &mut self.live_bounds {
                    *rect = rect.map(|value| value * self.zoom / old_zoom);
                    if self.live_shown { webview::set_bounds(*rect); }
                }
                if self.zoom != old_zoom || action == WindowKey::ActualSize {
                    self.toast = Some(Notice { message: format!("{:.0}%", self.zoom * 100.0),
                        until: f64::INFINITY, show_dot: false });
                }
                tx.instant();
            }
            WindowKey::None => return false,
        }
        true
    }

    pub(crate) fn keyboard_navigation(&mut self, ev: &KeyEvent) {
        if self.settings.is_some() || self.help || keys::parity_key(ev).is_none() { return; }
        if matches!(ev.key, rune::Key::Tab | rune::Key::Home | rune::Key::End | rune::Key::PageUp | rune::Key::PageDown)
            || (self.story.is_some() && keys::story_key(ev) != StoryKey::None)
            || (self.story.is_none() && (keys::story_action(ev) != StoryAction::None
                || matches!(ev.key, rune::Key::Left | rune::Key::Right)))
        {
            if self.story.is_none() && (matches!(ev.key, rune::Key::Left | rune::Key::Right)
                || matches!(keys::story_action(ev), StoryAction::NextStory | StoryAction::PrevStory
                    | StoryAction::FirstStory | StoryAction::LastStory | StoryAction::NextPage
                    | StoryAction::PrevPage | StoryAction::SetType(_)))
                || self.story.is_some() && matches!(keys::story_key(ev), StoryKey::NextComment | StoryKey::PrevComment
                    | StoryKey::NextSibling | StoryKey::PrevSibling | StoryKey::Parent | StoryKey::Fold
                    | StoryKey::Unfold | StoryKey::FirstComment | StoryKey::LastComment | StoryKey::PopTrail
                    | StoryKey::Enter | StoryKey::Back | StoryKey::Focus) {
                self.keyboard_selection = true;
            }
            self.hints.snap(1.0);
        }
    }

    pub(crate) fn pointer_wheel(s: &mut App, ev: rune::WheelEvent, _: &mut Tx) -> bool {
        if !ev.momentum && (ev.dx != 0.0 || ev.dy != 0.0) { s.hints.set(0.3); }
        false
    }

    fn scroll_observer(&self, slot: Option<usize>) -> impl Fn(rune::ScrollPos) + 'static {
        let pacing = Rc::clone(&self.scroll_pacing);
        let offset = slot.map(|i| Rc::clone(&self.comment_offsets[i]));
        let waker = self.scroll_waker.clone();
        let previous = Cell::new(0.0);
        // At this Rune pin, a source-less live-only handler can leave its live wake pending.
        // The offset observer wakes poll without a handler commit or a live-only event.
        move |at| {
            if let Some(offset) = &offset { offset.set(at.offset[1]); }
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
        self.requested = Some((fill, ranking, grid));
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
            show_dot: self.saved.error().is_some() || saved,
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
            self.story = Some(Story::opening(story.clone(), self.configured() && self.config.automatic));
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
        let rect = rect.map(|value| value * self.zoom);
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
            webview::set_zoom(self.zoom);
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

    fn land_comment_refresh(&mut self, result: crate::story_io::CommentsRefreshed) -> bool {
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
            show_dot: true,
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
            self.inspector = PaneState::new(width.clamp(INSPECTOR_MIN, INSPECTOR_MAX) / self.zoom)
                .range(INSPECTOR_MIN / self.zoom, INSPECTOR_MAX / self.zoom)
                .collapsed();
        }
    }

    /// What the window would write now: its size, the inspector's width, and where the feed is.
    fn snapshot(&self) -> Layout {
        Layout {
            window: self.window_size.get().or(self.layout.window),
            zoom: Some(self.zoom),
            inspector: Some(self.inspector.width() * self.zoom),
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
    let zoom = layout.zoom_level();
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
        keyboard_selection: true,
        hints: rune::Live::new(1.0),
        story: None,
        visible_comment: Cell::new(None),
        comment_offsets: [Rc::new(Cell::new(0.0)), Rc::new(Cell::new(0.0))],
        readings: HashMap::new(),
        scroll_pacing: Rc::new(Cell::new(ScrollPacing::Idle)),
        scroll_waker: None,
        inspector: PaneState::new(INSPECTOR_WIDTH / zoom)
            .range(INSPECTOR_MIN / zoom, INSPECTOR_MAX / zoom)
            .collapsed(),
        config,
        settings: None,
        help: false,
        zoom,
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
    let mut toast_expired = false;
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
                    show_dot: true,
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
        if s.feed.tab == StoryType::Saved
            && s.requested.is_some_and(|(_, _, previous)| previous != grid)
        {
            if let Some(id) = s.feed.selected_id() {
                tx.scroll_into_view(format!("story-{id}"));
            }
            tx.instant();
            changed = true;
        }
        if s.requested.map(|(fill, _, _)| fill) != Some(need) {
            // A page turn has its own destination. Reflow the page on screen only when
            // there is no such turn, or when an earlier reflow is already pending.
            if s.feed.tab != StoryType::Saved && need.count > 0
                && (s.feed.pending_cursor.is_none() || s.feed.pending_anchor.is_some())
                && let Some(id) = s.feed.selected_id()
            {
                let offset = (s.feed.selected / need.count) * need.count;
                if offset > 0 || s.feed.pending_cursor.is_some() {
                    s.feed.pending_anchor.get_or_insert(id);
                    s.feed.pending_cursor = (offset > 0).then_some(s.feed.display_cursor + offset);
                }
            }
            let ranking = if s.feed.status.loading {
                s.requested.map_or(Ranking::Keep, |(_, ranking, _)| ranking)
            } else {
                Ranking::Keep
            };
            if !matches!(s.pending_arrival, Some(Arrival::Initial | Arrival::Tab(_))) {
                s.pending_arrival = None;
            }
            s.request(grid, ranking);
            changed = true;
        } else if let Some((_, _, previous)) = &mut s.requested {
            *previous = grid;
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
            toast_expired = true;
        }
    }
    if changed {
        if (feed_arrived || summary_arrived || replies_arrived || toast_expired) && !s.reduced {
            tx.animated();
        } else {
            tx.instant();
        }
    }
    changed
}

fn on_key(s: &mut App, ev: &KeyEvent, tx: &mut Tx) -> bool {
    s.keyboard_navigation(ev);
    // AppKit consumes native menu equivalents first; this also handles headless and layout variants.
    if s.window_action(keys::window_key(ev), tx) { return true; }
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
    // Desktop feed exception: the terminal feed has no summary-settings action.
    if matches!(&ev.key, rune::Key::Text(text) if text == "M")
        && matches!(keys::parity_key(ev), Some(crossterm::event::KeyCode::Char('M'))) {
        s.open_summary_settings();
        return true;
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
    let visible_comment = s.visible_comment.get().and_then(|(id, source, row, viewport)| {
        let y = row[0] - s.comment_offsets[usize::from(source)].get();
        let top = viewport[0] + if source { 0.0 } else { 27.0 };
        (y < viewport[0] + viewport[1] && y + row[1] > top).then_some((id, source))
    });
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
                s.toast = Some(Notice { message: "Open links from the discussion".to_string(), until: f64::INFINITY, show_dot: true });
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
                if let Some(story) = s.story.as_mut() { thread_moved(story, root, true, visible_comment, tx); }
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
            if changed { thread_moved(story, root, key == StoryKey::Fold, visible_comment, tx); }
            true
        }
        StoryKey::Unfold => {
            if !in_source { story.citation_focus = false; }
            let id = cursor;
            let retry = id.is_some_and(|id| story.thread.children_of(id).is_empty()
                && story.thread.reply_batch(id, false).is_empty()
                && !story.thread.reply_batch(id, true).is_empty());
            if story.with_thread(|thread| thread.unfold()) { thread_moved(story, root, true, visible_comment, tx); }
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
fn thread_moved(story: &mut Story, before: Option<i32>, fold: bool, visible: Option<(i32, bool)>, tx: &mut Tx) {
    let source = story.source.is_some();
    if fold {
        let (root, cursor) = story.with_thread(|thread| (thread.root(), thread.cursor()));
        if root == before && let Some(id) = cursor && visible == Some((id, source)) {
            tx.keep_on_screen(format!("{}comment-{id}", if source { "source-" } else { "" }));
            return;
        }
    }
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
    cx.zoom(s.zoom);
    let dark = cx.system_dark();
    if s.reduced {
        cx.reduced_motion(true);
    }
    let reduced = cx.is_reduced_motion();
    let contrast = cx.system_increased_contrast();
    let t = tok::theme(dark);
    let accent = tok::accent(3, dark);
    let mut k = Kit::neutral(dark);
    k.c.border_focus = accent;
    // DESIGN.md's focus affordance is amber, not AppKit's blue: the view-wide default, so every
    // control that does not set its own ring gets the same one.
    cx.focus_ring(2.0, 1.0, accent);

    // The breakpoint is in view points; persist the same rect in window points.
    let window = cx.layout_with("window", |rect| rect);
    if let Some(rect) = window {
        s.window_size.set(Some((rect[2] * s.zoom, rect[3] * s.zoom)));
    }
    // Tx has no keyed visibility query; scroll observers supply offsets without view commits.
    let visible = s.story.as_ref().and_then(|story| {
        let source = story.source.as_ref();
        let id = source.map_or_else(|| story.thread.cursor(), |view| view.nav.cursor)?;
        let viewport = cx.layout_with(if source.is_some() { "source-viewer" } else { "discussion-scroll" }, |r| [r[1], r[3]])?;
        let row = cx.layout_with(&format!("{}comment-{id}", if source.is_some() { "source-" } else { "" }), |r| [r[1], r[3]])?;
        Some((id, source.is_some(), row, viewport))
    });
    s.visible_comment.set(visible);
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
    // Saved is one scrollable list. Change its columns in the same commit that reveals the
    // selection at its new position, rather than putting it offscreen for a frame.
    let feed_cols = if s.feed.tab == StoryType::Saved {
        s.requested.map_or(cols, |(_, _, grid)| grid.1)
    } else { cols };

    let handoff = if reduced { Transition::none() } else {
        Transition::asymmetric(Transition::opacity(), Transition::none())
            .springs(SpringPair::new(Spring::new(0.15, 0.0), Spring::NONE))
    };
    let mut toolbar = Toolbar::new().leading(feed_tabs(s, &k, t, accent, reduced).transition(handoff));
    let trailing = if s.feed.tab == StoryType::Saved {
        row().gap(8.0).center_y().child(text(s.saved.path_label())
                .font(type_::LABEL)
                .color(t.text_secondary)
                .truncate_middle()).child(brand(t))
    } else { brand(t) };
    toolbar = toolbar.trailing(trailing);

    // One window, one `SplitView`: the feed is this window with the inspector collapsed, so the
    // inspector's slot exists before a story opens and its width can spring out from 0.
    // The overlay layer is built before the panes, because a pane can own a surface in it: the
    // discussion's link chooser hangs off a chip inside the inspector.
    let mut overlays = Vec::new();
    let (main, toolbar, inspector) = match &s.story {
        Some(story) => (
            story_main(story, &k, t, s.pane()).transition(handoff),
            Toolbar::new().leading(story_view::story_toolbar(
                t,
                accent,
                s.saved.contains(story.id()),
            ).transition(handoff)).trailing(brand(t)),
            story_view::inspector_pane(story, &k, t, accent, &mut overlays, reduced,
                window.map_or(360.0, |rect| (rect[3] - 310.0).clamp(230.0, 440.0)),
                [s.scroll_observer(Some(0)), s.scroll_observer(None), s.scroll_observer(Some(1))]),
        ),
        None => (feed_body(s, &k, t, accent, feed_cols, reduced, contrast).transition(handoff), toolbar, col()),
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
        let footer = if s.story.is_some() { "story-footer" } else { "feed-footer" };
        let inset = window.and_then(|w| cx.layout_with(footer, move |r| w[1] + w[3] - r[1] + 12.0)).unwrap_or(64.0);
        let mut toast = Toast::new("toast-saved", notice.message.clone());
        if notice.show_dot { toast = toast.lead(saved_dot("toast-dot", accent)); }
        overlays.push(
            ToastRegion::new()
                .toast(toast.build(&k).instant_changes())
                .build(&k)
                .pin(None, None, Some(32.0), Some(inset))
                .transition(Transition::opacity().springs(SpringPair::new(
                    Spring::new(0.2, 0.0), Spring::new(0.15, 0.0)))),
        );
    }
    // The dialogs are the last layer, so they sit over the toast and the split.
    if let Some(settings) = &s.settings {
        settings.view(&k, t, accent, true, s.story.is_some(), &mut overlays);
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

pub(crate) fn quiet_button(label: impl Into<String>) -> Button {
    Button::new(label).variant(ButtonVariant::GhostMuted).control_size(ControlSize::Small)
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

fn feed_body(s: &App, k: &Kit, t: tok::Theme, accent: Color, cols: usize, reduced: bool, contrast: bool) -> El {
    let mut body = col().w_full().h_full();
    if let Some(error) = s.saved.error() {
        body = body.child(banner(s, error, k, t));
    }
    // A failed ranking or window is a state with a retry, never a blank list.
    if let Some(error) = s.feed.status.error.as_deref() {
        body = body.child(feed_error(error, k, t));
    }
    body.child(
        feed_list(s, t, accent, cols, reduced)
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
fn feed_list(s: &App, t: tok::Theme, accent: Color, cols: usize, reduced: bool) -> El {
    // The measured grid is available during this view, before poll can settle its fill.
    // Show the screenful containing the selection immediately, using the held stories.
    let capacity = s.measured.get().unwrap_or(DEFAULT_GRID).0.saturating_mul(cols);
    let start = if s.feed.tab != StoryType::Saved && capacity > 0 {
        (s.feed.selected / capacity) * capacity
    } else { 0 };
    let shown = if s.feed.tab == StoryType::Saved { s.feed.stories.len() }
        else { s.feed.stories.len().saturating_sub(start).min(capacity) };
    let per_column = shown.div_ceil(cols.max(1));
    // Keep the scroll viewport stable; only a completed page turn remounts this group.
    if s.feed.status.loading && s.feed.stories.is_empty() && s.feed.status.error.is_none() {
        return feed_skeleton(s, t, cols, reduced);
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
        .pt(FEED_GUTTER)
        .pb(FEED_GUTTER)
        .gap(if cols == 2 { 0.0 } else { 2.0 });
    if shown == 0 && s.feed.status.error.is_none()
        && (s.feed.tab != StoryType::Saved || s.saved.error().is_none()) {
        let label = if s.feed.tab == StoryType::Saved { "Press S on a story to save it" } else { "No stories to show" };
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
            line = line.child(story_row(s, start + i, t, accent, reduced).grow(1.0));
        }
        list = list.child(line);
    }
    col().key("feed-list").w_full().child(list)
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
        .motion(if reduced || s.keyboard_selection { Spring::NONE } else { Spring::new(0.12, 0.0) })
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
                s.keyboard_selection = false;
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
            .motion(if reduced || s.keyboard_selection { Spring::NONE } else { Spring::new(0.12, 0.0) })
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
            let mut pop = Transition::asymmetric(Transition::scale(0.6), Transition::none())
                .springs(SpringPair::new(Spring::new(0.25, 0.15), Spring::NONE));
            pop.insertion.opacity = 1.0;
            saved_dot(key, accent).animated_changes()
                .transition(if reduced { Transition::none() } else { pop })
                .motion_intent(rune::MotionIntent::OVERSHOOT)
        } else {
            el().key(format!("unsaved-dot-{id}")).size(6.0, 6.0)
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
        .center_y();
    if !s.feed.stories.is_empty() {
        f = f.child(h(&["j", "k"], "move"))
        .child(h(&["←", "→"], "column / page"))
        .child(h(
            &["s"],
            if s.feed.tab == StoryType::Saved {
                "unsave"
            } else {
                "save"
            },
        ))
        .child(h(&["n", "p"], "page"));
    }
    f = f.child(h(&["1", "4"], "tabs"))
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
        let label = if s.requested.is_some_and(|(_, ranking, _)| ranking == Ranking::Fresh) {
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

/// The saved-file banner keeps recovery actions next to the error.
fn banner(s: &App, error: &str, k: &Kit, t: tok::Theme) -> El {
    let message = if s.saved.read_failed() { "Your saved list could not be read. Nothing was changed." }
        else { "Your saved list could not be updated. Nothing was changed." };
    let banner = row()
        .key("saved-banner")
        .role("alert")
        .w_full()
        .px(24.0)
        .py(10.0)
        .gap(12.0)
        .center_y()
        .fill(t.card)
        .child(col().grow(1.0).min_w(0.0).gap(2.0)
            .child(text(message).font(type_::LABEL).color(t.text).truncate())
            .child(text(error.to_string()).font(type_::LABEL).color(t.text_secondary).truncate()))
        .child(
            quiet_button("Reload")
                .on_press(|s: &mut App| {
                    s.saved.reload();
                    s.refresh();
                })
                .build(k).key("saved-reload").cursor(rune::Cursor::Pointer),
        );
    #[cfg(target_os = "macos")]
    let banner = if s.saved.path().is_some() {
        banner.child(quiet_button("Reveal in Finder").on_press(|s: &mut App| {
            if !s.headless && let Some(path) = s.saved.path() {
                let _ = std::process::Command::new("/usr/bin/open").arg("-R").arg(path).spawn();
            }
        }).build(k).key("saved-reveal").cursor(rune::Cursor::Pointer))
    } else { banner };
    banner
}

/// The feed's own failure: the ranking or the window could not be loaded. It is a state with a
/// retry, never a blank list.
fn feed_error(error: &str, k: &Kit, t: tok::Theme) -> El {
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
            quiet_button("Retry")
                .on_press(|s: &mut App| s.refresh())
                .build(k).key("feed-retry").cursor(rune::Cursor::Pointer),
        )
}

/// Decorative rows occupy the same viewport and lanes as the feed, with one status owner.
fn feed_skeleton(s: &App, t: tok::Theme, cols: usize, reduced: bool) -> El {
    let rows = s.measured.get().unwrap_or(DEFAULT_GRID).0;
    let transition = if reduced { Transition::none() } else {
        let mut transition = Transition::asymmetric(Transition::opacity(), Transition::opacity())
            .springs(SpringPair::new(Spring::new(0.20, 0.0), Spring::new(0.12, 0.0)));
        transition.insertion.opacity = 0.35;
        transition
    };
    let mut skeleton = col().key("feed-placeholder").transition(transition).instant_changes().motion(Spring::NONE).w_full()
        .pt(FEED_GUTTER).pb(FEED_GUTTER)
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
        show_dot: true,
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

#[cfg(target_os = "macos")]
const WINDOW_MENUS: [(&str, Option<&str>, &str, fn(&mut App, &mut Tx)); 6] = [
    ("Settings…", None, "cmd+,", |s, tx| { s.window_action(WindowKey::Settings, tx); }),
    ("Zoom In", Some("View"), "cmd+=", |s, tx| { s.window_action(WindowKey::ZoomIn, tx); }),
    ("Zoom Out", Some("View"), "cmd+-", |s, tx| { s.window_action(WindowKey::ZoomOut, tx); }),
    ("Actual Size", Some("View"), "cmd+0", |s, tx| { s.window_action(WindowKey::ActualSize, tx); }),
    ("Show/Hide Inspector", Some("View"), "cmd+\\", |s, tx| { s.window_action(WindowKey::ToggleSplit, tx); }),
    ("Keyboard Shortcuts", Some("Help"), "cmd+shift+/", |s, tx| { s.window_action(WindowKey::Help, tx); }),
];

#[cfg(target_os = "macos")]
fn native_menus(mut app: rune::App<App>) -> rune::App<App> {
    for (title, menu, shortcut, action) in WINDOW_MENUS {
        let mut item = rune::MenuItem::new(title, action).key_equivalent(shortcut.parse().expect("valid menu shortcut"));
        if let Some(menu) = menu { item = item.menu(menu); }
        app = app.menu_item(item);
    }
    app.standard_menus()
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
    #[cfg(target_os = "macos")]
    { app = native_menus(app); }
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
fn close(s: &mut App, _tx: &mut Tx) {
    if !s.headless {
        let snapshot = s.snapshot();
        if snapshot != s.layout_written {
            s.write_layout(snapshot, rune::scene_clock().now);
        }
    }
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
    mod feed_paging;
    mod feed_arrivals;
    mod saved;
    mod story_lifecycle;
    mod summary_lifecycle;
    mod discussion;
    mod citation;
    mod keys;
    mod window_layout;
    mod settings;
    mod support;
}
