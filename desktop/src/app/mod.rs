//! App state and lifecycle for the feed and reading window.
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

mod feed_actions;
mod reading_actions;
mod settings_actions;
mod live_page;
mod window_layout;
mod input;
mod poll;
pub mod window;
pub(crate) mod chrome;
mod feed_view;
mod scenes;

use std::cell::Cell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use hn_lib::reader::{self, Config};
use rune::{Waker, WindowHandle};
use rune_kit::PaneState;

use crate::bookmarks::SavedView;
use crate::bridge::Bridge;
use crate::feed::{Columns, FeedModel, Grid, Ranking, RequestLifecycle};
use crate::layout::{Layout, ZoomStep};
use crate::service::Source;
use crate::settings::Settings;
use crate::story::Story;

/// One feed row, in points. The measured row count is the viewport height over this.
const ROW_HEIGHT: f32 = 56.0;
/// Window width at which the feed becomes two columns (DESIGN.md's table).
const BREAKPOINT: f32 = 1200.0;
/// The grid the first request asks for: the 1180×760 default window, one column.
const DEFAULT_GRID: Grid = Grid::measured(11, Columns::One);
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

/// Why the feed arrives. Refresh and resize keep their current identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Arrival {
    Initial,
    Tab(i8),
    Page(i8),
    Back,
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
enum Live {
    /// No child view yet: nothing has asked for one.
    Idle,
    /// A child exists for this story; its own callback records page commit.
    Created { story_id: i32, child: crate::webview::Child },
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
    measured: Cell<Option<Grid>>,
    /// Accepted tab/page changes replace this group; refresh and resize keep its identity.
    feed_revision: usize,
    pending_arrival: Option<Arrival>,
    arrival: Option<Arrival>,
    feed_scroll_y: Rc<Cell<f32>>,
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
    /// The inspector's width and open state; the view updates its range after measuring the window.
    inspector: Cell<PaneState>,
    /// The chosen and reset widths in window points, independent of Rune's view-point rounding.
    inspector_width: Cell<f32>,
    inspector_default_width: f32,
    /// The reader's summary settings, loaded once at launch and never logged.
    config: Config,
    /// The summary settings dialog, when it is open. `None` is closed.
    settings: Option<Settings>,
    /// The keyboard sheet, when it is open.
    pub help: bool,
    /// The supported zoom applied to the Rune view and live page.
    zoom: ZoomStep,
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
    /// Whether the remembered window size has been asked for on non-macOS platforms.
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
    /// `--open-story <id>`, until the feed holds that story and it opens.
    open_story: Option<i32>,
}

/// What the article pane draws.
///
/// The live page is the default: the pane is a placeholder whose rect the child view is pinned
/// to, and the page itself is drawn by the web engine, not by Rune. The extracted Markdown is
/// the fallback, and it says so — a silent swap would leave the reader unsure whether they are
/// looking at the page or at a capture of it.
pub enum Pane<'a> {
    /// The story's real page, in a child view over the pane's rect.
    Live { loading: bool },
    /// The extracted text, with a banner saying why the live page is not there. `reason` is the
    /// child view's own failure, when there was one.
    Extracted { reason: Option<&'a str> },
}

impl App {
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

    /// Whether a summary can be asked for: the fake provider is a fixture, so a headless run
    /// needs no key, and a live one needs the reader's own `Config::key`.
    fn configured(&self) -> bool {
        self.headless || !self.config.key().is_empty()
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
        feed_revision: 0,
        pending_arrival: Some(Arrival::Initial),
        arrival: Some(Arrival::Initial),
        feed_scroll_y: Rc::new(Cell::new(0.0)),
        hover: None,
        keyboard_selection: true,
        hints: rune::Live::new(1.0),
        story: None,
        visible_comment: Cell::new(None),
        comment_offsets: [Rc::new(Cell::new(0.0)), Rc::new(Cell::new(0.0))],
        readings: HashMap::new(),
        scroll_pacing: Rc::new(Cell::new(ScrollPacing::Idle)),
        scroll_waker: None,
        inspector: Cell::new(PaneState::new(INSPECTOR_WIDTH / zoom.scale())
            .range(INSPECTOR_MIN / zoom.scale(), INSPECTOR_MAX / zoom.scale())
            .collapsed()),
        inspector_width: Cell::new(INSPECTOR_WIDTH),
        inspector_default_width: INSPECTOR_WIDTH,
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
        open_story: OPEN_STORY.get().copied().flatten(),
        toast: None,
        reduced: std::env::var("HN_REDUCED_MOTION").is_ok_and(|value| value == "1"),
        headless,
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
