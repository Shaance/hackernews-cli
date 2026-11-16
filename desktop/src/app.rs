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
use std::path::PathBuf;
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
    Transition, Tx, Waker, WindowHandle,
};
use rune_kit::shell::PaneMotion;
use rune_kit::{Keybind, Kit, PaneState, SegmentedControl, SplitView, Toast, ToastRegion, Toolbar};

use crate::bookmarks::SavedView;
use crate::bridge::{Bridge, Request, StoryRequest, StoryUpdate};
use crate::feed::{FeedModel, Fill, Filled};
use crate::help;
use crate::keys::{self, StoryKey, WindowKey};
use crate::layout::Layout;
use crate::service::Source;
use crate::settings::Settings;
use crate::story::{Story, SummaryState, STAGES};
use crate::story_view::{self, Pane};
use crate::webview;

/// One feed row, in points. The measured row count is the viewport height over this.
const ROW_HEIGHT: f32 = 56.0;
/// Window width at which the feed becomes two columns (DESIGN.md's table).
const BREAKPOINT: f32 = 1400.0;
/// The grid the first request asks for: the 1180×760 default window, one column.
const DEFAULT_GRID: (usize, usize) = (11, 1);
/// How long the save confirmation stays.
const TOAST_SECONDS: f64 = 2.4;
/// How long a request may stay in flight before the feed says so and offers a retry. Without
/// it a dropped answer leaves the feed loading forever, and every page turn is refused.
const LOAD_TIMEOUT: f64 = 10.0;
/// How often the window's own state may be written. A grip drag moves the width every frame,
/// and a file per frame is not worth it.
const LAYOUT_SECONDS: f64 = 2.0;
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
    /// The open story, when the split is showing one. `None` is the feed.
    story: Option<Story>,
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
    /// Set by `O` or the article header's "Open in browser" button; drained in `poll`,
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
        let configured = self.configured();
        self.story = Some(Story::opening(story.clone(), configured));
        self.inspector.set_open(true);
        self.request_story(story);
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

    /// Open `id`, wherever the press came from: the row's hover action, or `Enter` on the
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
    fn close_story(&mut self) {
        self.hide_live();
        self.story = None;
        self.inspector.set_open(false);
        self.bridge.cancel_story();
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
        let Some(request) = self.story.as_mut().and_then(|s| s.request_replies(parent, retry)) else { return; };
        if self.headless {
            let filled = self.bridge.replies_now(&self.source, request);
            if let Some(story) = self.story.as_mut() { story.land_replies(filled); }
        } else {
            self.bridge.request_replies(Arc::clone(&self.source), request);
        }
    }

    pub(crate) fn activate_comment(&mut self, id: i32) {
        let Some(story) = self.story.as_mut() else { return; };
        story.thread.select(id);
        if story.pending_reply.as_ref().is_some_and(|request| request.parent_id == id) {
            if story.thread.is_folded(id) { story.thread.unfold(); } else { story.thread.fold(); }
            return;
        }
        let unloaded = story.thread.has_children(id) && story.thread.children_of(id).is_empty();
        if unloaded { story.thread.unfold(); } else { story.thread.activate(); }
        if unloaded { self.load_replies(id, false); }
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
            .is_some_and(|story| matches!(&story.summary, SummaryState::Failed { auth: true, .. }))
    }

    /// The summary card's settings button, and `M`: the dialog, asking for a key when the
    /// provider rejected the last one.
    pub(crate) fn open_summary_settings(&mut self) {
        self.open_settings(self.auth_failed());
    }

    /// The summary card's Retry: ask for the story again with the current config.
    pub(crate) fn retry_summary(&mut self) {
        let configured = self.configured();
        if let Some(story) = self.story.as_mut() {
            let item = story.story.clone();
            story.retry(configured);
            self.request_story(item);
        }
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

    /// `Apply`, and `Refresh sources & summary`. A save failure keeps the dialog open with the
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
        if refresh {
            let configured = self.configured();
            if let Some(story) = self.story.as_mut() {
                let item = story.story.clone();
                story.retry(configured);
                self.request_story(item);
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
        story: None,
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
    s.bridge.set_waker(waker);
    // The remembered tab and page decide what the first request asks for, so they land before
    // it. The measured grid re-requests if the window is not the default size.
    s.restore();
    s.request(DEFAULT_GRID, Ranking::Fresh);
}

fn poll(s: &mut App, now: f64, tx: &mut Tx) -> bool {
    let mut changed = false;
    let mut feed_arrived = false;
    if let Some(filled) = s.bridge.take() {
        let landed = s.feed.apply_fill(filled);
        changed |= landed;
        if landed { feed_arrived = s.accept_arrival(); }
    }
    while let Some(update) = s.bridge.take_story() {
        let summary_finished = matches!(&update,
            StoryUpdate::Summary { .. } | StoryUpdate::Discussion { comments: Err(_), .. });
        // A landing for a story that is no longer open is dropped by `Story::land_update`.
        let landed = s.story.as_mut().map(|open| {
            let landed = open.land_update(update);
            (landed, (landed && summary_finished).then(|| open.summary_notice()).flatten())
        });
        if let Some((landed, notice)) = landed {
            changed |= landed;
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
    if let Some(filled) = s.bridge.take_replies() {
        changed |= s.story.as_mut().is_some_and(|story| story.land_replies(filled));
    }
    // A reveal the trail's removal aborted, asked again now that the layout has no trail.
    if let Some(id) = s.story.as_mut().and_then(|open| open.pending_reveal.take()) {
        tx.scroll_into_view_in("discussion-scroll", format!("comment-{id}"));
        changed = true;
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
        if feed_arrived && !s.reduced {
            tx.animated();
        } else {
            tx.instant();
        }
    }
    changed
}

fn on_key(s: &mut App, ev: &KeyEvent, tx: &mut Tx) -> bool {
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
    let Some(story) = s.story.as_mut() else {
        return false;
    };
    // The chooser is a surface over the discussion: while it is open it owns j/k, Enter and Esc.
    if story.links.is_some() {
        match key {
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
            StoryKey::Back => {
                story.close_links();
                return true;
            }
            _ => {}
        }
    }
    // The source viewer scrolls its own text, as README.md says: j/k, Page Up/Down and g/G.
    if story.source.is_some() {
        match key {
            StoryKey::Back => {
                story.close_source();
                return true;
            }
            StoryKey::NextComment | StoryKey::NextScreen => {
                tx.scroll_page("source-viewer", 1);
                return true;
            }
            StoryKey::PrevComment | StoryKey::PrevScreen => {
                tx.scroll_page("source-viewer", -1);
                return true;
            }
            StoryKey::FirstComment => {
                tx.scroll_to_y("source-viewer", 0.0);
                return true;
            }
            StoryKey::LastComment => {
                tx.scroll_to_bottom("source-viewer");
                return true;
            }
            StoryKey::NextCitation => {
                story.next_citation();
                return true;
            }
            StoryKey::PrevCitation => {
                story.prev_citation();
                return true;
            }
            _ => {}
        }
    }
    let root = story.thread.root();
    match key {
        StoryKey::None => false,
        StoryKey::Back => {
            // Esc leaves focus mode before it leaves the story, as DESIGN.md's table says.
            if story.thread.leave_focus() {
                return true;
            }
            s.close_story();
            true
        }
        StoryKey::NextCitation => {
            story.next_citation();
            true
        }
        StoryKey::PrevCitation => {
            story.prev_citation();
            true
        }
        StoryKey::Enter => {
            // A selected citation is what Enter reads; otherwise it is the thread's own expand,
            // which re-roots on a `continue thread` row.
            if story.open_source() {
                return true;
            }
            let id = story.thread.cursor();
            if let Some(id) = id { s.activate_comment(id); }
            if let Some(story) = s.story.as_mut() { thread_moved(story, root, tx); }
            true
        }
        StoryKey::ToggleSummary => {
            story.toggle_summary();
            true
        }
        StoryKey::Refresh => {
            s.retry_summary();
            true
        }
        StoryKey::Settings => {
            s.open_summary_settings();
            true
        }
        StoryKey::Links => story.open_links(),
        StoryKey::ToggleSaved => {
            let id = story.id();
            s.toggle_saved_id(id);
            true
        }
        StoryKey::NextComment => {
            story.thread.next();
            thread_moved(story, root, tx);
            true
        }
        StoryKey::PrevComment => {
            story.thread.prev();
            thread_moved(story, root, tx);
            true
        }
        StoryKey::NextSibling => {
            story.thread.next_sibling();
            thread_moved(story, root, tx);
            true
        }
        StoryKey::PrevSibling => {
            story.thread.prev_sibling();
            thread_moved(story, root, tx);
            true
        }
        StoryKey::Parent => {
            story.thread.to_parent();
            thread_moved(story, root, tx);
            true
        }
        StoryKey::Fold => {
            story.thread.fold();
            thread_moved(story, root, tx);
            true
        }
        StoryKey::Unfold => {
            let id = story.thread.cursor();
            story.thread.unfold();
            thread_moved(story, root, tx);
            if let Some(id) = id { s.load_replies(id, false); }
            true
        }
        StoryKey::FirstComment => {
            story.thread.first();
            thread_moved(story, root, tx);
            true
        }
        StoryKey::LastComment => {
            story.thread.last();
            thread_moved(story, root, tx);
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
        StoryKey::Focus => {
            story.thread.toggle_focus();
            true
        }
        StoryKey::PopTrail => {
            story.thread.pop_trail();
            thread_moved(story, root, tx);
            true
        }
    }
}

/// Keep the focused comment in view after a thread transition.
///
/// A transition that re-roots the tree glides the viewport to the new root on `GLIDE` — the
/// rows crossfade in place, so the offset is the only thing that travels. Anything else just
/// reveals the row the cursor landed on.
fn thread_moved(story: &mut Story, before: Option<i32>, tx: &mut Tx) {
    if story.thread.root() != before {
        tx.scroll_to("discussion-scroll", 0.0, 0.0);
    } else if let Some(id) = story.thread.cursor() {
        tx.scroll_into_view_in("discussion-scroll", format!("comment-{id}"));
        // The trail is sticky, and a sticky element that leaves the tree in the same commit
        // aborts the reveal asked for that commit: the row lands where the trail's removal
        // left it. With no ancestors left there is no trail, so ask again next frame, when
        // the layout no longer holds one.
        story.pending_reveal = (story.thread.trail().len() == 1).then_some(id);
    }
}

fn view(s: &App, cx: &mut Cx<App>) -> El {
    let dark = cx.system_dark();
    if s.reduced {
        cx.reduced_motion(true);
    }
    let reduced = cx.is_reduced_motion();
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
    if s.story.is_none() {
        let rows = cx
            .layout_with("feed-list", |rect| {
                (rect[3] / ROW_HEIGHT).floor().max(0.0) as usize
            })
            .filter(|rows| *rows > 0)
            .unwrap_or(DEFAULT_GRID.0);
        s.measured.set(Some((rows, cols)));
    }

    let tabs = SegmentedControl::new(
        &["Top", "New", "Best", "Saved"],
        Some(tab_index(s.feed.tab)),
    )
    .label("Feed")
    .fit()
    .on_pick(|s: &mut App, picked: usize, _tx: &mut Tx| {
        let tab = tab_at(picked);
        if tab != s.feed.tab {
            let state = s.feed.state();
            let action = feed_state::switch_tab(&state, tab);
            s.apply(action);
        }
    })
    .build(&k);

    let mut toolbar = Toolbar::new().leading(tabs);
    if s.feed.tab == StoryType::Saved {
        toolbar = toolbar.trailing(
            text(s.saved.path_label())
                .font(type_::LABEL)
                .color(t.text_secondary)
                .truncate_middle(),
        );
    }

    // One window, one `SplitView`: the feed is this window with the inspector collapsed, so the
    // inspector's slot exists before a story opens and its width can spring out from 0.
    // The overlay layer is built before the panes, because a pane can own a surface in it: the
    // discussion's link chooser hangs off a chip inside the inspector.
    let mut overlays = Vec::new();
    let (main, toolbar, inspector) = match &s.story {
        Some(story) => (
            story_main(story, &k, t, s.pane()),
            Toolbar::new().leading(story_view::story_toolbar(
                story,
                &k,
                t,
                accent,
                s.config.profile.label(),
                s.saved.contains(story.id()),
            )),
            story_view::inspector_pane(story, &k, t, accent, &mut overlays),
        ),
        None => (feed_body(s, &k, t, accent, cols, reduced), toolbar, col()),
    };
    let shell = SplitView::new(main)
        .toolbar(toolbar)
        .inspector(&s.inspector, |s: &mut App| &mut s.inspector, inspector)
        .titlebar_inset(cx.titlebar_inset())
        .pane_motion(pane_motion())
        .reduced_motion(reduced)
        .build(&k);

    let base = col().key("window").w_full().h_full().fill(t.bg).child(shell);
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
}

/// The split's motion: `MORPH`'s spring, scaled by the pane's width so a narrow window does not
/// feel twitchy, with the tail fade DESIGN.md asks for.
fn pane_motion() -> PaneMotion {
    PaneMotion::new()
        .spring(Spring::new(0.30, 0.16))
        .scale_with_width(480.0, 0.15, 0.3)
        .fade_tail(0.15)
}

/// The main pane while a story is open: the article, then the story's own footer.
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
        .child(el().w_full().h(1.0).fill(t.separator))
        .child(story_view::story_footer(k, t))
}

fn feed_body(s: &App, k: &Kit, t: tok::Theme, accent: Color, cols: usize, reduced: bool) -> El {
    let mut body = col().w_full().h_full();
    if let Some(error) = s.saved.error() {
        body = body.child(banner(error, t, accent));
    }
    // A failed ranking or window is a state with a retry, never a blank list.
    if let Some(error) = s.feed.status.error.as_deref() {
        body = body.child(feed_error(error, t, accent));
    }
    body.child(
        feed_list(s, t, accent, cols, reduced)
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .scroll_edge_fade(20.0)
            .grow(1.0)
            .min_h(0.0),
    )
    .child(el().w_full().h(1.0).fill(t.separator))
    .child(footer(s, k, t, accent))
}

/// Column-major: story `c * rows + r`, so the left column fills first and `j` walks straight
/// down it. One scroll viewport for both columns; every cell keeps its own key, so
/// `Tx::scroll_into_view` reaches a story whichever column it is in.
fn feed_list(s: &App, t: tok::Theme, accent: Color, cols: usize, reduced: bool) -> El {
    let shown = s.feed.stories.len();
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
        .gap(if cols == 2 { 0.0 } else { 2.0 });
    if shown == 0 && s.feed.status.error.is_none() {
        let label = if s.feed.tab == StoryType::Saved { "No saved stories yet" } else { "No stories to show" };
        list = list.h_full().center().role("status").label(label)
            .child(text(label).font(type_::BODY).color(t.text_secondary).a11y_hidden());
        return col().key("feed-list").w_full().h_full().child(list.grow(1.0));
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
    if saved && !in_saved_view {
        lead = lead.child(saved_dot(format!("saved-dot-{id}"), accent));
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
        r = r.child(
            row()
                .gap(6.0)
                .center_y()
                .child(act(save, accent, t, move |s: &mut App| {
                    s.toggle_saved_id(id)
                }))
                .child(act("open", t.text_secondary, t, move |s: &mut App| {
                    s.open_id(id)
                })),
        );
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

fn footer(s: &App, k: &Kit, t: tok::Theme, accent: Color) -> El {
    let mut f = row()
        .motion(Spring::NONE)
        .instant_changes()
        // Match the 18 pt page controls plus vertical padding while a label replaces them.
        .min_h(42.0)
        .w_full()
        .px(24.0)
        .py(12.0)
        .gap(18.0)
        .center_y()
        .child(hint(k, t, &["j", "k"], "move"))
        .child(hint(k, t, &["←", "→"], "column / page"))
        .child(hint(
            k,
            t,
            &["s"],
            if s.feed.tab == StoryType::Saved {
                "unsave"
            } else {
                "save"
            },
        ))
        .child(hint(k, t, &["n", "p"], "page"))
        .child(hint(k, t, &["1", "4"], "tabs"))
        .child(el().grow(1.0));
    if s.feed.exhausted(s.cells()) {
        f = f.child(
            text("No more stories")
                .font(type_::LABEL)
                .color(t.text_secondary),
        );
    }
    if s.feed.status.loading && s.feed.stories.is_empty() && s.feed.status.error.is_none() {
        f = f.child(text(loading_label(s.feed.tab)).font(type_::LABEL).color(t.text_secondary).a11y_hidden());
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
fn feed_skeleton(s: &App, t: tok::Theme, cols: usize, reduced: bool) -> El {
    let rows = s.measured.get().unwrap_or(DEFAULT_GRID).0;
    let exit = if reduced { Transition::none() } else {
        Transition::asymmetric(Transition::none(), Transition::opacity())
            .springs(SpringPair::new(Spring::NONE, Spring::new(0.12, 0.0)))
    };
    let mut skeleton = col().key("feed-placeholder").transition(exit).instant_changes().motion(Spring::NONE).w_full().gap(if cols == 2 { 0.0 } else { 2.0 }).a11y_hidden();
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
/// comment, no re-root and no focus mode.
fn scene_comments(s: &mut App) {
    scene_story(s);
}

/// A thread you did not start: re-rooted at the comment that hit the depth cap, the cursor on
/// its first reply, focus mode on. The trail carries the four ancestors above it.
fn scene_thread(s: &mut App) {
    scene_story(s);
    if let Some(story) = &mut s.story {
        story.thread.reroot(901000);
        story.thread.select(9010000);
        story.thread.toggle_focus();
    }
}

/// The citation source viewer: `Tab` twice selects the second citation, `Enter` reads it.
fn scene_citation(s: &mut App) {
    scene_story(s);
    if let Some(story) = &mut s.story {
        story.next_citation();
        story.next_citation();
        story.open_source();
    }
}

/// Generating: the article and the discussion have landed, the summary has not.
fn scene_summary_generating(s: &mut App) {
    scene_story(s);
    if let Some(story) = &mut s.story {
        story.summary = SummaryState::Generating {
            captured: 2,
            of: STAGES,
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
        assert!(matches!(story.summary, SummaryState::Generating { captured: 1, .. }));
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
        assert!(matches!(rt.state.story.as_ref().unwrap().summary, SummaryState::Generating { captured: 2, .. }));
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
        rt.state.open_story(crate::fixtures::item(0, 1));
        poll_until(&mut rt.state, |_| summary_started.load(Ordering::SeqCst) == 4);
        rt.state.open_story(crate::fixtures::item(1, 2));
        poll_until(&mut rt.state, |s| matches!(&s.story.as_ref().unwrap().summary,
            SummaryState::Done(summary) if summary.overview == "current summary 2"));
        assert!(matches!(rt.state.story.as_ref().unwrap().article, ArticleState::Loading));
        assert!(matches!(&rt.state.story.as_ref().unwrap().summary,
            SummaryState::Done(summary) if summary.captured == 2));
        assert!(other_article_tx.send(Ok(("second article".to_string(), false))).is_ok());
        poll_until(&mut rt.state, |s| matches!(&s.story.as_ref().unwrap().summary,
            SummaryState::Done(summary) if summary.captured == 3));
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
        assert_eq!(s.feed.display_cursor, 11);
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
        assert_eq!(s.feed.display_cursor, 11, "the page fill still lands");
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
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/ux-motion/skeleton-handoff/arrivals");
        std::fs::create_dir_all(&out).unwrap();
        for (name, init, poller, group, reduced) in [
            ("initial", loading as fn() -> App, initial as fn(&mut App, f64, &mut Tx) -> bool, "feed-page-0", false),
            ("tab", loaded, tab, "feed-page-1", false),
            ("page", loaded, page, "feed-page-1", false),
            ("saved-empty", saved, poll, "feed-page-1", false),
            ("reduced-page", reduced, page, "feed-page-1", true),
            ("reduced-initial", reduced_loading, initial, "feed-page-0", true),
        ] {
            let captures = if name == "tab" {
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

    #[test]
    #[ignore = "Explicit Metal-backed long-discussion mounting and reveal verification"]
    fn render_long_discussion_workflows() {
        fn long() -> App {
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
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/ux-motion/comments");
        std::fs::create_dir_all(&out).unwrap();
        fn reduced() -> App { let mut s = long(); s.reduced = true; s }
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
            let mut spec = AppSpec::new("Hacker News", (1180.0,760.0), if name == "resize-reduced" { reduced } else { long }, view);
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
        s.retry_summary();
        let story = s.story.as_ref().unwrap();
        assert_eq!(story.discussion, Some(Ok(())));
        assert!(story.thread.is_empty());
        assert!(story.sources.is_empty());
    }

    #[test]
    fn discussion_failure_is_explicit_retryable_and_does_not_generate_an_empty_summary() {
        let mut s = app(true, SavedView::in_memory());
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        fake.failing_comments.lock().unwrap().insert(crate::fixtures::STORY_ID);
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        assert!(matches!(s.story.as_ref().unwrap().discussion, Some(Err(_))));
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
        let request = s.story.as_ref().unwrap().pending_reply.clone().unwrap();
        s.activate_comment(901);
        assert!(s.story.as_ref().unwrap().thread.is_folded(901), "second activation folds pending branch");
        s.activate_comment(901);
        assert!(!s.story.as_ref().unwrap().thread.is_folded(901));
        s.activate_comment(901);
        let mut tx = Tx::default();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while s.story.as_ref().unwrap().pending_reply.is_some() && std::time::Instant::now() < deadline {
            poll(&mut s, 0.0, &mut tx); std::thread::yield_now();
        }
        assert!(s.story.as_ref().unwrap().pending_reply.is_none());
        assert!(s.story.as_ref().unwrap().thread.is_folded(901));
        assert_eq!(s.story.as_ref().unwrap().thread.children_of(901).len(), 2);
        s.close_story();
        s.headless = true;
        s.open_story(crate::fixtures::item(1, 2));
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
            story: None,
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
