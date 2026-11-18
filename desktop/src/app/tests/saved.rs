//! Saved stories, hover actions and persistence feedback.

use super::super::*;
use hn_lib::app::StoryType;
use rune::KeyEvent;
use super::super::input::on_key;
use super::super::poll::poll;
use super::super::scenes::scene_saved_corrupt;
use super::super::scenes::scene_story;
use super::super::window::view;
use super::support::*;

#[test]
fn saved_empty_and_unreadable_states_show_the_right_actions() {
    for corrupt in [false, true] {
        let mut s = app(true, SavedView::in_memory());
        if corrupt { scene_saved_corrupt(&mut s); } else { s.feed.tab = StoryType::Saved; }
        s.request(DEFAULT_GRID, Ranking::Fresh);
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.frame(0.0);
        let elements = rt.elements();
        let text = |value| elements.iter().any(|e| e.text.as_deref() == Some(value));
        assert_eq!(text("Press S on a story to save it"), !corrupt);
        assert!(!text("No saved stories yet"));
        assert_eq!(text("Your saved list could not be read. Nothing was changed."), corrupt);
        assert_eq!(text("expected value at line 1 column 2"), corrupt);
        assert_eq!(elements.iter().any(|e| e.key.as_deref() == Some("saved-reveal")), corrupt && cfg!(target_os = "macos"));
        for label in ["move", "column / page", "unsave", "page"] { assert!(!text(label), "{label}"); }
        assert!(text("tabs"));
        if corrupt {
            let path = elements.iter().find(|e| e.text.as_deref() == Some("~/.local/share/hn/saved.json")).unwrap().rect;
            let brand = elements.iter().find(|e| e.key.as_deref() == Some("brand")).unwrap().rect;
            assert!((brand[0] - path[0] - path[2] - 8.0).abs() < 0.5);
        }
    }
}

#[test]
fn saved_write_failure_is_not_reported_as_a_read_failure() {
    let dir = temp_dir("saved-write-banner");
    std::fs::create_dir_all(&dir).unwrap();
    let mut saved = SavedView::load_at(dir.join("blocked/saved.json"));
    std::fs::write(dir.join("blocked"), b"not a directory").unwrap();
    saved.toggle(&crate::fixtures::item(0, 1));
    assert!(saved.error().is_some());
    let mut rt = rune::Runtime::new(app(true, saved), view, (1180.0, 760.0), rune::Env::new(true, 1.0));
    rt.frame(0.0);
    assert!(rt.elements().iter().any(|e| e.text.as_deref() == Some("Your saved list could not be updated. Nothing was changed.")));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn save_dot_pops_and_toast_fades_above_footer() {
    for reduced in [false, true] {
        let mut s = app(true, SavedView::in_memory());
        s.reduced = reduced;
        s.request(DEFAULT_GRID, Ranking::Fresh);
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.on_key = Some(on_key);
        rt.poll = Some(poll);
        rt.frame(0.0);
        rt.key(KeyEvent { key: rune::Key::Text("s".into()), mods: Default::default() });
        rt.frame(0.0);
        rt.frame(0.05);
        let elements = rt.elements();
        let dot = elements.iter().find(|e| e.key.as_deref() == Some("saved-dot-1") && !e.exiting).unwrap();
        assert!(if reduced { dot.scale == 1.0 } else { dot.scale >= 0.6 && dot.scale < 1.0 }, "dot scale {}", dot.scale);
        let toast = elements.iter().find(|e| e.key.as_deref() == Some("toast-saved") && !e.exiting).unwrap();
        let footer = elements.iter().find(|e| e.key.as_deref() == Some("feed-footer")).unwrap();
        assert!(toast.rect[1] + toast.rect[3] < footer.rect[1], "toast {:?}, footer {:?}", toast.rect, footer.rect);
        rt.frame(0.4);
        assert!((rt.elements().iter().find(|e| e.key.as_deref() == Some("saved-dot-1")).unwrap().scale - 1.0).abs() < 0.001);
        rt.key(KeyEvent { key: rune::Key::Text("s".into()), mods: Default::default() });
        rt.frame(0.05);
        let elements = rt.elements();
        assert!(elements.iter().any(|e| e.key.as_deref() == Some("toast-saved") && e.label.as_deref() == Some("Removed from Saved")));
        assert!(!elements.iter().any(|e| e.key.as_deref() == Some("toast-dot") && e.opacity > 0.001));
        rt.key(KeyEvent { key: rune::Key::Text("s".into()), mods: Default::default() });
        rt.frame(0.4);
        rt.frame(1.8);
        while rt.state.toast.is_some() { rt.frame(0.01); }
        rt.frame(0.05);
        let toast = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("toast-saved"));
        if reduced { assert!(toast.is_none() || toast.unwrap().opacity == 0.0); }
        else { assert!(toast.as_ref().is_some_and(|e| e.exiting && e.opacity > 0.0 && e.opacity < 1.0), "toast {toast:?}"); }
        rt.frame(0.1);
        assert!(rt.elements().iter().find(|e| e.key.as_deref() == Some("toast-saved")).is_none_or(|e| e.opacity < 0.08));
        rt.frame(0.15);
        assert!(!rt.elements().iter().any(|e| e.key.as_deref() == Some("toast-saved") && e.opacity > 0.0));
    }
    let mut s = app(true, SavedView::in_memory());
    s.request(DEFAULT_GRID, Ranking::Fresh);
    scene_story(&mut s);
    s.zoom = ZoomStep::Z150;
    let mut rt = rune::Runtime::new(s, view, (800.0, 640.0), rune::Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    rt.frame(0.0);
    rt.key(KeyEvent { key: rune::Key::Text("s".into()), mods: Default::default() });
    rt.frame(0.1);
    let elements = rt.elements();
    let toast = elements.iter().find(|e| e.key.as_deref() == Some("toast-saved")).unwrap();
    let footer = elements.iter().find(|e| e.key.as_deref() == Some("story-footer")).unwrap();
    assert!(toast.rect[1] + toast.rect[3] < footer.rect[1], "toast {:?}, footer {:?}", toast.rect, footer.rect);
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
fn the_saved_view_renders_from_the_store_and_never_asks_the_source() {
    let mut s = app(false, saved_store());
    s.feed.tab = StoryType::Saved;
    s.request((11, 2), Ranking::Fresh);
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
    assert!(!s.feed.request.is_loading(), "the store is local: nothing is in flight");
    let fill = s.feed.fill(11, 2);
    s.measured.set(Some((22, 1)));
    poll(&mut s, 0.0, &mut rune::Tx::default());
    assert_eq!(s.feed.request, RequestLifecycle::Settled(fill, Ranking::Fresh, (22, 1)),
        "an equal-capacity resize still updates Saved's last grid");
    assert!(!s.bridge.is_pending());
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

