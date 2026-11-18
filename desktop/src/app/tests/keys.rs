//! Keyboard routing, help and modal shortcuts.

use super::super::*;
use std::time::Duration;
use hn_lib::app::StoryType;
use rune::KeyEvent;
use rune::Tx;
use super::super::input::on_key;
use super::super::poll::poll;
use super::super::scenes::scene_comments;
use super::super::window::view;
use super::support::*;

#[test]
fn feed_settings_key_is_a_plain_capital_m_desktop_exception() {
    let mut s = app(true, SavedView::in_memory());
    for (key, mods) in [
        ("m", rune::Mods::default()),
        ("M", rune::Mods { ctrl: true, ..Default::default() }),
    ] {
        assert!(!on_key(&mut s, &KeyEvent { key: rune::Key::Text(key.into()), mods }, &mut Tx::default()));
        assert!(s.settings.is_none());
    }
    let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    rt.frame(0.0);
    rt.key(KeyEvent { key: rune::Key::Text("M".into()), mods: rune::Mods { shift: true, ..Default::default() } });
    rt.frame(0.3);
    assert!(rt.elements().iter().any(|e| e.key.as_deref() == Some("settings-dialog")));
    assert!(rt.state.story.is_none());
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
fn full_feed_has_fixed_top_inset_and_pointer_tabs_return_navigation_to_rows() {
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
        let (_, cols) = rt.state.measured.get().unwrap();
        let top = first[1] - viewport[1];
        assert!((top - FEED_GUTTER).abs() < 0.5, "{size:?}: top inset {top}");
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
        assert!((line[1] - viewport[1] - FEED_GUTTER).abs() < 0.5);
        rt.state.bridge.cancel_feed();
        rt.state.headless = true;
        rt.poll = Some(poll);
        rt.set_viewport(if cols == 1 { (1600.0, 900.0) } else { (800.0, 640.0) }, 1.0);
        for _ in 0..4 { rt.frame(0.25); }
        let (_, new_cols) = rt.state.measured.get().unwrap();
        assert_ne!(cols, new_cols, "resize crosses the column breakpoint");
        let elements = rt.elements();
        let viewport = elements.iter().find(|e| e.key.as_deref() == Some("feed-list")).unwrap().rect;
        let first = elements.iter().find(|e| e.key.as_deref() == Some("story-1")).unwrap().rect;
        assert!((first[1] - viewport[1] - FEED_GUTTER).abs() < 0.5,
            "resized full pages retain the top inset: {size:?} {new_cols} {viewport:?} {first:?}");
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
    s.source = Arc::new(Source::Fake(crate::service::fake::FakeService::fixture().with_delay(Duration::from_millis(50))));
    s.headless = false;
    let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    rt.frame(0.0);
    route(&mut rt, rune::Key::Text("2".into()));
    route(&mut rt, rune::Key::Down);
    assert_eq!(rt.state.feed.tab, StoryType::New);
    assert!(rt.state.feed.request.is_loading(), "worker delivery is held until pending input is checked");
    assert_eq!(rt.state.feed.selected, 0);
    rt.poll = Some(poll);
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while rt.state.feed.request.is_loading() {
        std::thread::yield_now();
        rt.frame(0.0);
        assert!(std::time::Instant::now() < deadline);
    }
    route(&mut rt, rune::Key::Down);
    assert_eq!(rt.state.feed.tab, StoryType::New);
    assert_eq!(rt.state.feed.selected, 1);
}

