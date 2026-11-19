//! Citation selection and captured source navigation.

use super::super::*;
use rune::KeyEvent;
use crate::story_io::StoryUpdate;
use super::super::input::on_key;
use super::super::scenes::scene_story;
use super::super::window::view;
use super::support::*;

#[test]
fn enter_and_o_toggle_citation_source_and_reply() {
    for key in [rune::Key::Enter, rune::Key::Text("o".into())] {
        let mut s = app(true, SavedView::in_memory());
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        s.headless = false;
        press(&mut s, rune::Key::Tab);
        press(&mut s, rune::Key::Enter);
        assert_eq!(s.story().unwrap().source().unwrap().id(), 901);
        poll_until(&mut s, |s| s.story().unwrap().thread.children_of(901) == [9010, 9011]);

        press(&mut s, key.clone());
        assert!(s.story().unwrap().source().unwrap().nav.folded.contains(&901), "{key:?} must fold the source comment");
        press(&mut s, key.clone());
        assert!(!s.story().unwrap().source().unwrap().nav.folded.contains(&901));

        press(&mut s, rune::Key::Down);
        assert_eq!(s.story().unwrap().source().unwrap().nav.cursor, Some(9010));
        press(&mut s, key.clone());
        poll_until(&mut s, |s| s.story().unwrap().thread.children_of(9010) == [90100, 90101]);
        press(&mut s, key);
        assert!(s.story().unwrap().source().unwrap().nav.folded.contains(&9010), "repeat must fold the loaded source reply");
    }
}

#[test]
fn source_keys_keep_discussion_state_and_load_reply_context() {
    let mut s = app(true, SavedView::in_memory());
    s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
    s.load_replies(901, false);
    s.story.as_mut().unwrap().thread.select(901);
    s.story.as_mut().unwrap().thread.fold();
    s.story.as_mut().unwrap().thread.reroot(902);
    let story = s.story.as_ref().unwrap();
    assert!(story.thread.focus());
    let cursor = story.thread.cursor();
    let root = story.thread.root();
    let folded = story.thread.is_folded(901);
    press(&mut s, rune::Key::Tab);
    press(&mut s, rune::Key::Enter);
    assert_eq!(s.story.as_ref().unwrap().source().map(|source| source.id()), Some(901));
    assert_eq!(s.story.as_ref().unwrap().thread.children_of(901), vec![9010, 9011]);
    assert_eq!(s.story.as_ref().unwrap().source().unwrap().nav.cursor, Some(901));
    press(&mut s, rune::Key::Down);
    assert_eq!(s.story.as_ref().unwrap().source().unwrap().nav.cursor, Some(9010));
    press(&mut s, rune::Key::Enter);
    assert!(!s.story.as_ref().unwrap().thread.children_of(9010).is_empty(), "Enter loads the selected reply branch");
    press(&mut s, rune::Key::Text("j".into()));
    assert_eq!(s.story.as_ref().unwrap().source().unwrap().nav.cursor, Some(90100));
    press(&mut s, rune::Key::Text("c".into()));
    assert_eq!(s.story.as_ref().unwrap().source().unwrap().nav.cursor, Some(9010));
    assert!(s.story.as_ref().unwrap().source().unwrap().nav.folded.contains(&9010));
    let story = s.story.as_ref().unwrap();
    assert_eq!((story.thread.cursor(), story.thread.root(), story.thread.is_folded(901)), (cursor, root, folded));
    press(&mut s, rune::Key::Escape);
    assert!(s.story.as_ref().unwrap().source().is_none());
    press(&mut s, rune::Key::Escape);
    assert!(s.story.is_none(), "default focus mode does not consume back");
}

#[test]
fn pointer_back_unwinds_source_then_story_and_preserves_discussion() {
    let mut s = app(true, SavedView::in_memory());
    s.request(grid(11, 1), Ranking::Fresh);
    s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
    s.load_replies(901, false);
    let thread = &mut s.story.as_mut().unwrap().thread;
    thread.select(901);
    thread.fold();
    let before = (thread.cursor(), thread.root(), thread.is_folded(901));
    let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
    rt.frame(0.0);
    rt.update(|s, _| s.open_citation(0));
    rt.frame(0.3);
    let button = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("story-back")).unwrap();
    assert_eq!(button.label.as_deref(), Some("Back to discussion"));
    assert!(button.focusable && button.rect[2] >= 36.0 && button.rect[3] >= 36.0);
    let [x, y, w, h] = button.rect;
    rt.pointer_move(x + w / 2.0, y + h / 2.0);
    rt.frame(0.1);
    assert!(rt.elements().iter().find(|e| e.key.as_deref() == Some("story-back")).unwrap().hover > 0.0);
    rt.pointer_down(x + w / 2.0, y + h / 2.0);
    rt.pointer_up(x + w / 2.0, y + h / 2.0);
    rt.frame(0.3);
    let story = rt.state.story.as_ref().unwrap();
    assert!(story.source().is_none() && story.focused_citation().is_some());
    assert_eq!((story.thread.cursor(), story.thread.root(), story.thread.is_folded(901)), before);
    assert!(rt.elements().iter().any(|e| e.key.as_deref() == Some("summary-scroll") && e.focused));
    assert_eq!(rt.elements().iter().find(|e| e.key.as_deref() == Some("story-back")).unwrap().label.as_deref(), Some("Back to feed"));

    rt.update(|s, _| s.open_citation(0));
    rt.frame(0.3);
    let [x, y, w, h] = rt.elements().iter().find(|e| e.key.as_deref() == Some("source-comment-901")).unwrap().visible;
    let (x, y) = (x + w / 2.0, y + h / 2.0);
    for button in [rune::PointerButton::Forward, rune::PointerButton::Middle, rune::PointerButton::Secondary] {
        rt.button_down(x, y, button);
        rt.button_up(x, y, button);
        rt.frame(0.0);
        assert!(rt.state.story.as_ref().unwrap().source().is_some());
    }
    rt.button_down(x, y, rune::PointerButton::Back);
    rt.button_up(x, y, rune::PointerButton::Back);
    rt.frame(0.3);
    assert!(rt.state.story.as_ref().unwrap().source().is_none());
    let [x, y, w, h] = rt.elements().iter().find(|e| e.key.as_deref() == Some("article-open-original")).unwrap().rect;
    rt.button_down(x + w / 2.0, y + h / 2.0, rune::PointerButton::Back);
    rt.button_up(x + w / 2.0, y + h / 2.0, rune::PointerButton::Back);
    rt.frame(0.3);
    assert!(rt.state.story.is_none());
    rt.update(|s, _| s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID)));
    rt.frame(0.3);
    let [x, y, w, h] = rt.elements().iter().find(|e| e.key.as_deref() == Some("comment-disclosure-901")).unwrap().visible;
    rt.button_down(x + w / 2.0, y + h / 2.0, rune::PointerButton::Back);
    rt.button_up(x + w / 2.0, y + h / 2.0, rune::PointerButton::Back);
    rt.frame(0.3);
    assert!(rt.state.story.is_none(), "Back over a disclosure returns to the feed");
}

#[test]
fn source_selection_context_and_pointer_tab_follow_the_visible_citation() {
    let mut s = app(true, SavedView::in_memory());
    let mut fake = crate::service::fake::FakeService::fixture();
    fake.summary = Arc::new(|_| Box::pin(async {
        let mut summary = crate::fixtures::summary();
        summary.points[1].sources.push(901);
        Ok(summary)
    }));
    s.source = Arc::new(Source::Fake(fake));
    s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
    let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    rt.frame(0.0);
    route(&mut rt, rune::Key::Tab);
    route(&mut rt, rune::Key::Enter);
    let before = rt.elements();
    assert!(before.iter().any(|e| e.key.as_deref() == Some("source-summary-0")));
    assert!(before.iter().any(|e| e.key.as_deref() == Some("source-summary-1")), "all matching claims are shown");
    assert!(before.iter().any(|e| e.key.as_deref() == Some("source-comment-901") && e.item_selected));
    route(&mut rt, rune::Key::Down);
    assert!(rt.elements().iter().any(|e| e.key.as_deref() == Some("source-comment-9010") && e.item_selected));
    let [x, y, w, h] = rt.elements().iter().find(|e| e.key.as_deref() == Some("source-comment-9010")).unwrap().rect;
    rt.pointer_down(x + w / 2.0, y + h / 2.0);
    rt.pointer_up(x + w / 2.0, y + h / 2.0);
    rt.frame(0.0);
    route(&mut rt, rune::Key::Enter);
    assert!(!rt.state.story.as_ref().unwrap().thread.children_of(9010).is_empty(), "Enter activates the pointed reply");
    route(&mut rt, rune::Key::Tab);
    let story = rt.state.story.as_ref().unwrap();
    assert_eq!(story.source().map(|source| source.id()), Some(902));
    assert_eq!(story.source().unwrap().nav.cursor, Some(902));
    let after = rt.elements();
    assert!(after.iter().any(|e| e.key.as_deref() == Some("source-summary-0")));
    assert!(!after.iter().any(|e| e.key.as_deref() == Some("source-summary-1")));
}

#[test]
fn captured_source_remains_selected_when_the_live_comment_changes_or_is_gone() {
    for live in [false, true] {
        let mut s = app(true, SavedView::in_memory());
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        let story = s.story.as_mut().unwrap();
        let captured = story.summary.displayed().unwrap().sources().iter().find(|source| source.id == 901).unwrap().clone();
        if live {
            let mut comments = crate::fixtures::comments();
            comments[0].author = "new author".into();
            comments[0].text = "new text".into();
            story.thread.merge_comments(&comments, &[]);
        } else {
            story.thread = crate::comments::Thread::empty();
        }
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.on_key = Some(on_key);
        rt.frame(0.0);
        route(&mut rt, rune::Key::Tab);
        route(&mut rt, rune::Key::Enter);
        let rows = rt.elements();
        assert!(rows.iter().any(|e| e.key.as_deref() == Some("source-comment-901") && e.item_selected));
        assert!(rows.iter().any(|e| e.key.as_deref() == Some("comment-author-901") && e.text.as_deref() == Some(captured.author.as_str())));
        assert!(rows.iter().any(|e| e.key.as_deref() == Some("source-comment-text") && e.text.as_deref() == Some(captured.text.as_str())));
        if !live {
            route(&mut rt, rune::Key::Down);
            assert_eq!(rt.state.story.as_ref().unwrap().source().unwrap().nav.cursor, Some(901));
        }
    }
}

#[test]
fn source_selection_survives_a_refreshed_reply_becoming_unavailable() {
    for reroot in [false, true] {
        let mut s = app(true, SavedView::in_memory());
        s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
        s.open_citation(0);
        press(&mut s, rune::Key::Text("G".into()));
        assert_eq!(s.story.as_ref().unwrap().source().unwrap().nav.cursor, Some(9011));
        if reroot { s.story.as_mut().unwrap().with_thread(|thread| { thread.reroot(9011); }); }
        press(&mut s, rune::Key::Text("f".into()));
        let Source::Fake(fake) = s.source.as_ref() else { unreachable!() };
        fake.comments.lock().unwrap().iter_mut().find(|comment| comment.id == 9011).unwrap().deleted = true;
        s.refresh_comments();
        let source = s.story.as_ref().unwrap().source().unwrap();
        assert_eq!(source.nav.cursor, Some(if reroot { 901 } else { 9010 }));
        assert!(!source.nav.focus, "refresh preserves the chosen focus mode");
        press(&mut s, rune::Key::Up);
        assert_eq!(s.story.as_ref().unwrap().source().unwrap().nav.cursor, Some(901));
    }
}

#[test]
fn discussion_selection_after_source_return_gives_enter_back_to_the_comment() {
    let mut s = app(true, SavedView::in_memory());
    s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
    let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    rt.frame(0.0);
    route(&mut rt, rune::Key::Tab);
    route(&mut rt, rune::Key::Enter);
    route(&mut rt, rune::Key::Escape);
    route(&mut rt, rune::Key::Down);
    route(&mut rt, rune::Key::Enter);
    let story = rt.state.story.as_ref().unwrap();
    assert!(story.source().is_none(), "Enter belongs to the selected discussion comment");
    assert!(!story.thread.children_of(9010).is_empty(), "Enter loads the selected comment's replies");
    let [x, y, w, h] = rt.elements().iter().find(|e| e.key.as_deref() == Some("comment-9010")).unwrap().rect;
    rt.pointer_down(x + w / 2.0, y + h / 2.0);
    rt.pointer_up(x + w / 2.0, y + h / 2.0);
    rt.frame(0.0);
    route(&mut rt, rune::Key::Enter);
    assert!(rt.state.story.as_ref().unwrap().thread.is_folded(9010), "Enter after clicking folds the comment");
    assert!(rt.elements().iter().any(|e| e.key.as_deref() == Some("comment-text-9010")), "the selected comment remains readable");
}

#[test]
fn dragging_reading_text_copies_without_pressing_the_comment() {
    let mut s = app(true, SavedView::in_memory());
    scene_story(&mut s);
    let thread = &mut s.story.as_mut().unwrap().thread;
    if thread.focus() { thread.toggle_focus(); }
    assert!(thread.select(9010));
    let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    let clipboard = rune::clipboard::memory();
    rt.use_clipboard(&clipboard);
    rt.frame(0.0);

    let [x, y, _, _] = rt.elements().iter().find(|e| e.key.as_deref() == Some("comment-text-901")).unwrap().rect;
    rt.pointer_down(x + 2.0, y + 7.0); rt.frame(0.0);
    rt.pointer_move(x + 125.0, y + 7.0); rt.frame(0.0);
    rt.pointer_up(x + 125.0, y + 7.0); rt.frame(0.0);
    let selected = rt.selected_text().expect("drag selects comment text");
    assert!(selected.len() > 5);
    assert!(rt.state.story.as_ref().unwrap().thread.node(901).unwrap().text.contains(&selected));
    assert_eq!(rt.state.story.as_ref().unwrap().thread.cursor(), Some(9010), "a selection drag does not press its row");
    let cmd_c = || KeyEvent { key: rune::Key::Command('c'), mods: rune::Mods { cmd: true, ..Default::default() } };
    rt.key(cmd_c()); rt.frame(0.0);
    assert_eq!(clipboard.text().as_deref().map(String::as_str), Some(selected.as_str()));

    route(&mut rt, rune::Key::Tab);
    assert!(rt.state.story.as_ref().unwrap().focused_citation().is_some());
    let elements = rt.elements();
    let [x, y, _, _] = elements.iter().find(|e| e.key.as_deref() == Some("summary-text-0")).unwrap().rect;
    let [cx, cy, cw, ch] = elements.iter().find(|e| e.key.as_deref() == Some("cite-1")).unwrap().rect;
    rt.pointer_down(x + 2.0, y + 7.0); rt.frame(0.0);
    rt.pointer_move(cx + cw - 1.0, cy + ch / 2.0); rt.frame(0.0);
    rt.pointer_up(cx + cw - 1.0, cy + ch / 2.0); rt.frame(0.0);
    let selected = rt.selected_text().expect("drag selects summary point");
    assert!(selected.contains("[1]") && selected.contains("[2]"), "inline citations remain part of a selection: {selected:?}");
    assert!(rt.state.story.as_ref().unwrap().focused_citation().is_some());
    rt.key(cmd_c()); rt.frame(0.0);
    assert_eq!(clipboard.text().as_deref().map(String::as_str), Some(selected.as_str()));

    route(&mut rt, rune::Key::Enter);
    let [x, y, _, _] = rt.elements().iter().find(|e| e.key.as_deref() == Some("source-comment-text")).unwrap().rect;
    rt.pointer_down(x + 2.0, y + 7.0); rt.frame(0.0);
    rt.pointer_move(x + 100.0, y + 7.0); rt.frame(0.0);
    rt.pointer_up(x + 100.0, y + 7.0); rt.frame(0.0);
    let selected = rt.selected_text().expect("drag selects the captured comment");
    rt.key(cmd_c()); rt.frame(0.0);
    assert_eq!(clipboard.text().as_deref().map(String::as_str), Some(selected.as_str()));

    route(&mut rt, rune::Key::Escape);
    rt.update(|s, _| { s.story.as_mut().unwrap().thread.toggle_focus(); });
    rt.frame(0.0);
    let [x, y, _, _] = rt.elements().iter().find(|e| e.key.as_deref() == Some("comment-text-901")).unwrap().rect;
    rt.pointer_down(x + 2.0, y + 7.0); rt.frame(0.0);
    rt.pointer_move(x + 110.0, y + 7.0); rt.frame(0.0);
    rt.pointer_up(x + 110.0, y + 7.0); rt.frame(0.0);
    let selected = rt.selected_text().expect("drag selects a comment with focus mode on");
    rt.key(cmd_c()); rt.frame(0.0);
    assert_eq!(clipboard.text().as_deref().map(String::as_str), Some(selected.as_str()));
}

#[test]
fn clicking_selectable_text_keeps_comment_and_citation_actions() {
    let mut s = app(true, SavedView::in_memory());
    scene_story(&mut s);
    let thread = &mut s.story.as_mut().unwrap().thread;
    if thread.focus() { thread.toggle_focus(); }
    let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    rt.frame(0.0);

    let [x, y, _, _] = rt.elements().iter().find(|e| e.key.as_deref() == Some("comment-text-9010")).unwrap().rect;
    rt.pointer_down(x + 8.0, y + 7.0); rt.pointer_up(x + 8.0, y + 7.0); rt.frame(0.0);
    assert_eq!(rt.state.story.as_ref().unwrap().thread.cursor(), Some(9010));
    assert!(rt.selected_text().is_none());

    let [x, y, _, _] = rt.elements().iter().find(|e| e.key.as_deref() == Some("summary-text-0")).unwrap().rect;
    rt.pointer_down(x + 8.0, y + 7.0); rt.pointer_up(x + 8.0, y + 7.0); rt.frame(0.0);
    assert!(rt.elements().iter().any(|e| e.key.as_deref() == Some("summary-scroll") && e.focused));
    let [x, y, w, h] = rt.elements().iter().find(|e| e.key.as_deref() == Some("cite-0")).unwrap().rect;
    rt.pointer_down(x + w / 2.0, y + h / 2.0); rt.pointer_up(x + w / 2.0, y + h / 2.0); rt.frame(0.0);
    assert_eq!(rt.state.story.as_ref().unwrap().source().map(|source| source.id()), Some(901));
}

#[test]
fn inline_citation_keyboard_opens_and_returns_to_its_reference() {
    let mut s = app(true, SavedView::in_memory());
    s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
    let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    rt.frame(0.0);
    let cite = rt.elements().iter().find(|e| e.key.as_deref() == Some("cite-0")).unwrap().clone();
    assert!(cite.focusable && cite.role == "button" && cite.label.as_deref() == Some("Read comment by tptacek"));
    route(&mut rt, rune::Key::Tab);
    route(&mut rt, rune::Key::Enter);
    assert_eq!(rt.state.story.as_ref().unwrap().source().map(|source| source.id()), Some(901));
    assert!(rt.elements().iter().find(|e| e.key.as_deref() == Some("discussion-scroll")).unwrap().inert);
    assert!(!rt.accessibility_json().to_string().contains("Summary content"), "the hidden inspector leaves the accessibility tree");
    route(&mut rt, rune::Key::Escape);
    assert!(rt.state.story.as_ref().unwrap().source().is_none());
    assert!(rt.state.story.as_ref().unwrap().focused_citation().is_some());
    assert_eq!(rt.state.story.as_ref().unwrap().focused_citation().map(|citation| citation.position), Some(0));
    assert!(rt.elements().iter().any(|e| e.key.as_deref() == Some("summary-scroll") && e.focused), "native focus returns to the summary");
    route(&mut rt, rune::Key::Enter);
    assert_eq!(rt.state.story.as_ref().unwrap().source().map(|source| source.id()), Some(901), "Enter reopens the returned citation");
}

#[test]
fn clicking_load_replies_releases_citation_enter_ownership() {
    let mut s = app(true, SavedView::in_memory());
    s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
    s.story.as_mut().unwrap().thread.toggle_focus();
    let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    rt.frame(0.0);
    route(&mut rt, rune::Key::Tab);
    let [x, y, w, h] = rt.elements().iter().find(|e| e.key.as_deref() == Some("comment-load-901-false")).unwrap().visible;
    assert!(w > 0.0 && h > 0.0);
    rt.pointer_down(x + w / 2.0, y + h / 2.0);
    rt.pointer_up(x + w / 2.0, y + h / 2.0);
    rt.frame(0.0);
    assert!(!rt.state.story.as_ref().unwrap().focused_citation().is_some());
    route(&mut rt, rune::Key::Enter);
    assert!(rt.state.story.as_ref().unwrap().source().is_none());
}

#[test]
fn clicking_summary_keeps_tab_and_shift_tab_on_citations() {
    let mut s = app(true, SavedView::in_memory());
    s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
    let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    rt.frame(0.0);
    let [x, y, w, h] = rt.elements().iter().find(|e| e.key.as_deref() == Some("article-summary")).unwrap().rect;
    rt.pointer_down(x + w / 2.0, y + h / 2.0);
    rt.pointer_up(x + w / 2.0, y + h / 2.0);
    rt.frame(0.0);
    assert!(rt.elements().iter().any(|e| e.key.as_deref() == Some("summary-scroll") && e.focused));
    route(&mut rt, rune::Key::Tab);
    assert!(rt.state.story.as_ref().unwrap().focused_citation().is_some(), "Tab after a pointer click must select a citation");
    assert_eq!(rt.state.story.as_ref().unwrap().focused_citation().map(|citation| citation.position), Some(0));
    route(&mut rt, rune::Key::Tab);
    assert_eq!(rt.state.story.as_ref().unwrap().focused_citation().map(|citation| citation.position), Some(1));
    rt.key(KeyEvent { key: rune::Key::Tab, mods: rune::Mods { shift: true, ..Default::default() } });
    rt.frame(0.3);
    assert_eq!(rt.state.story.as_ref().unwrap().focused_citation().map(|citation| citation.position), Some(0));
    route(&mut rt, rune::Key::Enter);
    assert_eq!(rt.state.story.as_ref().unwrap().source().map(|source| source.id()), Some(901));
    route(&mut rt, rune::Key::Escape);
    route(&mut rt, rune::Key::Tab);
    assert_eq!(rt.state.story.as_ref().unwrap().focused_citation().map(|citation| citation.position), Some(1), "Tab must continue after returning from a source");
    route(&mut rt, rune::Key::Enter);
    assert_eq!(rt.state.story.as_ref().unwrap().source().map(|source| source.id()), Some(902));
}

#[test]
fn source_return_keeps_the_visible_passage_when_replies_arrive_above_it() {
    let mut s = app(true, SavedView::in_memory());
    if let Source::Fake(fake) = s.source.as_ref() {
        let mut comments = fake.comments.lock().unwrap();
        let template = comments.iter().find(|comment| comment.id == 902).unwrap().clone();
        comments.extend((10000..10020).map(|id| { let mut comment = template.clone(); comment.id = id; comment }));
    }
    s.open_story(crate::fixtures::item(crate::fixtures::STORY_INDEX, crate::fixtures::STORY_ID));
    let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
    rt.frame(0.0);
    rt.update(|_, tx| tx.scroll_to_instant("discussion-scroll", 0.0, 700.0));
    rt.frame(0.0);
    let before = rt.elements();
    let anchor = before.iter().find(|e| e.key.as_deref().is_some_and(|key| key.starts_with("comment-100")) && e.visible[3] > 30.0).unwrap();
    let key = anchor.key.clone().unwrap();
    let y = anchor.rect[1];
    rt.update(|s, _| s.open_citation(0));
    rt.frame(0.0);
    let [x, button_y, w, h] = rt.elements().iter().find(|e| e.key.as_deref() == Some("story-back")).unwrap().rect;
    rt.pointer_down(x + w / 2.0, button_y + h / 2.0);
    rt.pointer_up(x + w / 2.0, button_y + h / 2.0);
    rt.frame(0.3);
    let after = rt.elements();
    let restored = after.iter().find(|e| e.key.as_deref() == Some(&key)).unwrap();
    assert!((restored.rect[1] - y).abs() < 2.0, "same passage returns at the same position");
}

#[test]
fn returning_to_a_replacement_without_citations_restores_summary_focus() {
    let mut s = app(true, SavedView::in_memory());
    scene_story(&mut s);
    s.open_citation(0);
    let story = s.story.as_mut().unwrap();
    story.retry(true);
    let mut replacement = crate::fixtures::summary();
    for point in &mut replacement.points { point.sources.clear(); }
    story.land_update(StoryUpdate::Summary {
        story_id: story.id(), summary: Ok((replacement, crate::fixtures::sources())),
        header: hn_lib::reader::saved::Header::new(story.id(), Default::default(), Default::default()), coverage: String::new(),
    }, None);
    let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    rt.frame(0.0);
    route(&mut rt, rune::Key::Escape);
    assert!(rt.state.story.as_ref().unwrap().source().is_none());
    assert!(rt.elements().iter().any(|e| e.key.as_deref() == Some("summary-scroll") && e.focused));
}

#[test]
fn citation_navigation_moves_as_one_surface_and_preserves_discussion() {
    for reduced in [false, true] {
        let mut s = app(true, SavedView::in_memory());
        scene_story(&mut s);
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.on_key = Some(on_key);
        rt.set_system_reduced_motion(reduced);
        rt.frame(0.0);
        route(&mut rt, rune::Key::Tab);
        rt.key(KeyEvent { key: rune::Key::Enter, mods: Default::default() });
        rt.frame(0.0);
        rt.frame(0.05);
        let find = |rt: &rune::Runtime<App>, key: &str| rt.elements().into_iter()
            .find(|e| e.key.as_deref() == Some(key) && !e.exiting).unwrap();
        let source_50 = find(&rt, "source-viewer");
        let hidden = find(&rt, "inspector");
        assert_eq!(hidden.opacity, 0.0);
        assert!(hidden.inert);
        rt.frame(0.05);
        let source_100 = find(&rt, "source-viewer");
        if reduced {
            assert_eq!(source_50.opacity, 1.0);
            assert_eq!(source_50.rect, source_50.target);
        } else {
            assert!(source_50.rect[0] > source_100.rect[0] && source_100.rect[0] > source_100.target[0] + 1.0,
                "source should still move at 100ms: {} {} {}", source_50.rect[0], source_100.rect[0], source_100.target[0]);
            assert!(source_50.opacity < source_100.opacity && source_100.opacity < 1.0);
        }
        rt.key(KeyEvent { key: rune::Key::Escape, mods: Default::default() });
        rt.frame(0.0);
        rt.frame(0.05);
        let back_50 = find(&rt, "inspector");
        rt.frame(0.05);
        let back_100 = find(&rt, "inspector");
        assert!(!rt.elements().iter().any(|e| e.key.as_deref() == Some("source-viewer") && e.opacity > 0.001));
        if reduced {
            assert_eq!(back_50.opacity, 1.0);
            assert_eq!(back_50.rect, back_50.target);
        } else {
            assert!(back_50.rect[0] < back_100.rect[0] && back_100.rect[0] < back_100.target[0] - 1.0);
            assert!(back_50.opacity < back_100.opacity && back_100.opacity < 1.0);
        }
    }
}

