//! Feed paging, selection and ranking behavior.

use super::super::*;
use std::time::Duration;
use hn_lib::app::StoryType;
use hn_lib::feed_state;
use rune::KeyEvent;
use rune::Tx;
use super::super::input::on_key;
use super::super::poll::poll;
use super::super::scenes::scene_story;
use super::super::window::view;
use super::support::*;

#[test]
fn keyboard_selection_is_instant_and_feed_hover_eases() {
    let mut s = app(true, SavedView::in_memory());
    s.request(DEFAULT_GRID, Ranking::Fresh);
    let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    rt.frame(0.0);
    let full = rt.elements().iter().find(|e| e.key.as_deref() == Some("story-1")).unwrap().fill;
    rt.key(KeyEvent { key: rune::Key::Text("j".into()), mods: Default::default() });
    rt.frame(0.016);
    let after = rt.elements();
    assert_eq!(after.iter().find(|e| e.key.as_deref() == Some("story-2")).unwrap().fill, full);
    assert_eq!(after.iter().find(|e| e.key.as_deref() == Some("story-1")).unwrap().fill.a, 0.0);
    let [x, y, w, h] = after.iter().find(|e| e.key.as_deref() == Some("story-3")).unwrap().rect;
    rt.pointer_move(x + w / 2.0 - 40.0, y + h / 2.0);
    rt.frame(0.0);
    rt.pointer_move(x + w / 2.0, y + h / 2.0);
    rt.frame(0.0);
    rt.frame(0.05);
    let during = rt.elements().iter().find(|e| e.key.as_deref() == Some("story-3")).unwrap().fill.a;
    rt.key(KeyEvent { key: rune::Key::Text("s".into()), mods: Default::default() });
    rt.frame(0.016);
    let after_save = rt.elements().iter().find(|e| e.key.as_deref() == Some("story-3")).unwrap().fill.a;
    rt.frame(0.3);
    let settled = rt.elements().iter().find(|e| e.key.as_deref() == Some("story-3")).unwrap().fill.a;
    assert!(during > 0.0 && during < after_save && after_save < settled && (settled - full.a).abs() < 0.001,
        "hover {during} -> {after_save} -> {settled}");

    let mut s = app(true, SavedView::in_memory());
    scene_story(&mut s);
    let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    rt.frame(0.0);
    let selected = rt.elements().iter().find(|e| e.key.as_deref() == Some("comment-text-901")).unwrap().text_color;
    rt.key(KeyEvent { key: rune::Key::Text("j".into()), mods: Default::default() });
    rt.frame(0.016);
    assert_eq!(rt.state.story.as_ref().unwrap().thread.cursor(), Some(9010));
    let elements = rt.elements();
    assert_eq!(elements.iter().find(|e| e.key.as_deref() == Some("comment-text-9010")).unwrap().text_color, selected);
    assert!(!elements.iter().any(|e| e.key.as_deref() == Some("focus-rail-901") && e.opacity > 0.001));
    assert_eq!(elements.iter().find(|e| e.key.as_deref() == Some("focus-rail-9010") && !e.exiting).unwrap().opacity, 1.0);
    let dimmed = elements.iter().find(|e| e.key.as_deref() == Some("comment-text-901")).unwrap().text_color;
    let [x, y, w, h] = elements.iter().find(|e| e.key.as_deref() == Some("comment-901")).unwrap().visible;
    rt.pointer_move(x + w / 2.0 - 40.0, y + h / 2.0);
    rt.frame(0.0);
    rt.pointer_move(x + w / 2.0, y + h / 2.0);
    rt.frame(0.0);
    rt.frame(0.05);
    let during = rt.elements().iter().find(|e| e.key.as_deref() == Some("comment-text-901")).unwrap().text_color;
    rt.frame(0.3);
    let settled = rt.elements().iter().find(|e| e.key.as_deref() == Some("comment-text-901")).unwrap().text_color;
    assert_eq!(rt.state.story.as_ref().unwrap().thread.cursor(), Some(901));
    assert!(during != dimmed && during != settled && settled == selected, "discussion hover must ease");
    rt.key(KeyEvent { key: rune::Key::Text("j".into()), mods: Default::default() });
    rt.frame(0.016);
    assert!(!rt.elements().iter().any(|e| e.key.as_deref() == Some("focus-rail-901") && e.opacity > 0.001));
}

#[test]
fn desktop_arrows_follow_rendered_columns_and_clamp_short_columns() {
    let mut s = app(true, SavedView::in_memory());
    s.measured.set(Some(grid(14, 2)));
    s.request(grid(14, 2), Ranking::Fresh);
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
    s.request(grid(11, 1), Ranking::Fresh);
    s.feed.selected = 5;
    press(&mut s, rune::Key::Left);
    assert_eq!(s.feed.selected, 5);
    s.measured.set(Some(grid(11, 2)));
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
    s.headless = false;
    s.request(grid(11, 2), Ranking::Fresh);
    press(&mut s, rune::Key::Right);
    assert_eq!(s.feed.selected, 5);
    s.bridge.cancel_feed();
    s.headless = true;
    s.request(grid(11, 2), Ranking::Fresh);
    s.feed.stories.clear();
    press(&mut s, rune::Key::Right);
    assert_eq!(s.feed.display_cursor, 0);
    assert_eq!(s.feed_revision, 0);
}

#[test]
fn edge_arrows_turn_pages_and_land_at_the_matching_side() {
    for cols in [1, 2] {
        let mut s = app(true, SavedView::in_memory());
        s.measured.set(Some(grid(5, cols)));
        s.request(grid(5, cols), Ranking::Fresh);
        s.feed.selected = s.feed.stories.len() - 1;
        press(&mut s, rune::Key::Right);
        assert_eq!(s.feed.display_cursor, 5 * cols);
        assert_eq!(s.feed.selected, 0);
        assert_eq!(s.arrival, Some(Arrival::Page(1)));
        press(&mut s, rune::Key::Left);
        assert_eq!(s.feed.display_cursor, 0);
        assert_eq!(s.feed.selected, 5 * cols - 1);
        assert_eq!(s.arrival, Some(Arrival::Page(-1)));
    }
}

#[test]
fn arrow_paging_stops_at_known_ends_and_saved() {
    let mut s = app(true, SavedView::in_memory());
    s.measured.set(Some(grid(5, 2)));
    s.request(grid(5, 2), Ranking::Fresh);
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
    s.request(grid(11, 1), Ranking::Fresh);
    press(&mut s, rune::Key::Text("n".into()));
    let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
    let before = *fake.requests.lock().unwrap();
    press(&mut s, rune::Key::Text("p".into()));
    assert!(!s.feed.request.is_loading());
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
    s.request(grid(11, 1), Ranking::Keep);
    assert!(s.feed.failed.contains(&12));
    let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
    let failed = fake.requests.lock().unwrap().1;
    s.request(grid(11, 1), Ranking::Keep);
    let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
    assert!(fake.requests.lock().unwrap().1 > failed, "failure placeholders are not cache hits");
}

#[test]
fn a_cached_window_cancels_only_the_superseded_feed_worker() {
    let mut s = app(true, SavedView::in_memory());
    s.request(grid(11, 1), Ranking::Fresh);
    s.source = Arc::new(Source::Fake(crate::service::fake::FakeService::fixture()
        .with_delay(Duration::from_millis(50))));
    s.headless = false;
    s.request(grid(11, 1), Ranking::Fresh);
    s.open_selected();
    assert!(s.bridge.is_pending());
    assert!(s.bridge.is_story_pending());
    s.request(grid(11, 1), Ranking::Keep);
    assert!(!s.feed.request.is_loading());
    assert!(!s.bridge.is_pending(), "a held window supersedes the feed worker");
    assert!(s.bridge.is_story_pending(), "feed cache must not cancel the independent story request");
    let title = s.feed.stories[0].title.clone();
    std::thread::sleep(Duration::from_millis(80));
    poll(&mut s, 0.0, &mut Tx::default());
    assert_eq!(s.feed.stories[0].title, title);
    assert!(!s.feed.request.is_loading());
}

#[test]
fn a_page_turn_pages_against_the_snapshot_and_only_a_refresh_re_ranks() {
    let mut s = app(true, SavedView::in_memory());
    s.request(grid(11, 1), Ranking::Fresh);
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
    s.request(grid(11, 1), Ranking::Fresh);
    let before = s.feed.stories.len();
    if let Source::Fake(fake) = s.source.as_ref() {
        fake.fail_ranking(StoryType::Top);
    }
    s.refresh();
    assert_eq!(
        s.feed.error.as_deref(),
        Some("the ranking request failed"),
        "the failure is a state the view renders with a retry"
    );
    assert_eq!(s.feed.stories.len(), before, "the feed is not blanked");
    assert!(!s.feed.request.is_loading());
}


#[test]
fn keyboard_selection_survives_pointer_jitter_in_feed() {
    let mut s = app(true, SavedView::in_memory());
    s.request(DEFAULT_GRID, Ranking::Fresh);
    let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    rt.frame(0.0);
    let [x, y, w, h] = rt.elements().iter()
        .find(|e| e.key.as_deref() == Some("story-1")).unwrap().visible;
    let (px, py) = (x + w / 2.0, y + h / 2.0);
    rt.pointer_move(px, py);
    rt.frame(0.0);

    for (key, expected, nudge) in [("j", 2, 1.0), ("j", 3, 2.0), ("k", 2, 3.0)] {
        route(&mut rt, rune::Key::Text(key.into()));
        assert_eq!(rt.state.feed.selected_id(), Some(expected));
        rt.pointer_move(px + nudge, py);
        rt.frame(0.1);
        assert_eq!(rt.state.feed.selected_id(), Some(expected), "{key} keeps selection after a 1 pt nudge");
    }
    rt.pointer_move(px + 43.0, py);
    rt.frame(0.1);
    assert_eq!(rt.state.feed.selected_id(), Some(1));
}
