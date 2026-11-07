//! The window: `SplitView` shell, the responsive feed, the Saved view, and the story split.
//!
//! The view is a pure function of [`App`]; the only thing it writes back is the grid it
//! measured, because `view` takes `&S` and `poll` needs the number. Everything else — the
//! ranked window, the page stepper, the selection — is `hn_lib::feed_state`'s arithmetic
//! applied by [`FeedModel`].
//!
//! One window, one `SplitView`. Opening a story puts the article in the main pane and the
//! summary card plus the discussion in the inspector, which springs open from 0 width on
//! `MORPH`; `Esc` closes it and the feed is exactly where it was.

use std::cell::Cell;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use hn_lib::app::StoryType;
use hn_lib::event::StoryAction;
use hn_lib::feed_state;
use hn_lib::reader::{self, Config};
use hn_lib::HNCLIItem;
use rune::tok::{self, radius, type_};
use rune::{
    AppSpec, Color, Cx, El, KeyEvent, ScrollSpec, Spring, Tx, Waker, col, el, row, text,
};
use rune_kit::shell::PaneMotion;
use rune_kit::{Keybind, Kit, PaneState, SegmentedControl, SplitView, Toast, ToastRegion, Toolbar};

use crate::bookmarks::SavedView;
use crate::bridge::{Bridge, Request, StoryRequest};
use crate::feed::{FeedModel, Fill};
use crate::help;
use crate::keys::{self, StoryKey, WindowKey};
use crate::layout::Layout;
use crate::service::Source;
use crate::settings::Settings;
use crate::story::{Story, SummaryState, STAGES};
use crate::story_view;

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

pub struct App {
    feed: FeedModel,
    saved: SavedView,
    source: Arc<Source>,
    bridge: Bridge,
    /// The grid the last commit measured. Written by `view`, read by `poll`.
    measured: Cell<Option<(usize, usize)>>,
    /// The fill the newest request asked for, so `poll` asks once per invalidation.
    requested: Option<Fill>,
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
}

impl App {
    /// Ask for the window `grid` shows. `ranking` says whether this request re-snapshots.
    ///
    /// Saved is the local store, not a feed: it renders every saved story from memory and
    /// never reaches the source, so the viewport applies to display only.
    fn request(&mut self, grid: (usize, usize), ranking: Ranking) {
        let fill = self.feed.fill(grid.0, grid.1);
        self.requested = Some(fill);
        if fill.tab == StoryType::Saved {
            self.feed.apply_saved(self.saved.store());
            return;
        }
        let snapshot = match ranking {
            Ranking::Fresh => None,
            Ranking::Keep if !self.feed.id_snapshot.is_empty() => Some(self.feed.id_snapshot.clone()),
            Ranking::Keep => None,
        };
        self.feed.status.loading = true;
        let request = Request::new(fill, snapshot);
        if self.headless {
            let filled = self.bridge.fill_now(self.source.as_ref(), request);
            self.feed.apply_fill(filled);
        } else {
            self.bridge.request(self.source.clone(), request);
        }
    }

    /// Re-request the current window with a new ranking, as `r` does. The story the reader has
    /// selected stays selected: the feed re-anchors on its id when the new snapshot lands.
    fn refresh(&mut self) {
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
        let applied = self.feed.apply(action, grid);
        if applied.fill.is_some() {
            self.request(grid, ranking);
        }
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

    /// Open `story`: the split springs open and one aggregate request goes out.
    fn open_story(&mut self, story: HNCLIItem) {
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

    /// One aggregate story open: the article, the discussion, the summary.
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
        self.story = None;
        self.inspector.set_open(false);
        self.bridge.cancel_story();
    }

    /// The open story, for the views' press handlers: a press on a comment row, a trail segment
    /// or a link is a transition on the thread, and the thread lives here.
    pub(crate) fn story_mut(&mut self) -> Option<&mut Story> {
        self.story.as_mut()
    }

    /// The settings dialog's draft, for its own press handlers.
    pub(crate) fn settings_mut(&mut self) -> Option<&mut Settings> {
        self.settings.as_mut()
    }

    /// Whether the open story's summary failed because the provider rejected the key. That is
    /// the one failure a retry cannot fix, so `M` opens the dialog asking for a new key.
    fn auth_failed(&self) -> bool {
        self.story.as_ref().is_some_and(|story| {
            matches!(&story.summary, SummaryState::Failed { auth: true, .. })
        })
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
        toast: None,
        reduced: std::env::var("HN_REDUCED_MOTION").is_ok_and(|value| value == "1"),
        headless,
    }
}

/// One aggregate fill request, then the waker that brings its answer back to `poll`.
fn setup(s: &mut App, waker: Waker) {
    s.bridge.set_waker(waker);
    // The remembered tab and page decide what the first request asks for, so they land before
    // it. The measured grid re-requests if the window is not the default size.
    s.restore();
    s.request(DEFAULT_GRID, Ranking::Fresh);
}

fn poll(s: &mut App, now: f64, tx: &mut Tx) -> bool {
    let mut changed = false;
    if let Some(filled) = s.bridge.take() {
        changed |= s.feed.apply_fill(filled);
    }
    if let Some(filled) = s.bridge.take_story() {
        // A landing for a story that is no longer open is dropped by `Story::land`.
        let landed = s.story.as_mut().map(|open| {
            let landed = open.land(filled);
            (landed, landed.then(|| open.summary_notice()).flatten())
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
    // A reveal the trail's removal aborted, asked again now that the layout has no trail.
    if let Some(id) = s.story.as_mut().and_then(|open| open.pending_reveal.take()) {
        tx.scroll_into_view_in("discussion-scroll", format!("comment-{id}"));
        changed = true;
    }
    // One request per invalidation: the measured grid is the only thing that moves it. A
    // resize pages against the ranking on screen, so the ranks do not move under the reader.
    if let Some(grid) = s.measured.get() {
        let need = s.feed.fill(grid.0, grid.1);
        if s.requested != Some(need) {
            s.request(grid, Ranking::Keep);
            changed = true;
        }
    }
    // A request that never lands must not leave the feed loading forever.
    if s.feed.status.loading {
        let since = *s.loading_since.get_or_insert(now);
        if s.feed.timed_out(now - since, LOAD_TIMEOUT) {
            s.loading_since = None;
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
        tx.instant();
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
    if s.story.is_some() {
        return on_story_key(s, ev, tx);
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
        StoryAction::SetType(tab) => feed_state::switch_tab(&state, tab),
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
            story.thread.activate();
            thread_moved(story, root, tx);
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
            story.thread.unfold();
            thread_moved(story, root, tx);
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
            .layout_with("feed-list", |rect| (rect[3] / ROW_HEIGHT).floor().max(0.0) as usize)
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
            story_main(story, &k, t),
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
        None => (feed_body(s, &k, t, accent, cols), toolbar, col()),
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
fn story_main(story: &Story, k: &Kit, t: tok::Theme) -> El {
    col()
        .key("story-main")
        .w_full()
        .h_full()
        .child(
            story_view::article_pane(story, k, t)
                .grow(1.0)
                .min_h(0.0),
        )
        .child(el().w_full().h(1.0).fill(t.separator))
        .child(story_view::story_footer(k, t))
}

fn feed_body(s: &App, k: &Kit, t: tok::Theme, accent: Color, cols: usize) -> El {
    let mut body = col().w_full().h_full();
    if let Some(error) = s.saved.error() {
        body = body.child(banner(error, t, accent));
    }
    // A failed ranking or window is a state with a retry, never a blank list.
    if let Some(error) = s.feed.status.error.as_deref() {
        body = body.child(feed_error(error, t, accent));
    } else if s.feed.status.loading && s.feed.stories.is_empty() {
        body = body.child(feed_loading(t));
    }
    body.child(
        feed_list(s, t, accent, cols)
            .scroll(ScrollSpec { stick_bottom: false })
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
fn feed_list(s: &App, t: tok::Theme, accent: Color, cols: usize) -> El {
    let shown = s.feed.stories.len();
    let per_column = shown.div_ceil(cols.max(1));
    let mut list = col()
        .key("feed-list")
        .w_full()
        .gap(if cols == 2 { 0.0 } else { 2.0 });
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
            line = line.child(story_row(s, i, t, accent).grow(1.0));
        }
        list = list.child(line);
    }
    list
}

fn story_row(s: &App, i: usize, t: tok::Theme, accent: Color) -> El {
    let story = &s.feed.stories[i];
    let id = story.id;
    let rank = s.feed.display_cursor + i + 1;
    let selected = i == s.feed.selected;
    let failed = s.feed.failed.contains(&id);
    let saved = s.saved.contains(id);
    let in_saved_view = s.feed.tab == StoryType::Saved;

    let mut r = row()
        .key(format!("story-{id}"))
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
    if selected {
        r = r.child(el().w(2.0).h(26.0).radius(1.0).fill(accent));
    }
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
        r = r.fill(t.card);
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
fn act(
    label: &str,
    color: Color,
    t: tok::Theme,
    on_press: impl Fn(&mut App) + 'static,
) -> El {
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
        .w_full()
        .px(24.0)
        .py(12.0)
        .gap(18.0)
        .center_y()
        .child(hint(k, t, &["j", "k"], "move"))
        .child(hint(k, t, &["s"], if s.feed.tab == StoryType::Saved { "unsave" } else { "save" }))
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
    if s.feed.tab != StoryType::Saved {
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
        .child(chev(
            "\u{2039}",
            "Previous page",
            t,
            |s: &mut App| {
                let cells = s.cells();
                let action = feed_state::prev_page(&s.feed.state(), cells);
                s.apply(action);
            },
        ))
        .child(
            row()
                .w(20.0)
                .center()
                .child(text(format!("{page}")).font(type_::LABEL).color(accent)),
        )
        .child(chev(
            "\u{203a}",
            "Next page",
            t,
            |s: &mut App| {
                let cells = s.cells();
                let action = feed_state::next_page(&s.feed.state(), cells);
                s.apply(action);
            },
        ))
}

/// One arrow of the page stepper. It is a button, not decoration: a press turns the page.
fn chev(
    glyph: &str,
    label: &str,
    t: tok::Theme,
    on_press: impl Fn(&mut App) + 'static,
) -> El {
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

/// The first load: the window is empty because nothing has landed yet, not because the feed
/// failed.
fn feed_loading(t: tok::Theme) -> El {
    row()
        .key("feed-loading")
        .role("status")
        .w_full()
        .px(24.0)
        .py(10.0)
        .child(
            text("Loading stories")
                .font(type_::LABEL)
                .color(t.text_secondary),
        )
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

    /// The inspector the window builds: 543 points, clamped to 360..=640, collapsed.
    fn inspector() -> PaneState {
        PaneState::new(INSPECTOR_WIDTH)
            .range(INSPECTOR_MIN, INSPECTOR_MAX)
            .collapsed()
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
            loading_since: None,
            hover: None,
            story: None,
            inspector: inspector(),
            config: Config::default(),
            settings: None,
            help: false,
            layout: Layout::default(),
            layout_path: None,
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
