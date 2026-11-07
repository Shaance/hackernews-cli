//! The window: `SplitView` shell, the responsive feed, and the Saved view.
//!
//! The view is a pure function of [`App`]; the only thing it writes back is the grid it
//! measured, because `view` takes `&S` and `poll` needs the number. Everything else — the
//! ranked window, the page stepper, the selection — is `hn_lib::feed_state`'s arithmetic
//! applied by [`FeedModel`].

use std::cell::Cell;
use std::sync::Arc;
use std::time::Duration;

use hn_lib::app::StoryType;
use hn_lib::event::StoryAction;
use hn_lib::feed_state;
use hn_lib::HNCLIItem;
use rune::tok::{self, radius, type_};
use rune::{AppSpec, Color, Cx, El, KeyEvent, ScrollSpec, Tx, Waker, col, el, row, text};
use rune_kit::{Keybind, Kit, SegmentedControl, SplitView, Toast, ToastRegion, Toolbar};

use crate::bookmarks::SavedView;
use crate::bridge::{Bridge, Request};
use crate::feed::{FeedModel, Fill};
use crate::keys;
use crate::service::Source;

/// One feed row, in points. The measured row count is the viewport height over this.
const ROW_HEIGHT: f32 = 56.0;
/// Window width at which the feed becomes two columns (DESIGN.md's table).
const BREAKPOINT: f32 = 1400.0;
/// The grid the first request asks for: the 1180×760 default window, one column.
const DEFAULT_GRID: (usize, usize) = (11, 1);
/// How long the save confirmation stays.
const TOAST_SECONDS: f64 = 2.4;

/// The transient save confirmation. `until` is on the frame clock, so `poll` expires it.
struct Notice {
    message: String,
    until: f64,
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
    /// The row under the pointer, for the hover actions.
    hover: Option<i32>,
    toast: Option<Notice>,
    /// `HN_REDUCED_MOTION=1`, the same override the terminal app honours.
    reduced: bool,
    /// Headless runs use the fake service and an in-memory Saved store, so a golden never
    /// depends on the network or on the machine's `saved.json`.
    headless: bool,
}

impl App {
    fn request(&mut self, grid: (usize, usize)) {
        let fill = self.feed.fill(grid.0, grid.1);
        self.requested = Some(fill);
        let request = if fill.tab == StoryType::Saved {
            Request::against(fill, self.saved.ids())
        } else {
            Request::new(fill)
        };
        if self.headless {
            let filled = self.bridge.fill_now(self.source.as_ref(), request);
            self.feed.apply_fill(filled);
        } else {
            self.bridge.request(self.source.clone(), request);
        }
    }

    /// Re-request the current window, as `r` does.
    fn refresh(&mut self) {
        self.requested = None;
        let grid = self.measured.get().unwrap_or(DEFAULT_GRID);
        self.request(grid);
    }

    fn toggle_saved(&mut self) {
        let Some(story) = self.feed.stories.get(self.feed.selected).cloned() else {
            return;
        };
        if self.feed.failed.contains(&story.id) {
            return;
        }
        let saved = self.saved.toggle(&story);
        self.toast = Some(Notice {
            message: if saved {
                "Saved · Press 4 for Saved".to_string()
            } else {
                "Removed from Saved".to_string()
            },
            until: f64::INFINITY,
        });
        if self.feed.tab == StoryType::Saved {
            self.refresh();
        }
    }

    fn apply(&mut self, action: feed_state::FeedAction) {
        let grid = self.measured.get().unwrap_or(DEFAULT_GRID);
        let applied = self.feed.apply(action, grid);
        if let Some(fill) = applied.fill {
            self.requested = Some(fill);
            let request = if fill.tab == StoryType::Saved {
                Request::against(fill, self.saved.ids())
            } else {
                Request::new(fill)
            };
            if self.headless {
                let filled = self.bridge.fill_now(self.source.as_ref(), request);
                self.feed.apply_fill(filled);
            } else {
                self.bridge.request(self.source.clone(), request);
            }
        }
    }

    fn cells(&self) -> usize {
        let (rows, cols) = self.measured.get().unwrap_or(DEFAULT_GRID);
        rows * cols
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
    App {
        feed: FeedModel::new(),
        saved,
        source: Arc::new(source),
        bridge: Bridge::new().expect("a tokio runtime for the feed's requests"),
        measured: Cell::new(None),
        requested: None,
        hover: None,
        toast: None,
        reduced: std::env::var("HN_REDUCED_MOTION").is_ok_and(|value| value == "1"),
        headless,
    }
}

/// One aggregate fill request, then the waker that brings its answer back to `poll`.
fn setup(s: &mut App, waker: Waker) {
    s.bridge.set_waker(waker);
    s.request(DEFAULT_GRID);
}

fn poll(s: &mut App, now: f64, tx: &mut Tx) -> bool {
    let mut changed = false;
    if let Some(filled) = s.bridge.take() {
        changed |= s.feed.apply_fill(filled);
    }
    // One request per invalidation: the measured grid is the only thing that moves it.
    if let Some(grid) = s.measured.get() {
        let need = s.feed.fill(grid.0, grid.1);
        if s.requested != Some(need) {
            s.request(grid);
            changed = true;
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
    let cells = s.cells();
    let state = s.feed.state();
    let action = match keys::story_action(ev) {
        StoryAction::None => return false,
        StoryAction::Quit => {
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
        // Story open, comments and help are later workstreams; the window has nothing for
        // them yet, so it does not swallow the key.
        StoryAction::OpenUrl | StoryAction::ViewComments | StoryAction::ToggleHelp => return false,
    };
    s.apply(action);
    if let Some(id) = s.feed.selected_id() {
        tx.scroll_into_view(format!("story-{id}"));
    }
    true
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

    // The window root, not the shell's inset card: the breakpoint is a window width.
    let wide = cx
        .layout_with("window", |rect| rect[2] >= BREAKPOINT)
        .unwrap_or(false);
    let cols = if wide { 2 } else { 1 };
    let rows = cx
        .layout_with("feed-list", |rect| (rect[3] / ROW_HEIGHT).floor().max(0.0) as usize)
        .filter(|rows| *rows > 0)
        .unwrap_or(DEFAULT_GRID.0);
    s.measured.set(Some((rows, cols)));

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

    let shell = SplitView::new(feed_body(s, &k, t, accent, cols))
        .toolbar(toolbar)
        .titlebar_inset(cx.titlebar_inset())
        .reduced_motion(reduced)
        .build(&k);

    let base = col().key("window").w_full().h_full().fill(t.bg).child(shell);
    let mut overlays = Vec::new();
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
    rune_kit::root(base, overlays)
}

fn feed_body(s: &App, k: &Kit, t: tok::Theme, accent: Color, cols: usize) -> El {
    let mut body = col().w_full().h_full();
    if let Some(error) = s.saved.error() {
        body = body.child(banner(error, t, accent));
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
        r = r.child(
            row()
                .gap(6.0)
                .center_y()
                .child(act(
                    if in_saved_view { "unsave" } else { "save" },
                    accent,
                    t,
                ))
                .child(act("open", t.text_secondary, t)),
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
    let comments = match story.comments {
        Some(0) => "discuss".to_string(),
        Some(n) => format!("{n} comments"),
        None => "no comments".to_string(),
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

fn act(label: &str, color: Color, t: tok::Theme) -> El {
    row()
        .h(20.0)
        .px(7.0)
        .center()
        .radius(4.0)
        .fill(t.control_off)
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
        .child(chev("\u{2039}", t))
        .child(
            row()
                .w(20.0)
                .center()
                .child(text(format!("{page}")).font(type_::LABEL).color(accent)),
        )
        .child(chev("\u{203a}", t))
}

fn chev(glyph: &str, t: tok::Theme) -> El {
    row()
        .size(18.0, 18.0)
        .center()
        .radius(4.0)
        .fill(t.control_off)
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

pub fn spec() -> AppSpec<App> {
    AppSpec {
        scenes: &[
            ("feed", scene_feed),
            ("feed-wide", scene_feed),
            ("feed-saved", scene_feed_saved),
            ("feed-saved-view", scene_saved_view),
            ("feed-saved-corrupt", scene_saved_corrupt),
            ("resize", scene_resize),
        ],
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(on_key),
        ..AppSpec::new("Hacker News", (1180.0, 760.0), init, view)
    }
}
