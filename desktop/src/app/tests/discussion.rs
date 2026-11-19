//! Discussion navigation, replies and scrolling.

use super::super::*;
use std::time::Duration;
use rune::tok;
use rune::AppSpec;
use rune::KeyEvent;
use rune::Tx;
use crate::story::SummaryState;
use super::super::input::on_key;
use super::super::poll::{poll, poll_updates};
use super::super::scenes::scene_comments;
use super::super::scenes::scene_story;
use super::super::window::view;
use super::support::*;

#[test]
fn kit_button_focus_ring_is_amber_in_both_themes() {
    for dark in [false, true] {
        let mut s = app(true, SavedView::in_memory());
        scene_story(&mut s);
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(dark, 1.0));
        rt.frame(0.0);
        rt.update(|_, tx| tx.focus("discussion-focus", true));
        rt.frame(0.0);
        let tree = rt.elements();
        let button = tree.iter().find(|e| e.key.as_deref() == Some("discussion-focus")).unwrap();
        assert!(button.focused && button.focus_visible);
        let [x, y, w, h] = button.rect;
        let amber = tok::accent(3, dark).premul(1.0);
        assert!(rt.instances.iter().any(|i| i.border == amber &&
            i.rect[0] <= x && i.rect[1] <= y && i.rect[2] >= w && i.rect[3] >= h),
            "focused kit button must paint an amber ring");
    }
}

#[test]
fn citation_return_keeps_keyboard_focus_and_matches_native_back_motion() {
    for reduced in [false, true] {
        let mut rune_frames = Vec::new();
        for native in [false, true] {
            let mut s = app(true, SavedView::in_memory());
            s.reduced = reduced;
            scene_story(&mut s);
            let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
            rt.on_key = Some(on_key);
            rt.frame(0.0);
            route(&mut rt, rune::Key::Tab);
            route(&mut rt, rune::Key::Enter);
            rt.frame(0.3);
            assert!(rt.elements().iter().any(|e| e.key.as_deref() == Some("source-viewer")));

            if native {
                rt.poll = Some(|s, now, tx| poll_updates(s, now, tx, true));
            } else {
                rt.key(KeyEvent { key: rune::Key::Escape, mods: Default::default() });
            }
            rt.frame(0.0);
            rt.poll = None;
            let mut frames = Vec::new();
            for (ms, dt) in [(30, 0.03), (100, 0.07), (300, 0.20)] {
                rt.frame(dt);
                let elements = rt.elements();
                let find = |key: &str| elements.iter().find(|e| e.key.as_deref() == Some(key));
                assert!(find("source-viewer").is_none(), "source exits immediately");
                let discussion = find("inspector").unwrap();
                assert!(!discussion.inert && !discussion.exiting && find("comment-901").is_some());
                frames.push((discussion.opacity, discussion.rect[0]));
                println!("native={native} reduced={reduced} +{ms}ms: source absent, discussion opacity={:.4} x={:.2}",
                    discussion.opacity, discussion.rect[0]);
            }
            if native {
                assert_eq!(frames, rune_frames);
            } else {
                rune_frames = frames;
                if reduced {
                    assert!(rune_frames.iter().all(|frame| *frame == rune_frames[2] && frame.0 == 1.0));
                } else {
                    assert!(rune_frames[0].0 < rune_frames[1].0 && rune_frames[1].0 < rune_frames[2].0);
                    assert!(rune_frames[0].1 < rune_frames[1].1 && rune_frames[1].1 < rune_frames[2].1);
                }
            }

            rt.frame(0.3);
            let elements = rt.elements();
            let find = |key: &str| elements.iter().find(|e| e.key.as_deref() == Some(key)).unwrap();
            let summary = find("summary-scroll");
            assert!(summary.focused && summary.focusable);
            assert!(find("cite-0").label.as_deref().unwrap().starts_with("Selected."));
            find("citation-focus");
            let [x, y, w, h] = summary.rect;
            let amber = tok::accent(3, true).premul(1.0);
            assert!(!rt.instances.iter().any(|i| i.border == amber &&
                i.rect[0] <= x && i.rect[1] <= y && i.rect[2] >= w && i.rect[3] >= h),
                "the selected citation supplies the focus cue");

            route(&mut rt, rune::Key::Tab);
            assert_eq!(rt.state.story().unwrap().focused_citation().unwrap().position, 1);
            let cursor = |rt: &rune::Runtime<App>| rt.state.story().unwrap().thread.cursor();
            let before = cursor(&rt);
            route(&mut rt, rune::Key::Text("j".into()));
            assert_ne!(cursor(&rt), before);
            route(&mut rt, rune::Key::Text("k".into()));
            assert_eq!(cursor(&rt), before);
        }
    }
}

#[test]
fn quiet_control_and_disclosure_respond_to_hover() {
    let mut s = app(true, SavedView::in_memory());
    scene_story(&mut s);
    let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
    rt.frame(0.0);
    for key in ["discussion-focus", "comment-disclosure-901"] {
        rt.pointer_move(0.0, 0.0);
        rt.frame(0.3);
        let before = rt.elements().iter().find(|e| e.key.as_deref() == Some(key)).unwrap().clone();
        let paint = |rt: &rune::Runtime<App>| rt.instances.iter()
            .filter(|i| i.rect.iter().zip(before.rect).all(|(a, b)| (*a - b).abs() <= 1.0) && i.fill[3] > 0.0)
            .map(|i| i.fill).collect::<Vec<_>>();
        let before_paint = paint(&rt);
        let [x, y, w, h] = before.visible;
        rt.pointer_move(x + w / 2.0, y + h / 2.0);
        rt.frame(0.0);
        rt.frame(0.3);
        let after = rt.elements().iter().find(|e| e.key.as_deref() == Some(key)).unwrap().hover;
        assert!(after > before.hover, "{key} must receive hover");
        assert_eq!(rt.cursor(), rune::Cursor::Pointer, "{key} must show a pointer cursor");
        assert_ne!(paint(&rt), before_paint, "{key} must visibly respond to hover");
    }
}

#[test]
fn passive_hints_follow_pointer_and_keyboard_without_dimming_actions() {
    for (reduced, contrast) in [(false, false), (true, false), (false, true)] {
        let mut s = app(true, SavedView::in_memory());
        s.request(grid(11, 1), Ranking::Fresh);
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.set_system_reduced_motion(reduced);
        rt.set_system_increased_contrast(contrast);
        rt.on_key = Some(on_key);
        rt.frame(0.0);
        let hint = |rt: &rune::Runtime<App>| rt.elements().into_iter()
            .find(|e| e.text.as_deref() == Some("move") && e.path.contains("footer")).unwrap().opacity;
        rt.pointer_move(100.0, 100.0);
        rt.frame(0.05);
        if contrast { assert_eq!(hint(&rt), 1.0); }
        else if reduced { assert!((hint(&rt) - 0.3).abs() < 0.01); }
        else { assert!(hint(&rt) < 1.0 && hint(&rt) > 0.3); }
        rt.key(KeyEvent { key: rune::Key::Down, mods: Default::default() });
        rt.frame(0.0);
        assert_eq!(hint(&rt), 1.0, "keyboard navigation immediately restores hints");
        assert_eq!(rt.state.feed.selected, 1);
        rt.pointer_down(10.0, 10.0);
        rt.pointer_up(10.0, 10.0);
        rt.frame(0.5);
        if !contrast { assert!((hint(&rt) - 0.3).abs() < 0.01, "a stationary click counts as pointer use"); }

        rt.update(|s, _| {
            s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
            s.story.as_mut().unwrap().article_summary = crate::story::ArticleSummaryState::Done("Scrollable summary. ".repeat(150));
        });
        rt.frame(1.0);
        rt.key(KeyEvent { key: rune::Key::Text("j".into()), mods: Default::default() });
        rt.frame(0.0);
        assert_eq!(hint(&rt), 1.0, "discussion navigation restores hints");
        let elements = rt.elements();
        let [x, y, w, h] = elements.iter().find(|e| e.key.as_deref() == Some("summary-scroll")).unwrap().rect;
        let before = elements.iter().find(|e| e.key.as_deref() == Some("article-summary")).unwrap().rect[1];
        rt.wheel_xy(x + w / 2.0, y + h / 2.0, 0.0, -80.0);
        rt.frame(0.5);
        let after = rt.elements();
        assert!(after.iter().find(|e| e.key.as_deref() == Some("article-summary")).unwrap().rect[1] < before);
        if !contrast { assert!((hint(&rt) - 0.3).abs() < 0.01, "wheel use dims the passive hints"); }
        for key in ["story-back", "article-open-original"] {
            assert_eq!(after.iter().find(|e| e.key.as_deref() == Some(key)).unwrap().opacity, 1.0);
        }
        rt.update(|_, tx| tx.focus("summary-scroll", true));
        rt.frame(0.0);
        rt.key(KeyEvent { key: rune::Key::Tab, mods: Default::default() });
        rt.frame(0.0);
        assert_eq!(hint(&rt), 1.0, "citation Tab restores hints");
        rt.wheel_xy(x + w / 2.0, y + h / 2.0, 0.0, 80.0);
        rt.frame(0.5);
        if !contrast { assert!((hint(&rt) - 0.3).abs() < 0.01); }
        rt.update(|_, tx| tx.focus("summary-scroll", true));
        rt.frame(0.0);
        rt.key(KeyEvent { key: rune::Key::PageUp, mods: Default::default() });
        rt.frame(0.0);
        assert_eq!(hint(&rt), 1.0, "a viewport-consumed Page Up restores hints");
    }
}

#[test]
fn discussion_c_collapses_the_nearest_open_ancestor() {
    let mut s = app(true, SavedView::in_memory());
    s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
    s.load_replies(901, false);
    s.story.as_mut().unwrap().thread.select(9011);
    press(&mut s, rune::Key::Text("c".into()));
    let thread = &s.story.as_ref().unwrap().thread;
    assert_eq!(thread.cursor(), Some(901));
    assert!(thread.is_folded(901));
    assert!(thread.rows().iter().any(|row| row.id == 901));
    assert!(!thread.rows().iter().any(|row| row.id == 9011));
}

#[test]
fn pointer_movement_selects_in_focus_mode_without_scroll_or_keyboard_churn() {
    let mut s = app(true, SavedView::in_memory());
    s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
    s.load_replies(901, false);
    let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    rt.frame(0.0);
    rt.update(|_, tx| tx.scroll_into_view_in("discussion-scroll", "comment-9010"));
    rt.frame(0.5);
    let [x, y, w, h] = rt.elements().iter().find(|e| e.key.as_deref() == Some("comment-9010")).unwrap().visible;
    assert!(h > 2.0 && w > 2.0);
    rt.pointer_move(x + w / 2.0, y + h / 2.0);
    rt.frame(0.5);
    assert_eq!(rt.state.story.as_ref().unwrap().thread.cursor(), Some(9010));
    route(&mut rt, rune::Key::Down);
    assert_eq!(rt.state.story.as_ref().unwrap().thread.cursor(), Some(9011));
    rt.update(|_, tx| tx.scroll_to_instant("discussion-scroll", 0.0, 0.0));
    rt.frame(0.5);
    assert_eq!(rt.state.story.as_ref().unwrap().thread.cursor(), Some(9011), "scrolling under a still pointer does not select");
    route(&mut rt, rune::Key::Text("f".into()));
    let [x, y, w, h] = rt.elements().iter().find(|e| e.key.as_deref() == Some("comment-901")).unwrap().visible;
    rt.pointer_move(x + w / 2.0, y + h / 2.0);
    rt.frame(0.5);
    assert_eq!(rt.state.story.as_ref().unwrap().thread.cursor(), Some(9011), "hover selection follows focus mode");
}

#[test]
fn hovering_across_a_root_reply_boundary_keeps_rows_under_the_pointer() {
    let mut s = app(true, SavedView::in_memory());
    scene_comments(&mut s);
    let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
    rt.frame(0.0);
    let row = |rt: &rune::Runtime<App>, key: &str| rt.elements().into_iter()
        .find(|e| e.key.as_deref() == Some(key) && !e.exiting).unwrap();
    let before_root = row(&rt, "comment-901");
    let before_reply = row(&rt, "comment-9010");
    let x = before_reply.rect[0] + before_reply.rect[2] / 2.0;
    let boundary = before_reply.rect[1];
    rt.pointer_move(x, before_root.rect[1] + before_root.rect[3] - 1.0);
    rt.frame(0.05);
    for dy in [1.0, 2.0, 1.0, 2.0] {
        rt.pointer_move(x, boundary + dy);
        rt.frame(0.05);
        assert_eq!(rt.state.story.as_ref().unwrap().thread.cursor(), Some(9010), "1 pt movement must stay on the reply");
        let root = row(&rt, "comment-901");
        let reply = row(&rt, "comment-9010");
        assert_eq!(root.rect, before_root.rect, "hover must not move the root row");
        assert_eq!(reply.rect, before_reply.rect, "hover must not move the reply row");
        assert_eq!(reply.scrolled, before_reply.scrolled, "hover must not scroll the discussion");
    }
}

#[test]
fn hovering_between_equal_depths_with_different_ancestors_keeps_geometry() {
    let mut s = app(true, SavedView::in_memory());
    scene_comments(&mut s);
    let comments: Vec<_> = crate::fixtures::thread().into_iter()
        .filter(|c| [901, 9010, 903, 9030].contains(&c.id)).collect();
    s.story.as_mut().unwrap().thread = hn_lib::comments::Thread::from_comments(&comments, Some(crate::story::VISIBLE_COMMENT_LEVELS), crate::story::REPLY_BATCH_SIZE);
    let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
    rt.frame(0.0);
    let row = |rt: &rune::Runtime<App>, key: &str| rt.elements().into_iter()
        .find(|e| e.key.as_deref() == Some(key) && !e.exiting).unwrap();
    let first = row(&rt, "comment-9010").visible;
    rt.pointer_move(first[0] + first[2] / 2.0, first[1] + first[3] / 2.0);
    rt.frame(0.5);
    let before = row(&rt, "comment-9030");
    let [x, y, w, h] = before.visible;
    assert!(w > 0.0 && h > 0.0);
    rt.pointer_move(x + w / 2.0, y + h / 2.0);
    rt.frame(0.05);
    assert_eq!(rt.state.story.as_ref().unwrap().thread.cursor(), Some(9030));
    assert_eq!(row(&rt, "comment-9030").rect, before.rect);
    assert_eq!(row(&rt, "comment-9030").scrolled, before.scrolled);
}

#[test]
fn disclosure_edge_click_folds_and_unfolds_without_slow_row_reflow() {
    for (source, reduced) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut s = app(true, SavedView::in_memory());
        scene_comments(&mut s);
        if source { s.open_citation(0); }
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.set_system_reduced_motion(reduced);
        rt.frame(0.0);
        for _ in 0..30 { rt.frame(1.0 / 60.0); }
        let elements = rt.elements();
        let button = elements.iter().find(|e| e.key.as_deref() == Some("comment-disclosure-901") && !e.inert).unwrap();
        assert_eq!((button.rect[2], button.rect[3]), (28.0, 28.0));
        let [x, y, _, _] = button.rect;
        let measure = |rt: &rune::Runtime<App>| rt.elements().iter()
            .find(|e| e.key.as_deref() == Some(if source { "source-replies" } else { "discussion-rows" })).unwrap().rect[3];
        let footer = elements.iter().find(|e| e.key.as_deref() == Some("story-footer")).unwrap().rect;
        let original = measure(&rt);
        let click = |rt: &mut rune::Runtime<App>| {
            rt.pointer_down(x + 1.0, y + 1.0);
            rt.pointer_up(x + 1.0, y + 1.0);
            rt.frame(0.0);
        };
        click(&mut rt);
        let story = rt.state.story.as_ref().unwrap();
        assert!(if source { story.source().unwrap().nav.folded.contains(&901) } else { story.thread.is_folded(901) });
        let reply = if source { "source-comment-9010" } else { "comment-9010" };
        assert!(!rt.elements().iter().any(|e| e.key.as_deref() == Some(reply)), "outgoing replies must not paint over moving live rows");
        let mut closing = vec![measure(&rt)];
        for dt in [0.05, 0.05, 0.10, 0.10] {
            rt.frame(dt);
            closing.push(measure(&rt));
            assert_eq!(rt.elements().iter().find(|e| e.key.as_deref() == Some("story-footer")).unwrap().rect, footer);
        }
        if reduced { assert!(closing.iter().all(|y| (*y - closing[0]).abs() < 0.5)); }
        else {
            assert!(closing[0] > closing[1] && closing[1] > closing[2] && closing[2] > closing[3], "{source}: {closing:?}");
            assert!((closing[3] - closing[4]).abs() < 3.0, "fold settles by 300 ms: {closing:?}");
        }
        click(&mut rt);
        let mut opening = vec![measure(&rt)];
        for dt in [0.05, 0.05, 0.10, 0.10] {
            rt.frame(dt);
            opening.push(measure(&rt));
        }
        if reduced { assert!(opening.iter().all(|y| (*y - opening[0]).abs() < 0.5)); }
        else {
            assert!(opening[0] < opening[1] && opening[1] < opening[2] && opening[2] < opening[3], "{source}: {opening:?}");
            assert!((opening[3] - opening[4]).abs() < 3.0, "unfold settles by 300 ms: {opening:?}");
        }
        assert!((opening[4] - original).abs() < 0.5, "unfold restores the same rows");
        if !reduced {
            click(&mut rt);
            rt.frame(0.05);
            let mid = measure(&rt);
            click(&mut rt);
            let reversed = measure(&rt);
            rt.frame(0.05);
            assert!((reversed - mid).abs() <= 10.0, "reversal starts near the visible position");
            assert!(measure(&rt) > reversed, "reversal opens immediately");
            rt.frame(0.25);
            assert!((measure(&rt) - original).abs() < 0.5);
        }
    }
}

#[test]
fn keyboard_fold_and_unfold_move_the_loaded_rows() {
    let mut s = app(true, SavedView::in_memory());
    scene_comments(&mut s);
    let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    rt.frame(0.0);
    let height = |rt: &rune::Runtime<App>| rt.elements().iter()
        .find(|e| e.key.as_deref() == Some("discussion-rows")).unwrap().rect[3];
    rt.key(KeyEvent { key: rune::Key::Text("h".into()), mods: Default::default() });
    rt.frame(0.0);
    let closing = height(&rt);
    rt.frame(0.05);
    assert!(height(&rt) < closing);
    rt.key(KeyEvent { key: rune::Key::Text("l".into()), mods: Default::default() });
    rt.frame(0.0);
    let opening = height(&rt);
    rt.frame(0.05);
    assert!(height(&rt) > opening);
    rt.frame(0.3);
    rt.key(KeyEvent { key: rune::Key::Text("h".into()), mods: Default::default() });
    rt.frame(0.05);
    let before_hover = height(&rt);
    let [x, y, w, h] = rt.elements().iter().find(|e| e.key.as_deref() == Some("comment-902") && !e.inert).unwrap().visible;
    rt.pointer_move(x + w / 2.0, y + h / 2.0);
    rt.frame(0.0);
    assert!((height(&rt) - before_hover).abs() < 3.0, "hover must not interrupt an active fold");
}

#[test]
fn keyboard_fold_holds_a_partly_visible_parent() {
    for source in [false, true] {
        let mut s = app(true, SavedView::in_memory());
        scene_comments(&mut s);
        let mut comments = crate::fixtures::thread();
        let sample = comments.iter().find(|comment| comment.id == 9011).unwrap().clone();
        for id in 9900..9925 {
            let mut comment = sample.clone();
            comment.id = id;
            comment.child_ids.clear();
            comments.iter_mut().find(|comment| comment.id == 901).unwrap().child_ids.push(id);
            comments.push(comment);
        }
        s.story.as_mut().unwrap().thread = hn_lib::comments::Thread::from_comments(&comments, Some(crate::story::VISIBLE_COMMENT_LEVELS), crate::story::REPLY_BATCH_SIZE);
        if source { s.open_citation(0); }
        s.story.as_mut().unwrap().with_thread(|thread| { assert!(thread.select(9010)); });
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.on_key = Some(on_key);
        rt.frame(0.0);
        let (viewport, parent) = if source { ("source-viewer", "source-comment-9010") } else { ("discussion-scroll", "comment-9010") };
        let element = |rt: &rune::Runtime<App>, key: &str| rt.elements().into_iter()
            .find(|e| e.key.as_deref() == Some(key) && !e.inert).unwrap();
        for key in ["h", "l"] {
            let current = element(&rt, parent);
            let offset = current.scrolled[1] + current.rect[1] - element(&rt, viewport).rect[1] + 20.0;
            rt.update(|_, tx| tx.scroll_to_instant(viewport, 0.0, offset));
            rt.frame(0.0);
            let before = element(&rt, parent);
            assert!(before.visible[3] > 0.0 && before.visible[3] < before.rect[3], "the parent is partly visible: {source} {:?}", before.rect);
            let mut positions = vec![before.rect[1]];
            rt.key(KeyEvent { key: rune::Key::Text(key.into()), mods: Default::default() });
            for dt in [0.0, 0.05, 0.05, 0.10] {
                rt.frame(dt);
                positions.push(element(&rt, parent).rect[1]);
            }
            assert!(positions.iter().all(|y| (*y - positions[0]).abs() < 1.0), "the visible parent stays in place: {positions:?}");
        }
        let current = element(&rt, parent);
        let offset = current.scrolled[1] + current.rect[1] + current.rect[3] - element(&rt, viewport).rect[1] + 30.0;
        rt.update(|_, tx| tx.scroll_to_instant(viewport, 0.0, offset));
        rt.frame(0.0);
        assert_eq!(element(&rt, parent).visible[3], 0.0, "the selected parent is offscreen");
        rt.key(KeyEvent { key: rune::Key::Text("h".into()), mods: Default::default() });
        rt.frame(0.0);
        assert!(element(&rt, parent).visible[3] > 0.0, "an offscreen keyboard target is revealed");
        rt.key(KeyEvent { key: rune::Key::Text("l".into()), mods: Default::default() });
        rt.frame(0.3);
        rt.update(|_, tx| tx.scroll_to_instant(viewport, 0.0, 100_000.0));
        rt.frame(0.0);
        assert!(!rt.elements().iter().any(|e| e.key.as_deref() == Some(parent) && !e.inert), "a distant virtual row is unmounted");
        rt.key(KeyEvent { key: rune::Key::Text("h".into()), mods: Default::default() });
        for dt in [0.0, 0.05, 0.10] { rt.frame(dt); }
        assert!(element(&rt, parent).visible[3] > 0.0, "a distant virtual target is revealed");
    }
}

#[test]
fn arriving_replies_fade_once_and_respect_reduced_motion() {
    for reduced in [false, true] {
        let mut s = app(true, SavedView::in_memory());
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        s.headless = false;
        s.open_citation(0);
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.set_system_reduced_motion(reduced);
        rt.frame(0.0);
        rt.poll = Some(poll);
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while rt.state.story.as_ref().unwrap().pending_reply.is_some() {
            rt.frame(0.0);
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        rt.frame(0.05);
        let opacity = rt.elements().iter().find(|e| e.key.as_deref() == Some("source-comment-9010")).unwrap().opacity;
        if reduced { assert_eq!(opacity, 1.0); }
        else { assert!(opacity > 0.0 && opacity < 1.0, "reply opacity at 50ms: {opacity}"); }
        rt.frame(1.0);
        rt.update(|_, tx| tx.scroll_to_instant("source-viewer", 0.0, 10000.0));
        rt.frame(0.0);
        rt.update(|_, tx| tx.scroll_to_instant("source-viewer", 0.0, 0.0));
        rt.frame(0.0);
        assert_eq!(rt.elements().iter().find(|e| e.key.as_deref() == Some("source-comment-9010")).unwrap().opacity, 1.0,
            "scrolling back does not replay the arrival");
    }
}

#[test]
#[ignore = "Metal-backed reply arrival and summary overflow capture"]
fn render_reply_and_summary_revision() {
    fn reply() -> App {
        let mut s = app(true, SavedView::in_memory());
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        s.headless = false;
        s.open_citation(0);
        s
    }
    fn reduced() -> App { let mut s = reply(); s.reduced = true; s }
    fn overflow() -> App {
        let mut s = app(true, SavedView::in_memory());
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        s.story.as_mut().unwrap().article_summary = crate::story::ArticleSummaryState::Done(
            "The article describes the design and its consequences for readers. ".repeat(14));
        s
    }
    fn delayed_poll(s: &mut App, now: f64, tx: &mut Tx) -> bool {
        if now < 0.2 { return false; }
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut changed = false;
        while s.story.as_ref().unwrap().pending_reply.is_some() {
            changed |= poll(s, now, tx);
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        changed | poll(s, now, tx)
    }
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/citation-nav-review");
    std::fs::create_dir_all(&out).unwrap();
    for (name, init, at, is_reply) in [
        ("reply-normal-50", reply as fn() -> App, "0.25", true),
        ("reply-normal-100", reply, "0.30", true),
        ("reply-reduced-50", reduced, "0.25", true),
        ("summary-overflow", overflow, "0.5", false),
    ] {
        let stem = out.join(name);
        let args = rune::tooling::Args::parse_from(vec!["citation-revision".into(),
            "--size".into(), "1180x760".into(), "--scale".into(), "2".into(),
            "--theme".into(), "dark".into(), "--at".into(), at.into(),
            "--snap".into(), stem.with_extension("png").display().to_string(),
            "--tree".into(), stem.with_extension("json").display().to_string()]);
        let mut spec = AppSpec::new("Hacker News", (1180.0, 760.0), init, view);
        if is_reply { spec.poll = Some(delayed_poll); }
        rune::tooling::headless(rune::App::new(spec), args);
    }
}

fn long_discussion() -> App {
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

#[test]
fn discussion_scroll_keeps_frames_paced_until_it_is_still() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let mut app = long_discussion();
    assert_eq!(app.story.as_ref().unwrap().thread.len(), 244, "the synchronous fixture loaded its comments");
    let wakes = Arc::new(AtomicUsize::new(0));
    let wake_count = Arc::clone(&wakes);
    app.scroll_waker = Some(Arc::new(move || { wake_count.fetch_add(1, Ordering::Relaxed); }));
    let mut rt = rune::Runtime::new(app, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
    rt.poll = Some(poll);
    for _ in 0..120 { rt.frame(1.0 / 60.0); }
    assert_eq!(rt.state.scroll_pacing.get(), ScrollPacing::Idle, "initial layout settles");
    assert_eq!(wakes.load(Ordering::Relaxed), 0, "initial offset is not a scroll");

    let viewport = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("discussion-scroll")).unwrap().rect;
    let point = (viewport[0] + viewport[2] / 2.0, viewport[1] + viewport[3] / 2.0);
    let scroll_y = |rt: &rune::Runtime<App>| rt.elements().iter()
        .find(|e| e.key.as_deref() == Some("discussion-header")).unwrap().scrolled[1];
    let before = scroll_y(&rt);
    let commits = rt.stats.commits;
    rt.wheel(point.0, point.1, -5.0);
    rt.frame(1.0 / 120.0);
    assert!(scroll_y(&rt) > before, "the real discussion viewport scrolled");
    assert_eq!(rt.state.scroll_pacing.get(), ScrollPacing::Moved);
    assert_eq!(wakes.load(Ordering::Relaxed), 1, "the first movement wakes poll");
    assert_eq!(rt.stats.commits, commits, "the moving offset callback does not rebuild the view");
    rt.frame(1.0 / 120.0);
    let ScrollPacing::Until(first_deadline) = rt.state.scroll_pacing.get() else { panic!("scroll starts pacing") };
    assert!(rt.wake_deadline().is_some(), "pacing asks for another frame");
    assert_eq!(rt.stats.commits, commits, "the offset callback does not rebuild the view");

    rt.wheel(point.0, point.1, -5.0);
    rt.frame(1.0 / 120.0);
    rt.frame(1.0 / 120.0);
    assert!(matches!(rt.state.scroll_pacing.get(), ScrollPacing::Until(until) if until > first_deadline), "further scrolling extends pacing");
    assert_eq!(wakes.load(Ordering::Relaxed), 1, "an active cadence needs no extra wake");
    for _ in 0..20 { rt.frame(1.0 / 120.0); }
    assert_eq!(rt.state.scroll_pacing.get(), ScrollPacing::Idle, "pacing ends after the scroll rests");
    assert!(rt.wake_deadline().is_none(), "the idle window has no pacing wake");
    assert!(!rt.live_pending(), "the idle window has no live work pending");

    rt.wheel(point.0, point.1, -5.0);
    rt.frame(1.0 / 120.0);
    assert_eq!(wakes.load(Ordering::Relaxed), 2, "a later gesture wakes poll again");
    for _ in 0..20 { rt.frame(1.0 / 120.0); }
    assert_eq!(rt.state.scroll_pacing.get(), ScrollPacing::Idle);
    assert!(rt.wake_deadline().is_none() && !rt.live_pending(), "the window returns to idle again");
}

#[test]
fn summary_and_source_scroll_return_to_idle() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    for source in [false, true] {
        let mut s = app(true, SavedView::in_memory());
        if source {
            let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
            fake.comments.lock().unwrap().iter_mut().find(|comment| comment.id == 901).unwrap().text =
                "A captured comment with enough text to scroll. ".repeat(120);
        }
        scene_story(&mut s);
        let story = s.story.as_mut().unwrap();
        if source {
            s.open_citation(0);
        } else {
            story.article_summary = crate::story::ArticleSummaryState::Done("A long article summary. ".repeat(120));
        }
        let wakes = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&wakes);
        s.scroll_waker = Some(Arc::new(move || { count.fetch_add(1, Ordering::Relaxed); }));
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.poll = Some(poll);
        for _ in 0..120 { rt.frame(1.0 / 60.0); }
        assert_eq!(rt.state.scroll_pacing.get(), ScrollPacing::Idle);
        let viewport = if source { "source-viewer" } else { "summary-scroll" };
        let content = if source { "source-content" } else { "article-summary" };
        let elements = rt.elements();
        let [x, y, w, h] = elements.iter().find(|e| e.key.as_deref() == Some(viewport)).unwrap().rect;
        let before = elements.iter().find(|e| e.key.as_deref() == Some(content)).unwrap().rect[1];
        rt.wheel(x + w / 2.0, y + h / 2.0, -10.0);
        rt.frame(1.0 / 120.0);
        let after = rt.elements().iter().find(|e| e.key.as_deref() == Some(content)).unwrap().rect[1];
        assert!(after < before, "{viewport} must actually scroll");
        assert_eq!(rt.state.scroll_pacing.get(), ScrollPacing::Moved);
        assert!(wakes.load(Ordering::Relaxed) > 0, "{viewport} wakes the frame clock");
        for _ in 0..20 { rt.frame(1.0 / 120.0); }
        assert_eq!(rt.state.scroll_pacing.get(), ScrollPacing::Idle);
        assert!(rt.wake_deadline().is_none() && !rt.live_pending(), "{viewport} returns to idle");
    }
}

#[test]
fn unavailable_reply_group_retries_from_pointer_and_keyboard() {
    for activation in [None, Some(rune::Key::Enter), Some(rune::Key::Right)] {
        let mut s = app(true, SavedView::in_memory());
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        let mut parent = crate::fixtures::comments().remove(0);
        parent.deleted = true;
        parent.child_ids = vec![9010];
        let child = crate::fixtures::thread().into_iter().find(|c| c.id == 9010).unwrap();
        *fake.comments.lock().unwrap() = vec![parent, child];
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        let story = s.story.as_mut().unwrap();
        let batch = story.thread.reply_batch(901, false).unwrap();
        story.thread.merge_replies(&batch, Err("Offline".into()));
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.on_key = Some(on_key);
        rt.frame(0.0);
        let elements = rt.elements();
        let control = elements.iter().find(|e| e.key.as_deref() == Some("comment-group-901")).unwrap();
        assert_eq!(control.label.as_deref(), Some("Retry replies: 1 reply"));
        assert!(!elements.iter().any(|e| e.key.as_deref() == Some("comment-load-901-true")));
        if let Some(key) = activation {
            route(&mut rt, key);
        } else {
            let [x, y, w, h] = control.rect;
            rt.pointer_down(x + w / 2.0, y + h / 2.0);
            rt.pointer_up(x + w / 2.0, y + h / 2.0);
            rt.frame(0.3);
        }
        let story = rt.state.story.as_ref().unwrap();
        assert_eq!(story.thread.children_of(901), &[9010]);
        assert!(story.thread.reply_batch(901, true).is_none());
    }
}

#[test]
fn keyboard_return_to_root_then_pointer_selection_keeps_the_visible_row() {
    for reduced in [false, true] {
        let mut s = app(true, SavedView::in_memory());
        scene_comments(&mut s);
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.set_system_reduced_motion(reduced);
        rt.on_key = Some(on_key);
        rt.poll = Some(poll);
        rt.frame(0.0);
        route(&mut rt, rune::Key::Text("j".into()));
        route(&mut rt, rune::Key::Text("u".into()));
        assert_eq!(rt.state.story.as_ref().unwrap().thread.cursor(), Some(901));
        let root = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("comment-901")).unwrap();
        assert!(root.visible[3] > 0.0, "the returned parent is revealed");
        let reply = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("comment-9010")).unwrap();
        let [x, y, w, h] = reply.visible;
        rt.pointer_move(x + w / 2.0 - 40.0, y + h / 2.0);
        rt.frame(0.0);
        rt.pointer_move(x + w / 2.0, y + h / 2.0);
        rt.frame(0.05);
        rt.frame(0.1);
        assert_eq!(rt.state.story.as_ref().unwrap().thread.cursor(), Some(9010));
        let after = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("comment-9010")).unwrap();
        assert_eq!(after.scrolled, reply.scrolled, "the pointer must not trigger a stale reveal");
    }
}

#[test]
fn discussion_boundary_key_does_not_pull_the_reader_back() {
    let mut rt = rune::Runtime::new(long_discussion(), view, (1180.0, 760.0), rune::Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    rt.poll = Some(poll);
    rt.frame(0.0);
    rt.update(|_, tx| tx.scroll_to_y("discussion-scroll", 1000.0));
    for _ in 0..40 { rt.frame(1.0 / 60.0); }
    let y = |rt: &rune::Runtime<App>| rt.elements().iter()
        .find(|e| e.key.as_deref() == Some("discussion-header")).unwrap().scrolled[1];
    let before = y(&rt);
    assert!(before > 0.0, "the reader scrolled away from the first comment");
    route(&mut rt, rune::Key::Text("k".into()));
    for _ in 0..20 { rt.frame(1.0 / 60.0); }
    assert!((y(&rt) - before).abs() < 0.5, "a boundary key must not reveal the unchanged cursor");
}

#[test]
#[ignore = "Explicit Metal-backed long-discussion mounting and reveal verification"]
fn render_long_discussion_workflows() {
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/ux-motion/comments");
    std::fs::create_dir_all(&out).unwrap();
    fn reduced() -> App { let mut s = long_discussion(); s.reduced = true; s }
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
        let mut spec = AppSpec::new("Hacker News", (1180.0,760.0), if name == "resize-reduced" { reduced } else { long_discussion }, view);
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
#[ignore = "Explicit Metal-backed continuous discussion scroll profile"]
fn render_continuous_discussion_scroll() {
    use std::io::BufRead;

    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/ux-motion/comments");
    std::fs::create_dir_all(&out).unwrap();
    let script = out.join("continuous-scroll-script.json");
    std::fs::write(&script, r#"[
        {"t":0.5,"type":"scroll","key":"discussion-scroll","dy":-1800,"dur":1.4},
        {"t":2.3,"type":"scroll","key":"discussion-scroll","dy":3000,"dur":1.4},
        {"t":4.1,"type":"scroll","key":"discussion-scroll","dy":-1800,"dur":1.4},
        {"t":5.9,"type":"scroll","key":"discussion-scroll","dy":3000,"dur":1.4}
    ]"#).unwrap();
    let stem = out.join("continuous-scroll");
    let trace = stem.with_extension("trace.jsonl");
    let args = rune::tooling::Args::parse_from(vec!["comments-scroll".into(),
        "--size".into(), "1180x760".into(), "--theme".into(), "dark".into(),
        "--script".into(), script.display().to_string(), "--at".into(), "7.8".into(),
        "--snap".into(), stem.with_extension("png").display().to_string(),
        "--tree".into(), stem.with_extension("json").display().to_string(),
        "--trace".into(), trace.display().to_string()]);
    let mut spec = AppSpec::new("Hacker News", (1180.0, 760.0), long_discussion, view);
    spec.on_key = Some(on_key);
    rune::tooling::headless(rune::App::new(spec), args);

    let mut discussion = None;
    let mut reached = [false; 4];
    for line in std::io::BufReader::new(std::fs::File::open(trace).unwrap()).lines() {
        let frame: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
        for meta in frame["m"].as_array().into_iter().flatten() {
            if meta["nm"].as_str().is_some_and(|name| name.ends_with("/discussion-scroll/discussion")) {
                discussion = meta["id"].as_u64();
            }
        }
        for offset in frame["sc"].as_array().into_iter().flatten() {
            if offset[0].as_u64() != discussion || discussion.is_none() { continue; }
            let (Some(t), Some(y)) = (frame["t"].as_f64(), offset[2].as_f64()) else { continue; };
            if (500.0..=2000.0).contains(&t) && y > 1000.0 { reached[0] = true; }
            if (2300.0..=3800.0).contains(&t) && y < 2.0 { reached[1] = true; }
            if (4100.0..=5600.0).contains(&t) && y > 1000.0 { reached[2] = true; }
            if (5900.0..=7400.0).contains(&t) && y < 2.0 { reached[3] = true; }
        }
    }
    assert_eq!(reached, [true; 4], "both cold and warm swipes must move down and back up");
}

#[test]
fn replies_use_actual_child_ids_and_never_change_the_summary_capture() {
    let mut s = app(true, SavedView::in_memory());
    s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
    let story = s.story.as_ref().unwrap();
    assert_eq!(story.thread.len(), 5);
    let sources: Vec<_> = story.summary.displayed().unwrap().sources().iter().map(|s| (s.id, s.text.clone())).collect();
    let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
    assert!(fake.comment_requests.lock().unwrap().is_empty(), "no reply prefetch");
    let summaries = *fake.summary_requests.lock().unwrap();
    s.activate_comment(901);
    assert_eq!(s.story.as_ref().unwrap().thread.children_of(901), &[9010, 9011]);
    s.activate_comment(9010);
    let story = s.story.as_ref().unwrap();
    assert_eq!(story.thread.children_of(9010), &[90100, 90101]);
    assert_eq!(story.thread.tree_depth(90100), Some(2));
    assert_eq!(story.summary.displayed().unwrap().sources().iter().map(|s| (s.id, s.text.clone())).collect::<Vec<_>>(), sources, "lazy replies do not alter citation identity");
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
    assert_eq!(story.summary.displayed().unwrap().sources().len(), 244, "summary capture has no arbitrary root cap");
    let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
    assert!(fake.comment_requests.lock().unwrap().is_empty());
    assert_eq!(*fake.summary_requests.lock().unwrap(), 1);
    fake.comments.lock().unwrap().clear();
    s.refresh_comments();
    let story = s.story.as_ref().unwrap();
    assert_eq!(story.discussion, Some(Ok(())));
    assert_eq!(story.thread.roots(), 244, "refresh retains previously loaded roots");
    assert_eq!(story.summary.displayed().unwrap().sources().len(), 244, "refresh preserves summary citations");
    let mut empty = app(true, SavedView::in_memory());
    let Source::Fake(fake) = empty.source.as_ref() else { unreachable!() };
    fake.comments.lock().unwrap().clear();
    empty.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
    let story = empty.story.as_ref().unwrap();
    assert_eq!(story.discussion, Some(Ok(())));
    assert!(story.thread.is_empty());
    assert!(story.summary.displayed().is_none_or(|summary| summary.sources().is_empty()));
}

#[test]
fn refresh_adds_roots_and_replies_without_replacing_summary_or_reading_position() {
    let mut s = app(true, SavedView::in_memory());
    s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
    s.activate_comment(901);
    let story = s.story.as_mut().unwrap();
    assert!(story.thread.reroot(901));
    assert!(story.thread.select(9010));
    assert!(story.thread.fold());
    let before = summary_contents(story.summary.readable().unwrap());
    let sources: Vec<_> = story.summary.displayed().unwrap().sources().iter().map(|s| (s.id, s.text.clone())).collect();
    let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
    let discussions = *fake.summary_requests.lock().unwrap();
    let articles = *fake.article_summary_requests.lock().unwrap();
    let mut new_root = crate::fixtures::comments().remove(0);
    new_root.id = 987654;
    new_root.child_ids.clear();
    let mut comments = fake.comments.lock().unwrap();
    comments.iter_mut().find(|c| c.id == 901).unwrap().child_ids.push(987655);
    comments.push(new_root);
    drop(comments);
    s.refresh_comments();
    let story = s.story.as_ref().unwrap();
    assert!(story.thread.node(987654).is_some());
    assert_eq!(story.thread.children_of(901), &[9010, 9011]);
    assert!(story.thread.reply_batch(901, false).unwrap().ids().contains(&987655));
    assert_eq!(story.thread.cursor(), Some(9010));
    assert_eq!(story.thread.root(), Some(901));
    assert!(story.thread.focus());
    assert!(story.thread.is_folded(9010));
    assert_eq!(summary_contents(story.summary.readable().unwrap()), before);
    assert_eq!(story.summary.displayed().unwrap().sources().iter().map(|s| (s.id, s.text.clone())).collect::<Vec<_>>(), sources);
    assert!(story.current_sources.iter().any(|s| s.id == 987654));
    let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
    assert_eq!(*fake.summary_requests.lock().unwrap(), discussions);
    assert_eq!(*fake.article_summary_requests.lock().unwrap(), articles);
    s.close_story();
    s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
    let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
    assert_eq!(*fake.summary_requests.lock().unwrap(), discussions);
    assert_eq!(*fake.article_summary_requests.lock().unwrap(), articles);
    let story = s.story.as_ref().unwrap();
    assert_eq!(summary_contents(story.summary.readable().unwrap()), before);
    assert_eq!(story.summary.displayed().unwrap().sources().iter().map(|s| (s.id, s.text.clone())).collect::<Vec<_>>(), sources);
    assert!(story.current_sources.iter().any(|s| s.id == 987654));
    s.retry_summary();
    let story = s.story.as_ref().unwrap();
    assert_eq!(story.thread.cursor(), Some(9010));
    assert_eq!(story.thread.children_of(901), &[9010, 9011]);
    assert!(matches!(story.article_summary, crate::story::ArticleSummaryState::Done(_)));
    assert!(matches!(story.summary, SummaryState::Done(_)));
    let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
    assert_eq!(*fake.summary_requests.lock().unwrap(), discussions + 1);
    assert_eq!(*fake.article_summary_requests.lock().unwrap(), articles + 1);
}

#[test]
fn failed_comment_refresh_leaves_the_existing_tree_and_summaries_readable() {
    let mut s = app(true, SavedView::in_memory());
    s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
    s.activate_comment(901);
    let before = summary_contents(s.story.as_ref().unwrap().summary.readable().unwrap());
    let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
    let calls = *fake.summary_requests.lock().unwrap();
    fake.failing_comments.lock().unwrap().insert(crate::fixtures::STORY_ID);
    s.refresh_comments();
    let story = s.story.as_ref().unwrap();
    assert!(!story.refreshing_comments);
    assert!(story.refresh_error.is_some());
    assert_eq!(story.thread.children_of(901), &[9010, 9011]);
    assert_eq!(summary_contents(story.summary.readable().unwrap()), before);
    let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
    assert_eq!(*fake.summary_requests.lock().unwrap(), calls);
}

#[test]
fn refresh_updates_loaded_nested_replies_and_keeps_new_children_lazy() {
    let mut s = app(true, SavedView::in_memory());
    s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
    s.activate_comment(901);
    s.activate_comment(9010);
    let story = s.story.as_mut().unwrap();
    assert!(story.thread.select(90100));
    assert!(story.thread.fold());
    let pending = story.request_replies(90100, false).unwrap();
    assert_eq!(pending.batch.parent_id(), 90100);
    assert_eq!(story.thread.tree_depth(90100), Some(2));
    let before = summary_contents(story.summary.readable().unwrap());
    let sources: Vec<_> = story.summary.displayed().unwrap().sources().iter().map(|source| (source.id, source.text.clone())).collect();
    let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
    let summaries = *fake.summary_requests.lock().unwrap();
    let mut comments = fake.comments.lock().unwrap();
    let nested = comments.iter_mut().find(|comment| comment.id == 90100).unwrap();
    nested.text = "Updated nested reply".into();
    nested.child_ids.push(987655);
    drop(comments);

    s.refresh_comments();
    let story = s.story.as_mut().unwrap();
    assert_eq!(story.thread.node(90100).unwrap().text, "Updated nested reply");
    assert_eq!(story.pending_reply.as_ref(), Some(&pending));
    assert_eq!(story.thread.cursor(), Some(90100));
    assert!(story.thread.is_folded(90100));
    assert_eq!(summary_contents(story.summary.readable().unwrap()), before);
    assert_eq!(story.summary.displayed().unwrap().sources().iter().map(|source| (source.id, source.text.clone())).collect::<Vec<_>>(), sources);
    let replies = crate::fixtures::replies(pending.batch.ids());
    assert!(story.land_replies(crate::story_io::ReplyFilled { request: pending, comments: Ok(replies) }));
    assert_eq!(story.thread.children_of(90100), &[901000]);
    assert_eq!(story.thread.reply_batch(90100, false).unwrap().ids(), &[987655]);
    assert!(story.thread.node(987655).is_none());
    let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
    let requests = fake.comment_requests.lock().unwrap();
    let refreshed = requests.last().unwrap();
    assert!(refreshed.contains(&90100));
    assert!(!refreshed.contains(&901000) && !refreshed.contains(&987655), "unloaded replies stay lazy");
    assert_eq!(*fake.summary_requests.lock().unwrap(), summaries);
}

#[test]
fn partial_comment_refresh_keeps_failed_nodes_and_applies_successes() {
    let mut s = app(true, SavedView::in_memory());
    s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
    s.activate_comment(901);
    let original = s.story.as_ref().unwrap().thread.node(9010).unwrap().text.clone();
    let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
    {
        let mut comments = fake.comments.lock().unwrap();
        comments.iter_mut().find(|comment| comment.id == 9010).unwrap().text = "Unavailable update".into();
        comments.iter_mut().find(|comment| comment.id == 9011).unwrap().text = "Available update".into();
    }
    fake.failing_comments.lock().unwrap().insert(9010);
    s.refresh_comments();
    let story = s.story.as_ref().unwrap();
    assert_eq!(story.thread.node(9010).unwrap().text, original);
    assert_eq!(story.thread.node(9011).unwrap().text, "Available update");
    assert!(story.refresh_error.as_deref().unwrap().contains("1 loaded reply"));

    let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
    fake.failing_comments.lock().unwrap().clear();
    fake.failing_comments.lock().unwrap().insert(crate::fixtures::STORY_ID);
    s.refresh_comments();
    let story = s.story.as_ref().unwrap();
    assert_eq!(story.thread.node(9010).unwrap().text, "Unavailable update", "loaded replies can refresh even when roots fail");
    assert!(story.refresh_error.is_some(), "the root failure remains visible");
    assert_eq!(story.discussion, Some(Ok(())));
}

#[test]
fn discussion_failure_is_explicit_retryable_and_does_not_generate_an_empty_summary() {
    let mut s = app(true, SavedView::in_memory());
    let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
    fake.failing_comments.lock().unwrap().insert(crate::fixtures::STORY_ID);
    s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
    assert!(matches!(s.story.as_ref().unwrap().discussion, Some(Err(_))));
    assert_eq!(s.story.as_ref().unwrap().summary_notice().as_deref(), Some("Discussion unavailable · press r to retry comments"));
    let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
    assert_eq!(*fake.summary_requests.lock().unwrap(), 0);
    fake.failing_comments.lock().unwrap().clear();
    s.retry_summary();
    assert_eq!(s.story.as_ref().unwrap().discussion, Some(Ok(())));
    assert_eq!(s.story.as_ref().unwrap().thread.roots(), 5);
}

#[test]
fn enter_and_o_toggle_discussion_comments_after_replies_land() {
    for key in [rune::Key::Enter, rune::Key::Text("o".into())] {
        let mut s = app(true, SavedView::in_memory());
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        s.headless = false;
        assert!(s.story().unwrap().thread.children_of(901).is_empty());
        press(&mut s, key.clone());
        poll_until(&mut s, |s| s.story().unwrap().thread.children_of(901) == [9010, 9011]);
        press(&mut s, key.clone());
        assert!(s.story().unwrap().thread.is_folded(901), "{key:?} must fold after lazy replies land");

        s.headless = true;
        scene_comments(&mut s);
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.on_key = Some(on_key);
        rt.frame(0.0);
        let [x, y, w, h] = rt.elements().iter().find(|e| e.key.as_deref() == Some("comment-text-901")).unwrap().visible;
        rt.pointer_down(x + w / 2.0, y + h / 2.0);
        rt.pointer_up(x + w / 2.0, y + h / 2.0);
        rt.frame(0.0);

        route(&mut rt, rune::Key::Text("h".into()));
        assert!(rt.state.story().unwrap().thread.is_folded(901));
        route(&mut rt, key.clone());
        assert!(!rt.state.story().unwrap().thread.is_folded(901), "{key:?} must unfold with preloaded replies");
        route(&mut rt, key.clone());
        assert!(rt.state.story().unwrap().thread.is_folded(901), "{key:?} must fold with preloaded replies");
        assert!(!rt.elements().iter().any(|e| e.key.as_deref() == Some("comment-9010") && !e.exiting));
        route(&mut rt, rune::Key::Text("l".into()));
        for _ in 0..3 { route(&mut rt, rune::Key::Down); }
        assert_eq!(rt.state.story().unwrap().thread.cursor(), Some(901000));
        route(&mut rt, key);
        assert_eq!(rt.state.story().unwrap().thread.root(), Some(901000), "continue thread re-roots");
    }
}

#[test]
fn pending_replies_stay_folded_and_a_replaced_story_cannot_accept_them() {
    let mut s = app(true, SavedView::in_memory());
    s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
    s.headless = false;
    s.source = Arc::new(Source::Fake(crate::service::fake::FakeService::fixture().with_delay(std::time::Duration::from_millis(25))));
    s.activate_comment(901);
    s.refresh_comments();
    assert!(s.story.as_ref().unwrap().pending_reply.is_some() && s.story.as_ref().unwrap().refreshing_comments);
    s.close_story();
    s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
    assert!(s.story.as_ref().unwrap().pending_reply.is_none() && !s.story.as_ref().unwrap().refreshing_comments);
    s.activate_comment(901);
    s.refresh_comments();
    let request = s.story.as_ref().unwrap().pending_reply.clone().unwrap();
    s.activate_comment(901);
    assert!(s.story.as_ref().unwrap().thread.is_folded(901), "second activation folds pending branch");
    s.activate_comment(901);
    assert!(!s.story.as_ref().unwrap().thread.is_folded(901));
    press(&mut s, rune::Key::Text("c".into()));
    assert!(s.story.as_ref().unwrap().thread.is_folded(901), "c folds the branch while replies are loading");
    let mut tx = Tx::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while (s.story.as_ref().unwrap().pending_reply.is_some() || s.story.as_ref().unwrap().refreshing_comments)
        && std::time::Instant::now() < deadline {
        poll(&mut s, 0.0, &mut tx); std::thread::yield_now();
    }
    assert!(s.story.as_ref().unwrap().pending_reply.is_none());
    assert!(s.story.as_ref().unwrap().thread.is_folded(901));
    assert_eq!(s.story.as_ref().unwrap().thread.children_of(901).len(), 2);
    s.close_story();
    s.headless = true;
    s.open_story(crate::fixtures::item(0, 1));
    assert!(!s.story.as_mut().unwrap().land_replies(crate::story_io::ReplyFilled { request, comments: Ok(crate::fixtures::replies(&[9010,9011])) }));
    assert_eq!(s.story.as_ref().unwrap().thread.len(), 5);
}


/// The user's mixed sequence: the pointer rests over the discussion while Enter expands a
/// comment, its replies draw under the pointer, the hand nudges the trackpad, and Enter again
/// should fold the comment it expanded.
#[test]
fn a_resting_pointer_does_not_take_the_selection_from_enter() {
    for (nudge, focus) in [(0.0, true), (1.0, true), (1.0, false)] {
        let mut s = app(true, SavedView::in_memory());
        scene_comments(&mut s);
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.on_key = Some(on_key);
        rt.frame(0.0);
        if !focus { route(&mut rt, rune::Key::Text("f".into())); }
        let reply = |rt: &rune::Runtime<App>| rt.elements().iter()
            .find(|e| e.key.as_deref() == Some("comment-9010") && !e.exiting).map(|e| e.visible);
        let [x, y, w, h] = reply(&rt).expect("9010 drawn while 901 is open");
        let (px, py) = (x + w / 2.0, y + h / 2.0);
        rt.state.story_mut().unwrap().with_thread(|t| { t.select(901); t.fold(); });
        rt.frame(0.3);
        rt.pointer_move(px, py);
        rt.frame(0.3);
        rt.state.story_mut().unwrap().with_thread(|t| { t.select(901); });
        rt.frame(0.3);
        route(&mut rt, rune::Key::Enter);
        for _ in 0..10 { rt.frame(0.1); }
        assert!(!rt.state.story().unwrap().thread.is_folded(901));
        if nudge > 0.0 { rt.pointer_move(px, py + nudge); rt.frame(0.1); }
        let cursor = rt.state.story().unwrap().thread.cursor();
        route(&mut rt, rune::Key::Enter);
        assert!(rt.state.story().unwrap().thread.is_folded(901),
            "nudge {nudge} focus {focus}: second Enter must fold 901; cursor was {cursor:?}");
    }
}

#[test]
fn keyboard_selection_survives_pointer_jitter_in_discussion() {
    let mut s = app(true, SavedView::in_memory());
    scene_comments(&mut s);
    let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    rt.frame(0.0);
    let cursor = |rt: &rune::Runtime<App>| rt.state.story().unwrap().thread.cursor();
    let [x, y, w, h] = rt.elements().iter()
        .find(|e| e.key.as_deref() == Some("comment-901")).unwrap().visible;
    let (px, py) = (x + w / 2.0, y + h / 2.0);
    rt.pointer_move(px, py);
    rt.frame(0.0);

    for (key, expected, nudge) in [("j", 9010, 1.0), ("j", 90100, 2.0), ("k", 9010, 3.0)] {
        route(&mut rt, rune::Key::Text(key.into()));
        assert_eq!(cursor(&rt), Some(expected));
        rt.pointer_move(px + nudge, py);
        rt.frame(0.1);
        assert_eq!(cursor(&rt), Some(expected), "{key} keeps selection after a 1 pt nudge");
    }
    rt.pointer_move(px + 43.0, py);
    rt.frame(0.1);
    assert_eq!(cursor(&rt), Some(901));
}
