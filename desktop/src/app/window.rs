//! Window composition, startup and native menus.

use super::{App, Arrival, Pane, Live, RequestLifecycle, OPEN_STORY, ROW_HEIGHT, BREAKPOINT, FEED_GUTTER, DEFAULT_FEED_HEIGHT, INSPECTOR_MAX, init};
use hn_lib::app::StoryType;
use rune::tok;
use rune::tok::type_;
use rune::col;
use rune::el;
use rune::row;
use rune::text;
use rune::AppSpec;
use rune::Cx;
use rune::El;
use rune::KeyEvent;
use rune::Spring;
use rune::SpringPair;
use rune::Transition;
use rune::Tx;
use rune_kit::Kit;
use rune_kit::SplitView;
use rune_kit::Toast;
use rune_kit::ToastRegion;
use rune_kit::Toolbar;
#[cfg(target_os = "macos")]
use crate::keys::WindowKey;
use crate::story::Story;
use super::chrome::{brand, pane_motion, saved_dot};
use super::feed_view::{feed_arrival, feed_body, feed_tabs};
use super::input::{on_key, on_story_key};
use super::poll::{poll, setup};
use super::scenes::*;
use crate::{help, story_view};

pub(super) fn view(s: &App, cx: &mut Cx<App>) -> El {
    cx.zoom(s.zoom.scale());
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
        s.window_size.set(Some((rect[2] * s.zoom.scale(), rect[3] * s.zoom.scale())));
        let width = if s.story.is_some() { rect[2] * s.zoom.scale() } else { INSPECTOR_MAX * 2.0 };
        s.resize_inspector(width);
    }
    // Tx has no keyed visibility query; scroll observers supply offsets without view commits.
    let visible = s.story.as_ref().and_then(|story| {
        let source = story.source();
        let id = source.map_or_else(|| story.thread.cursor(), |view| view.nav.cursor)?;
        let viewport = cx.layout_with(if source.is_some() { "source-viewer" } else { "discussion-scroll" }, |r| [r[1], r[3]])?;
        let row = cx.layout_with(&format!("{}comment-{id}", if source.is_some() { "source-" } else { "" }), |r| [r[1], r[3]])?;
        Some((id, source.is_some(), row, viewport))
    });
    s.visible_comment.set(visible);
    let wide = window.is_some_and(|rect| rect[2] >= BREAKPOINT);
    let cols = if wide { super::Columns::Two } else { super::Columns::One };
    // The feed's grid is only measurable while the feed is on screen: a story open must not
    // overwrite it with the default and make `poll` re-request the wrong window.
    let feed_height = cx.layout_with("feed-list", |rect| rect[3]).unwrap_or(DEFAULT_FEED_HEIGHT);
    if s.story.is_none() {
        let gap = if cols == super::Columns::Two { 0.0 } else { 2.0 };
        let rows = ((feed_height - 2.0 * FEED_GUTTER + gap) / (ROW_HEIGHT + gap)).floor() as usize;
        s.measured.set(Some(super::Grid::measured(rows, cols)));
    }
    // Saved is one scrollable list. Change its columns in the same commit that reveals the
    // selection at its new position, rather than putting it offscreen for a frame.
    let feed_cols = match (s.feed.tab, s.feed.request) {
        (StoryType::Saved, RequestLifecycle::Loading(_, _, grid, _) | RequestLifecycle::Settled(_, _, grid)) => grid.columns,
        _ => cols,
    };

    let handoff = if reduced { Transition::none() } else {
        Transition::asymmetric(Transition::opacity(), Transition::none())
            .springs(SpringPair::new(Spring::new(0.15, 0.0), Spring::NONE))
    };
    let returning = s.arrival == Some(Arrival::Back);
    let feed_handoff = if returning && !reduced { feed_arrival(-24.0, 0.0) } else { handoff };
    let mut reading_handoff = Transition::opacity();
    reading_handoff.springs.exit = Spring::NONE;
    if reduced { reading_handoff = Transition::none(); }
    let tabs = feed_tabs(s, &k, t, accent, reduced).transition(feed_handoff);
    let mut toolbar = Toolbar::new().leading(if reduced { tabs.instant_changes() } else { tabs });
    let trailing = if s.feed.tab == StoryType::Saved {
        row().gap(8.0).center_y().child(text(s.saved.path_label())
                .font(type_::LABEL)
                .color(t.text_secondary)
                .truncate_middle()).child(brand(t))
    } else { brand(t) };
    toolbar = toolbar.trailing(if reduced { trailing.transition(Transition::none()).instant_changes() } else { trailing });

    // One window, one `SplitView`: the feed is this window with the inspector collapsed, so the
    // inspector's slot exists before a story opens and its width can spring out from 0.
    // The overlay layer is built before the panes, because a pane can own a surface in it: the
    // discussion's link chooser hangs off a chip inside the inspector.
    let mut overlays = Vec::new();
    let (main, toolbar, inspector) = match &s.story {
        Some(story) => (
            story_main(story, &k, t, s.pane()).transition(handoff),
            Toolbar::new().leading(story_view::reading_chrome::story_toolbar(
                t,
                accent,
                s.saved.contains(story.id()),
            ).transition(handoff)).trailing(brand(t)),
            story_view::inspector_pane(story, &k, t, accent, &mut overlays, reduced,
                window.map_or(360.0, |rect| (rect[3] - 310.0).clamp(230.0, 440.0)),
                [s.scroll_observer(Some(0)), s.scroll_observer(None), s.scroll_observer(Some(1))])
                .transition(reading_handoff),
        ),
        None => {
            let body = feed_body(s, &k, t, accent, feed_cols, reduced, contrast).transition(feed_handoff);
            let body = if returning || reduced { body.instant_changes() } else { body };
            (body, toolbar, col().transition(Transition::none()))
        }
    };
    let shell = SplitView::new(main)
        .toolbar(toolbar)
        .inspector(&s.inspector.get(), |s: &mut App| s.inspector.get_mut(), inspector)
        .titlebar_inset(cx.titlebar_inset())
        .pane_motion(if returning { rune_kit::shell::PaneMotion::new().spring(Spring::NONE) } else { pane_motion() })
        .reduced_motion(reduced)
        .build(&k);

    let shell = if returning { shell.motion(Spring::NONE) } else { shell };
    let mut base = col().key("window").w_full().h_full().fill(t.bg)
        .on_key(|s: &mut App, ev: &KeyEvent, tx: &mut Tx| {
            if ev.key != rune::Key::Tab || s.story.is_none() || s.settings.is_some() || s.help
                || s.story.as_ref().is_some_and(|story| story.links.is_some()) { return false; }
            s.keyboard_navigation(ev);
            let handled = on_story_key(s, ev, tx);
            if handled {
                let target = if s.story.as_ref().is_some_and(|story| story.source().is_some()) { "source-viewer" } else { "summary-scroll" };
                tx.focus(target, true);
            }
            handled
        })
        .child(shell.grow(1.0).min_h(0.0));
    if let Some(story) = &s.story {
        base = base.child(el().w_full().h(1.0).fill(t.separator).transition(reading_handoff))
            .child(story_view::reading_chrome::story_footer(story, &k, t, &s.hints, reduced, contrast)
                .transition(reading_handoff));
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

/// The main pane while a story is open.
fn story_main(story: &Story, k: &Kit, t: tok::Theme, pane: Pane<'_>) -> El {
    col()
        .key("story-main")
        .w_full()
        .h_full()
        .child(
            story_view::article::article_pane(story, k, t, pane)
                .grow(1.0)
                .min_h(0.0),
        )
}

#[cfg(target_os = "macos")]
pub(super) const WINDOW_MENUS: [(&str, Option<&str>, &str, fn(&mut App, &mut Tx)); 6] = [
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
    let legacy_size = std::rc::Rc::new(std::cell::Cell::new(None));
    #[cfg(target_os = "macos")]
    {
        let initial_size = std::rc::Rc::clone(&legacy_size);
        app = native_menus(app).init(move || {
            let state = init();
            initial_size.set(state.layout.window);
            state
        });
    }
    if on_top {
        app = app.window_options(rune::WindowOptions {
            always_on_top: true,
            ..Default::default()
        });
    }
    let handle = app.handle();
    app.on_window(move |window| {
        #[cfg(target_os = "macos")]
        if let Err(error) = crate::webview::remember_window_frame(window, legacy_size.take()) {
            eprintln!("could not restore window frame: {error}");
        }
        let window = window.clone();
        handle.update(move |s: &mut App, _| {
            s.native_window = Some(window);
            s.live = Live::Idle;
            s.live_bounds = None;
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
pub(super) fn close(s: &mut App, _tx: &mut Tx) {
    if !s.headless {
        let snapshot = s.snapshot();
        if snapshot != s.layout_written {
            s.write_layout(snapshot, rune::scene_clock().now);
        }
    }
    s.live = Live::Idle;
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
            ("summary-saved", scene_summary_saved),
            ("settings", scene_settings),
            ("settings-story", scene_settings_story),
            ("help", scene_help),
        ],
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(on_key),
        ..AppSpec::new("Hacker News", (1180.0, 760.0), init, view)
    }
}
