//! Feed loading, arrival and resize request behavior.

use super::super::*;
use std::time::Duration;
use hn_lib::app::StoryType;
use rune::tok;
use rune::col;
use rune::AppSpec;
use rune::Cx;
use rune::El;
use rune::KeyEvent;
use rune::Tx;
use super::super::feed_view::feed_skeleton;
use super::super::input::on_key;
use super::super::poll::poll;
use super::super::window::view;
use super::support::*;

#[test]
fn resize_retargets_pending_reflow_but_not_a_page_turn() {
    for page_turn in [false, true] {
        let mut s = app(true, SavedView::in_memory());
        s.measured.set(Some((14, 2)));
        s.request((14, 2), Ranking::Fresh);
        s.feed.selected = 27;
        s.headless = false;
        s.source = Arc::new(Source::Fake(crate::service::fake::FakeService::fixture()
            .with_delay(Duration::from_millis(30))));
        if page_turn { press(&mut s, rune::Key::Down); }
        s.measured.set(Some((13, 1)));
        poll(&mut s, 0.0, &mut Tx::default());
        assert_eq!(s.feed.pending_cursor, Some(if page_turn { 28 } else { 26 }));
        s.measured.set(Some((11, 1)));
        poll(&mut s, 0.0, &mut Tx::default());
        assert_eq!(s.feed.pending_cursor, Some(if page_turn { 28 } else { 22 }));
        settle_feed(&mut s);
        assert_eq!(s.feed.selected_id(), Some(if page_turn { 29 } else { 28 }));
        assert_eq!(s.feed.display_cursor, if page_turn { 28 } else { 22 });
        assert!(s.feed.story_cache.contains_key(&1), "reflow retains the earlier page");
    }
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
    while s.feed.request.is_loading() {
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
    while s.feed.request.is_loading() {
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
    while s.feed.request.is_loading() {
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
    let mut s = app(false, SavedView::in_memory());
    s.request(DEFAULT_GRID, Ranking::Fresh);
    s.headless = true;
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
        s.source = Arc::new(Source::Fake(crate::service::fake::FakeService::fixture()
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
        while rt.state.feed.request.is_loading() { rt.frame(0.0); assert!(std::time::Instant::now() < deadline); }
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
            while rt.state.feed.request.is_loading() { rt.frame(0.0); assert!(std::time::Instant::now() < deadline); }
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
        let child = if s.feed.request.is_loading() {
            col().key("new-parent").w_full().child(feed_skeleton(s, tok::theme(cx.system_dark()), 1, false))
        } else { col().key("old-parent").w_full() };
        col().w_full().h_full().child(child)
    }
    let mut rt = rune::Runtime::new(app(true, SavedView::in_memory()), stage, (1180.0, 760.0), rune::Env::new(true, 1.0));
    rt.frame(0.0);
    rt.update(|s, tx| { s.headless = false; s.request(DEFAULT_GRID, Ranking::Fresh); tx.animated(); });
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
    s.source = Arc::new(Source::Fake(crate::service::fake::FakeService::fixture()
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
    assert!(rt.state.feed.error.is_some());
    rt.update(|_, tx| tx.instant());
    rt.frame(0.0);
    assert!(!rt.accessibility_tree().nodes.iter().any(|(_, node)| node.is_busy()));

    // A failed refresh settles with the same held rows and no busy status.
    let failing = crate::service::fake::FakeService::fixture();
    failing.fail_ranking(StoryType::Top);
    rt.state.source = Arc::new(Source::Fake(failing));
    rt.state.refresh();
    poll_until(&mut rt.state, |s| !s.feed.request.is_loading());
    assert!(rt.state.feed.error.is_some());
    assert_eq!(rt.state.feed.stories[0].id, 1);

    rt.state.source = Arc::new(Source::Fake(crate::service::fake::FakeService::fixture()
        .with_delay(Duration::from_millis(50))));
    rt.state.refresh();
    poll_until(&mut rt.state, |s| !s.feed.request.is_loading());
    assert!(rt.state.feed.error.is_none());
    let page_size = rt.state.measured.get().unwrap_or(DEFAULT_GRID).0;
    press(&mut rt.state, rune::Key::Text("n".into()));
    rt.update(|_, tx| tx.instant());
    rt.frame(0.0);
    assert!(rt.elements().iter().any(|e| e.label.as_deref() == Some("Loading page…")));
    assert_eq!(rt.accessibility_tree().nodes.iter().filter(|(_, node)| node.is_busy()).count(), 1);
    assert_eq!(rt.state.feed.stories[0].id, 1);
    poll_until(&mut rt.state, |s| !s.feed.request.is_loading());
    assert_eq!(rt.state.feed.stories[0].id, 1 + page_size as i32);
    press(&mut rt.state, rune::Key::Text("p".into()));
    assert!(!rt.state.feed.request.is_loading(), "cached return settles before a frame can show busy");
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

/// Explicit renderer fixture: keeps GPU setup out of the ordinary unit-test suite.
#[test]
#[ignore = "Run explicitly with Metal access for loading visual/AX verification"]
fn render_loading_shell_fixtures() {
    fn loading() -> App {
        let mut s = app(false, SavedView::in_memory());
        s.request(DEFAULT_GRID, Ranking::Fresh);
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
    assert!(!s.feed.request.is_loading());
    assert!(s.feed.stories.is_empty());
    let saved_revision = s.feed_revision;
    s.headless = false;
    s.source = Arc::new(Source::Fake(crate::service::fake::FakeService::fixture()
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
    s.source = Arc::new(Source::Fake(crate::service::fake::FakeService::fixture()
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
    assert!(s.feed.request.is_loading() && s.feed.error.is_none());
}

#[test]
fn retry_clears_feed_error_and_settled_empty_is_distinct_from_loading() {
    let mut s = app(true, SavedView::in_memory());
    s.request((11, 1), Ranking::Fresh);
    if let Source::Fake(fake) = s.source.as_ref() { fake.fail_ranking(StoryType::New); }
    press(&mut s, rune::Key::Text("2".into()));
    assert!(s.feed.error.is_some());
    s.source = Arc::new(Source::Fake(crate::service::fake::FakeService::empty()
        .with_delay(Duration::from_millis(10))));
    s.headless = false;
    s.refresh();
    assert!(s.feed.request.is_loading() && s.feed.error.is_none());
    assert_eq!(s.arrival, Arrival::None, "retry must not replay stale tab motion");
    settle_feed(&mut s);
    assert!(!s.feed.request.is_loading() && s.feed.error.is_none());
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
        s.request(DEFAULT_GRID, Ranking::Fresh);
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
        wait_worker(s);
        poll(s, now, tx)
    }
    fn page(s: &mut App, now: f64, tx: &mut Tx) -> bool {
        if now < 0.2 { return false; }
        if s.feed_revision == 0 && !s.feed.request.is_loading() {
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
    while s.feed.request.is_loading() {
        poll(s, 0.0, &mut Tx::default());
        assert!(std::time::Instant::now() < deadline, "fake feed must settle");
        std::thread::yield_now();
    }
}

#[test]
fn tab_switch_resize_keeps_the_new_tabs_fresh_request() {
    let mut s = app(true, SavedView::in_memory());
    s.request((11, 1), Ranking::Fresh);
    s.source = Arc::new(Source::Fake(crate::service::fake::FakeService::fixture()
        .with_delay(Duration::from_millis(50))));
    s.headless = false;
    press(&mut s, rune::Key::Text("2".into()));
    s.measured.set(Some((10, 1)));
    poll(&mut s, 0.0, &mut Tx::default());
    assert!(s.feed.request.is_loading(), "resize must not settle the previous tab's cache");
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
    let fake = crate::service::fake::FakeService::fixture().with_delay(Duration::from_millis(50));
    fake.set_ranking(StoryType::Top, vec![9, 8, 7, 6]);
    let mut item = crate::fixtures::item(8, 9);
    item.title = "Fresh title after resize".into();
    fake.insert_item(item);
    s.source = Arc::new(Source::Fake(fake));
    s.headless = false;
    s.refresh();
    s.measured.set(Some((10, 1)));
    poll(&mut s, 0.0, &mut Tx::default());
    assert!(s.feed.request.is_loading(), "resize must not cancel an explicit refresh");
    assert_eq!(s.feed.stories[0].id, 1, "held rows stay until the fresh window lands");
    settle_feed(&mut s);
    assert_eq!(s.feed.id_snapshot, vec![9, 8, 7, 6]);
    assert_eq!(s.feed.stories[0].title, "Fresh title after resize");
}

#[test]
fn failed_tab_load_then_resize_never_reuses_the_previous_tabs_snapshot() {
    let mut s = app(true, SavedView::in_memory());
    s.request((11, 1), Ranking::Fresh);
    let fake = crate::service::fake::FakeService::fixture().with_delay(Duration::from_millis(50));
    fake.fail_ranking(StoryType::New);
    s.source = Arc::new(Source::Fake(fake));
    s.headless = false;
    press(&mut s, rune::Key::Text("2".into()));
    settle_feed(&mut s);
    assert!(s.feed.error.is_some());
    s.measured.set(Some((10, 1)));
    poll(&mut s, 0.0, &mut Tx::default());
    settle_feed(&mut s);
    assert_eq!(s.feed.tab, StoryType::New);
    assert!(s.feed.stories.is_empty(), "a failed New load must not display cached Top rows");
    assert!(s.feed.error.is_some());
    assert!(s.feed.stories_for.is_none());
}

