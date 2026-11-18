//! Window zoom, inspector sizing and layout persistence.

use super::super::*;
use hn_lib::app::StoryType;
use rune::KeyEvent;
use rune::Tx;
use super::super::window::close;
#[cfg(target_os = "macos")]
use super::super::window::WINDOW_MENUS;
use super::super::input::on_key;
use super::super::poll::poll;
use super::super::scenes::scene_story;
use super::super::window::view;
use super::support::*;

#[test]
fn zoom_shortcuts_step_clamp_and_reset_in_feed_and_story() {
    let mut rt = rune::Runtime::new(app(true, SavedView::in_memory()), view, (1100.0, 760.0), rune::Env::new(true, 2.0));
    rt.on_key = Some(on_key);
    rt.frame(0.0);
    for (key, expected) in [('-', 0.9), ('-', 0.8), ('-', 0.75), ('-', 0.75),
        ('=', 0.8), ('+', 0.9), ('=', 1.0), ('+', 1.1), ('=', 1.25), ('+', 1.5), ('=', 1.5), ('0', 1.0)] {
        rt.key(KeyEvent { key: rune::Key::Command(key), mods: rune::Mods { cmd: true, ..Default::default() } });
        rt.frame(0.0);
        assert_eq!((rt.state.zoom.scale(), rt.zoom()), (expected, expected), "{key}");
    }
    rt.update(|s, _| scene_story(s));
    rt.frame(0.0);
    for (key, expected) in [('+', 1.1), ('-', 1.0), ('-', 0.9), ('0', 1.0)] {
        rt.key(KeyEvent { key: rune::Key::Command(key), mods: rune::Mods { cmd: true, ..Default::default() } });
        rt.frame(0.0);
        assert_eq!((rt.state.zoom.scale(), rt.zoom()), (expected, expected), "story {key}");
    }
}

#[test]
fn inspector_width_returns_exactly_after_a_zoom_round_trip() {
    let mut s = app(true, SavedView::in_memory());
    scene_story(&mut s);
    let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    rt.frame(0.0);
    assert_eq!(rt.state.snapshot().inspector, Some(543.0));
    for key in ['=', '=', '=', '0'] {
        rt.key(KeyEvent { key: rune::Key::Command(key), mods: rune::Mods { cmd: true, ..Default::default() } });
        rt.frame(0.0);
        assert_eq!(rt.state.snapshot().inspector, Some(543.0));
    }
    assert_eq!(rt.state.snapshot().inspector, Some(543.0));
    assert_eq!(rt.state.inspector.get().width(), 543.0);
}

#[test]
fn narrowing_preserves_the_saved_width_and_widening_restores_it() {
    let mut s = app(true, SavedView::in_memory());
    s.layout.inspector = Some(600.0);
    s.restore();
    scene_story(&mut s);
    s.reduced = true;
    let mut rt = rune::Runtime::new(s, view, (1400.0, 1100.0), rune::Env::new(true, 1.0));
    rt.frame(0.0);
    rt.set_viewport((700.0, 1100.0), 1.0);
    for _ in 0..3 { rt.frame(0.0); }
    assert_eq!(rt.state.inspector.get().width(), 350.0);
    assert_eq!((rt.state.inspector.get().min_width(), rt.state.inspector.get().max_width()), (350.0, 350.0));
    assert_eq!(rt.state.inspector_width.get(), 600.0);
    assert_eq!(rt.state.snapshot().inspector, Some(600.0));
    let dir = temp_dir("narrow-inspector");
    let path = dir.join("desktop.json");
    rt.state.snapshot().save(&path).unwrap();
    assert_eq!(Layout::load(&path).inspector, Some(600.0));
    rt.set_viewport((1400.0, 1100.0), 1.0);
    for _ in 0..3 { rt.frame(0.0); }
    assert_eq!(rt.state.inspector.get().width(), 600.0);
    assert_eq!(rt.state.snapshot().inspector, Some(600.0));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn narrow_inspector_range_uses_window_points_at_zoom() {
    let mut s = app(true, SavedView::in_memory());
    scene_story(&mut s);
    s.reduced = true;
    let mut rt = rune::Runtime::new(s, view, (900.0, 1100.0), rune::Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    for _ in 0..3 { rt.frame(0.0); }
    for _ in 0..2 {
        rt.key(KeyEvent { key: rune::Key::Command('='), mods: rune::Mods { cmd: true, ..Default::default() } });
        for _ in 0..3 { rt.frame(0.0); }
    }
    assert_eq!(rt.zoom(), 1.25);
    assert_eq!(rt.state.inspector.get().min_width() * rt.zoom(), 360.0);
    assert_eq!(rt.state.inspector.get().max_width() * rt.zoom(), 450.0);
    assert_eq!(rt.state.inspector.get().width() * rt.zoom(), 450.0);
    assert_eq!(rt.state.snapshot().inspector, Some(543.0));
}

#[test]
fn narrow_grip_drag_survives_a_resize_and_saves_the_dragged_width() {
    let mut s = app(true, SavedView::in_memory());
    scene_story(&mut s);
    s.reduced = true;
    let mut rt = rune::Runtime::new(s, view, (1000.0, 1100.0), rune::Env::new(true, 1.0));
    for _ in 0..3 { rt.frame(0.0); }
    assert_eq!(rt.state.inspector.get().width(), 500.0);
    let grip = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("inspector-grip")).unwrap().rect;
    let (x, y) = (grip[0] + grip[2] / 2.0, grip[1] + 40.0);
    rt.pointer_down(x, y); rt.frame(0.0);
    rt.pointer_move(x + 60.0, y); rt.frame(0.0);
    assert!(rt.state.inspector.get().is_resizing());
    rt.set_viewport((980.0, 1100.0), 1.0);
    for _ in 0..3 { rt.frame(0.0); }
    assert!(rt.state.inspector.get().is_resizing());
    rt.pointer_move(x + 80.0, y); rt.frame(0.0);
    rt.pointer_up(x + 80.0, y); rt.frame(0.0);
    assert_eq!(rt.state.snapshot().inspector, Some(420.0));
    rt.set_viewport((1400.0, 1100.0), 1.0);
    for _ in 0..3 { rt.frame(0.0); }
    assert_eq!(rt.state.inspector.get().width(), 420.0);
}

#[test]
fn resizing_keeps_the_inspector_open_or_collapsed_without_reopening_it() {
    let mut s = app(true, SavedView::in_memory());
    scene_story(&mut s);
    let mut rt = rune::Runtime::new(s, view, (1400.0, 1100.0), rune::Env::new(true, 1.0));
    rt.frame(1.0);
    rt.set_viewport((700.0, 1100.0), 1.0);
    for _ in 0..3 { rt.frame(0.0); }
    let pane = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("inspector-pane")).unwrap();
    assert!(rt.state.inspector.get().is_open());
    assert_eq!(rt.state.inspector.get().width(), 350.0);
    assert_eq!(pane.rect[2], 350.0, "resize must not replay the opening slide");
    rt.update(|s, _| s.inspector.get_mut().set_open(false));
    rt.frame(1.0);
    rt.set_viewport((900.0, 1100.0), 1.0);
    for _ in 0..3 { rt.frame(0.0); }
    assert!(!rt.state.inspector.get().is_open());
    assert_eq!(rt.state.inspector.get().shown_width(), 0.0);
    assert_eq!(rt.state.inspector.get().max_width(), 450.0);
}

#[test]
fn resizing_during_open_follows_runes_existing_motion_and_keeps_later_transitions() {
    let width = |rt: &rune::Runtime<App>| rt.elements().into_iter()
        .find(|e| e.key.as_deref() == Some("inspector-pane")).unwrap().rect[2];
    for (window, target) in [(1300.0, 543.0), (1000.0, 500.0)] {
        let s = app(true, SavedView::in_memory());
        let mut rt = rune::Runtime::new(s, view, (1400.0, 1100.0), rune::Env::new(true, 1.0));
        for _ in 0..3 { rt.frame(0.0); }
        rt.update(|s, tx| { scene_story(s); tx.animated(); });
        rt.frame(0.03);
        let before = width(&rt);
        assert!(before > 0.0 && before < target, "opening width {before}");
        rt.set_viewport((window, 1100.0), 1.0);
        for _ in 0..3 { rt.frame(0.0); }
        let after = width(&rt);
        assert_eq!(after, target, "window {window}: opening width {before} -> {after}");
    }
    let s = app(true, SavedView::in_memory());
    let mut rt = rune::Runtime::new(s, view, (700.0, 1100.0), rune::Env::new(true, 1.0));
    for _ in 0..3 { rt.frame(0.0); }
    assert!(!rt.state.inspector.get().is_open());
    assert_eq!(rt.state.inspector.get().width(), 543.0);
    rt.update(|s, tx| { scene_story(s); tx.animated(); });
    rt.frame(0.03);
    assert!(width(&rt) > 0.0 && width(&rt) < 350.0);
    rt.frame(0.5);
    assert_eq!(width(&rt), 350.0);
    rt.update(|s, tx| { s.inspector.get_mut().set_open(false); tx.animated(); });
    rt.frame(0.03);
    assert!(width(&rt) > 0.0 && width(&rt) < 350.0);
    rt.frame(0.5);
    assert_eq!(width(&rt), 0.0);
    rt.update(|s, _| s.close_story());
    rt.frame(0.0);
    assert_eq!((rt.state.inspector.get().width(), rt.state.inspector.get().max_width()), (543.0, 640.0));
}

#[test]
fn inspector_window_width_survives_zoom_drag_and_relaunch() {
    let mut s = app(true, SavedView::in_memory());
    scene_story(&mut s);
    s.reduced = true;
    let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    rt.frame(0.0);
    let widths = |rt: &rune::Runtime<App>| {
        let e = rt.elements();
        let width = |key| e.iter().find(|v| v.key.as_deref() == Some(key)).unwrap().rect[2];
        (width("story-main"), width("inspector-pane"))
    };
    let zoom = |rt: &mut rune::Runtime<App>, key| {
        rt.key(KeyEvent { key: rune::Key::Command(key), mods: rune::Mods { cmd: true, ..Default::default() } });
        rt.frame(0.0);
    };
    let drag = |rt: &mut rune::Runtime<App>, delta: f32| {
        let grip = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("inspector-grip")).unwrap().rect;
        let (x, y) = (grip[0] + grip[2] / 2.0, grip[1] + 40.0);
        let (start, z) = (grip[0] * rt.zoom(), rt.zoom());
        rt.pointer_down(x, y); rt.frame(0.0);
        rt.pointer_move(x + delta / z, y); rt.frame(0.0);
        rt.pointer_up(x + delta / z, y); rt.frame(0.0);
        let end = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("inspector-grip")).unwrap().rect[0] * z;
        assert!((end - start - delta).abs() <= z + 0.1, "zoom {z}, drag {delta}: {start} -> {end}");
    };
    let (article, inspector) = widths(&rt);
    let initial = rt.state.snapshot().inspector.unwrap();
    for _ in 0..3 { zoom(&mut rt, '='); }
    assert_eq!(rt.zoom(), 1.5);
    let (zoomed_article, zoomed_inspector) = widths(&rt);
    assert!((article / inspector - zoomed_article / zoomed_inspector).abs() < 0.03);
    assert_eq!(rt.state.snapshot().inspector, Some(initial));
    drag(&mut rt, 60.0);
    let dragged = rt.state.snapshot().inspector.unwrap();
    assert!((initial - dragged - 60.0).abs() < 2.0);
    for key in ['0', '=', '=', '='] { zoom(&mut rt, key); }
    assert_eq!(rt.state.snapshot().inspector, Some(dragged));
    let dir = temp_dir("inspector-zoom");
    let path = dir.join("desktop.json");
    let snapshot = rt.state.snapshot();
    snapshot.save(&path).unwrap();
    let mut next = app(true, SavedView::in_memory());
    next.layout = Layout::load(&path);
    next.zoom = next.layout.zoom_level();
    next.restore();
    assert_eq!(next.zoom, ZoomStep::Z150);
    assert_eq!(next.snapshot().inspector, snapshot.inspector);
    let _ = std::fs::remove_dir_all(dir);
    for (i, key) in ['-', '-', '-', '-', '-', '-', '+', '+', '+', '+', '+', '+'].into_iter().enumerate() {
        zoom(&mut rt, key);
        assert!((rt.state.inspector.get().min_width() * rt.zoom() - INSPECTOR_MIN).abs() < 0.01);
        assert!((rt.state.inspector.get().max_width() * rt.zoom() - 590.0).abs() < 0.01);
        drag(&mut rt, if i % 2 == 0 { 15.0 } else { -15.0 });
    }
    for _ in 0..2 { zoom(&mut rt, '-'); }
    assert_eq!(rt.zoom(), 1.1);
    let reset_width = rt.state.inspector.get().default_width() * rt.zoom();
    assert_ne!(rt.state.snapshot().inspector, Some(reset_width));
    rt.frame(0.6);
    let grip = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("inspector-grip")).unwrap().rect;
    let (x, y) = (grip[0] + grip[2] / 2.0, grip[1] + 40.0);
    rt.pointer_move(x, y);
    for _ in 0..2 {
        rt.pointer_down(x, y); rt.frame(0.01);
        rt.pointer_up(x, y); rt.frame(0.01);
    }
    assert_eq!(rt.state.snapshot().inspector, Some(reset_width));
    for key in ['0', '='] { zoom(&mut rt, key); }
    assert_eq!(rt.state.snapshot().inspector, Some(reset_width));
}

#[cfg(target_os = "macos")]
#[test]
fn command_equal_menu_action_steps_once_headlessly() {
    for (_, _, chord, _) in WINDOW_MENUS { chord.parse::<rune::Shortcut>().unwrap(); }
    let (_, menu, chord, action) = WINDOW_MENUS.iter().find(|(title, ..)| *title == "Zoom In").unwrap();
    assert_eq!((*menu, *chord), (Some("View"), "cmd+="));
    let shortcut: rune::Shortcut = chord.parse().unwrap();
    assert_eq!(shortcut.physical_key(), rune::PhysicalKeyCode::Equal);
    assert!(shortcut.modifiers().cmd && !shortcut.modifiers().shift);
    let mut rt = rune::Runtime::new(app(true, SavedView::in_memory()), view, (1100.0, 760.0), rune::Env::new(true, 2.0));
    rt.update(|s, tx| action(s, tx));
    rt.frame(0.0);
    assert_eq!((rt.state.zoom, rt.zoom()), (ZoomStep::Z110, 1.1));
}

#[cfg(target_os = "macos")]
#[test]
fn menu_actions_operate_on_the_same_window_state() {
    let mut s = app(true, SavedView::in_memory());
    let run = |title: &str, s: &mut App| {
        let (_, _, _, action) = WINDOW_MENUS.iter().find(|(name, ..)| *name == title).unwrap();
        action(s, &mut Tx::default());
    };
    run("Settings…", &mut s);
    assert!(s.settings.is_some());
    run("Keyboard Shortcuts", &mut s);
    assert!(s.help);
    scene_story(&mut s);
    let open = s.inspector.get().is_open();
    run("Show/Hide Inspector", &mut s);
    assert_ne!(s.inspector.get().is_open(), open);
    run("Zoom Out", &mut s);
    assert_eq!(s.zoom, ZoomStep::Z90);
    run("Actual Size", &mut s);
    assert_eq!((s.zoom, s.toast.as_ref().unwrap().message.as_str()), (ZoomStep::Z100, "100%"));
}

#[test]
fn zoom_toast_shows_percentage_and_expires() {
    let mut rt = rune::Runtime::new(app(true, SavedView::in_memory()), view, (1100.0, 760.0), rune::Env::new(true, 2.0));
    rt.on_key = Some(on_key);
    rt.key(KeyEvent { key: rune::Key::Command('='), mods: rune::Mods { cmd: true, ..Default::default() } });
    rt.frame(0.0);
    assert_eq!(rt.state.zoom, ZoomStep::Z110);
    let notice = rt.state.toast.as_ref().unwrap();
    assert_eq!(notice.message, "110%");
    assert!(!notice.show_dot);
    poll(&mut rt.state, 0.0, &mut Tx::default());
    poll(&mut rt.state, TOAST_SECONDS + 0.1, &mut Tx::default());
    assert!(rt.state.toast.is_none());
    rt.key(KeyEvent { key: rune::Key::Command('0'), mods: rune::Mods { cmd: true, ..Default::default() } });
    rt.frame(0.0);
    assert_eq!(rt.state.toast.as_ref().unwrap().message, "100%");
}

#[test]
fn zoom_changes_feed_columns_and_tracks_window_points() {
    let mut s = app(true, SavedView::in_memory());
    s.request(DEFAULT_GRID, Ranking::Fresh);
    let mut rt = rune::Runtime::new(s, view, (1100.0, 760.0), rune::Env::new(true, 2.0));
    rt.on_key = Some(on_key);
    rt.frame(0.0);
    assert_eq!(rt.state.measured.get().unwrap().columns.count().get(), 1);
    rt.key(KeyEvent { key: rune::Key::Command('-'), mods: rune::Mods { cmd: true, ..Default::default() } });
    rt.frame(0.0);
    assert_eq!(rt.zoom(), 0.9);
    assert_eq!(rt.state.measured.get().unwrap().columns.count().get(), 2);
    let size = rt.state.window_size.get().unwrap();
    assert!((size.0 - 1100.0).abs() < 0.01 && (size.1 - 760.0).abs() < 0.01, "{size:?}");

    let dir = temp_dir("zoom-window");
    let path = dir.join("desktop.json");
    rt.state.layout_path = Some(path.clone());
    rt.state.headless = false;
    rt.state.layout.window = Some((900.0, 600.0));
    close(&mut rt.state, &mut Tx::default());
    let saved = Layout::load(&path);
    assert_eq!(saved.zoom_level(), ZoomStep::Z90);
    let expected = if cfg!(target_os = "macos") { None } else { Some(size) };
    assert_eq!(saved.window, expected);
    rt.state.headless = true;
    rt.key(KeyEvent { key: rune::Key::Command('0'), mods: rune::Mods { cmd: true, ..Default::default() } });
    rt.frame(0.0);
    assert_eq!(rt.state.measured.get().unwrap().columns.count().get(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

fn reflow_frames(rt: &mut rune::Runtime<App>, selected_id: i32) -> Vec<(usize, usize, bool)> {
    (0..4).map(|_| {
        rt.frame(0.016);
        let elements = rt.elements();
        let lines = elements.iter().filter(|e| !e.exiting && e.key.as_deref()
            .and_then(|key| key.strip_prefix("line-"))
            .is_some_and(|n| n.parse::<usize>().is_ok())).count();
        let selected = format!("story-{selected_id}");
        let visible = elements.iter().any(|e| e.key.as_deref() == Some(&selected)
            && !e.exiting && e.visible[3] >= ROW_HEIGHT - 1.0);
        let snapped = elements.iter().filter(|e| !e.exiting && e.key.as_deref()
            .is_some_and(|key| key.starts_with("story-")))
            .all(|e| e.rect.iter().zip(e.target).all(|(a, b)| (*a - b).abs() < 0.1));
        (rt.state.measured.get().unwrap().rows.get(), lines, visible && snapped)
    }).collect()
}

#[test]
fn resize_keeps_rank_sixteen_visible_without_an_overfull_frame() {
    let mut s = app(true, SavedView::in_memory());
    s.request(grid(14, 2), Ranking::Fresh);
    let mut rt = rune::Runtime::new(s, view, (1600.0, 900.0), rune::Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    rt.poll = Some(poll);
    for _ in 0..3 { rt.frame(0.016); }
    assert_eq!(rt.state.measured.get(), Some(grid(14, 2)));
    route(&mut rt, rune::Key::Down);
    route(&mut rt, rune::Key::Right);
    assert_eq!(rt.state.feed.selected_id(), Some(16));

    rt.set_viewport((1100.0, 900.0), 1.0);
    let frames = reflow_frames(&mut rt, 16);
    assert_eq!(rt.state.feed.selected_id(), Some(16));
    assert_eq!(rt.state.feed.display_cursor, 13);
    assert!(frames.iter().all(|(rows, lines, visible)| lines <= rows && *visible), "{frames:?}");
}

#[test]
fn zoom_breakpoint_keeps_the_selected_story_visible_in_every_frame() {
    let mut s = app(true, SavedView::in_memory());
    s.request(DEFAULT_GRID, Ranking::Fresh);
    let mut rt = rune::Runtime::new(s, view, (1100.0, 760.0), rune::Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    rt.poll = Some(poll);
    for _ in 0..3 { rt.frame(0.016); }
    let narrow_rows = rt.state.measured.get().unwrap().rows.get();
    rt.key(KeyEvent { key: rune::Key::Command('-'), mods: rune::Mods { cmd: true, ..Default::default() } });
    let widening = reflow_frames(&mut rt, 1);
    assert_eq!((rt.state.zoom, rt.state.measured.get().unwrap().columns.count().get()), (ZoomStep::Z90, 2));
    route(&mut rt, rune::Key::Down);
    route(&mut rt, rune::Key::Right);
    let selected = rt.state.feed.selected_id().unwrap();
    assert!(rt.state.feed.selected >= narrow_rows);

    rt.key(KeyEvent { key: rune::Key::Command('0'), mods: rune::Mods { cmd: true, ..Default::default() } });
    let narrowing = reflow_frames(&mut rt, selected);
    assert_eq!((rt.state.zoom, rt.state.measured.get().unwrap().columns.count().get()), (ZoomStep::Z100, 1));
    assert_eq!(rt.state.feed.selected_id(), Some(selected));
    assert_eq!(rt.state.feed.display_cursor, narrow_rows);
    assert!(widening.iter().chain(&narrowing).all(|(rows, lines, visible)| lines <= rows && *visible),
        "widening {widening:?}, narrowing {narrowing:?}");
}

#[test]
fn saved_selection_remains_visible_after_resize() {
    let store = (1..=28).enumerate().map(|(index, id)| crate::fixtures::item(index, id)).collect();
    let mut s = app(true, SavedView::seeded(store));
    s.feed.tab = StoryType::Saved;
    s.request(grid(14, 2), Ranking::Fresh);
    let mut rt = rune::Runtime::new(s, view, (1600.0, 900.0), rune::Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    rt.poll = Some(poll);
    for _ in 0..3 { rt.frame(0.016); }
    route(&mut rt, rune::Key::Down);
    route(&mut rt, rune::Key::Right);
    assert_eq!(rt.state.feed.selected_id(), Some(16));
    rt.set_viewport((1100.0, 900.0), 1.0);
    for frame in 0..4 {
        rt.frame(0.016); assert_eq!(rt.state.feed.selected_id(), Some(16));
        assert!(rt.elements().iter().any(|e| e.key.as_deref() == Some("story-16")
            && e.visible[3] >= ROW_HEIGHT - 1.0), "frame {frame}");
    }
    assert_eq!(rt.state.feed.selected_id(), Some(16));
    assert_eq!(rt.state.feed.stories.len(), 28, "Saved remains one scrollable list");
    rt.set_viewport((1600.0, 900.0), 1.0);
    for _ in 0..3 { rt.frame(0.016); }
    route(&mut rt, rune::Key::Text("G".into()));
    assert_eq!(rt.state.feed.selected_id(), Some(28));
    rt.set_viewport((1100.0, 700.0), 1.0);
    for frame in 0..4 {
        rt.frame(0.016); assert_eq!(rt.state.feed.selected_id(), Some(28));
        assert!(frame == 0 || rt.elements().iter().any(|e| e.key.as_deref() == Some("story-28")
            && e.visible[3] >= ROW_HEIGHT - 1.0), "shorter window frame {frame}");
    }
}

#[test]
#[cfg(target_os = "macos")]
fn live_region_bounds_follow_zoom_in_window_points() {
    let mut s = app(true, SavedView::in_memory());
    scene_story(&mut s);
    s.headless = false;
    let mut rt = rune::Runtime::new(s, view, (1100.0, 760.0), rune::Env::new(true, 2.0));
    rt.on_key = Some(on_key);
    for (key, zoom) in [('-', 0.9), ('0', 1.0), ('+', 1.1)] {
        rt.key(KeyEvent { key: rune::Key::Command(key), mods: rune::Mods { cmd: true, ..Default::default() } });
        rt.frame(0.0);
        rt.frame(0.0);
        let rect = rt.state.live_bounds.expect("live region reports layout");
        let key = format!("article-live-{}", crate::fixtures::STORY_ID);
        let drawn = rt.elements().into_iter().find(|e| e.key.as_deref() == Some(&key)).unwrap().rect;
        for (actual, expected) in rect.into_iter().zip(rt.to_window(drawn)) {
            assert!((actual - expected).abs() < 1.0, "zoom {zoom}: {rect:?}, {drawn:?}");
        }
        assert_eq!(rt.zoom(), zoom);
    }
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

#[test]
fn restore_puts_the_reader_back_where_they_left_off() {
    let mut s = app(true, SavedView::in_memory());
    s.layout = Layout {
        window: Some((1280.0, 820.0)),
        zoom: Some(1.0),
        inspector: Some(512.0),
        tab: Some(StoryType::Best),
        page: Some(3),
        selected: Some(42),
    };

    s.restore();

    assert_eq!(s.feed.tab, StoryType::Best);
    assert_eq!(
        s.feed.display_cursor,
        2 * hn_lib::feed_state::API_PAGE_SIZE as usize,
        "page 3 is the third window of the page size"
    );
    assert_eq!(s.feed.pending_anchor, Some(42), "the feed re-anchors on it");
    assert_eq!(s.inspector.get().width(), 512.0, "the grip's width came back");
}

#[test]
fn the_layout_file_round_trips_the_window_and_the_feed() {
    let dir = temp_dir("layout");
    let path = dir.join("hn/desktop.json");
    let mut s = app(true, SavedView::in_memory());
    s.request(grid(11, 1), Ranking::Fresh);
    s.layout_path = Some(path.clone());
    s.window_size.set(Some((1280.0, 820.0)));
    s.layout.window = Some((1100.0, 760.0));
    s.layout.inspector = Some(512.0);
    s.restore();
    s.feed.tab = StoryType::Best;
    s.feed.selected = 4;

    let snapshot = s.snapshot();
    let expected = if cfg!(target_os = "macos") { None } else { Some((1280.0, 820.0)) };
    assert_eq!(snapshot.window, expected);
    assert_eq!(snapshot.selected, Some(5), "the selected story's id");
    s.write_layout(snapshot.clone(), 0.0);
    assert_eq!(Layout::load(&path), snapshot, "the file holds what was drawn");

    // A second window starts from the file, not from the defaults.
    let mut next = app(true, SavedView::in_memory());
    next.layout = Layout::load(&path);
    next.restore();
    assert_eq!(next.feed.tab, StoryType::Best);
    assert_eq!(next.feed.pending_anchor, Some(5));
    assert_eq!(next.inspector.get().width(), 512.0);
    let _ = std::fs::remove_dir_all(&dir);
}
