use super::*;
use crate::app::Comment;
use anyhow::anyhow;

fn discussion(records: &[i32]) -> App {
    let mut app = ready_app();
    app.view_comments(1, "active story".into(), "https://active".into());
    app.set_comments(records.iter().copied().map(test_comment).collect());
    app
}

fn start(app: &mut App, parent: i32) -> u64 {
    assert!(app.thread.select(parent));
    let (id, generation, ids) = app.toggle_comment_replies().unwrap();
    assert_eq!((id, ids), (parent, vec![parent + 100]));
    generation
}

fn response(app: &mut App, parent: i32, generation: u64, result: Result<Vec<Comment>>) {
    handle_app_message(
        app,
        AppMessage::CommentChildren {
            story_id: 1,
            view_generation: app.comment_view_generation(),
            comment_id: parent,
            child_load_generation: generation,
            result,
        },
    );
}

#[test]
fn applies_comment_children_only_for_active_story() {
    let mut app = discussion(&[10]);
    let attempt = start(&mut app, 10);
    let view_generation = app.comment_view_generation();
    handle_app_message(
        &mut app,
        AppMessage::CommentChildren {
            story_id: 2,
            view_generation,
            comment_id: 10,
            child_load_generation: attempt,
            result: Ok(vec![test_comment(110)]),
        },
    );
    assert!(app.comment_replies_pending(10));
    assert_eq!(app.visible_comment_count(), 1);
    response(&mut app, 10, attempt, Ok(vec![test_comment(110)]));
    assert!(app.comment_replies_open(10));
    assert_eq!(app.visible_comment_count(), 2);
}

#[test]
fn ignores_child_results_after_loading_comment_is_collapsed() {
    for result in [Ok(vec![test_comment(110)]), Err(anyhow!("late failure"))] {
        let mut app = discussion(&[10]);
        let attempt = start(&mut app, 10);
        app.collapse_current_thread();
        assert!(!app.comment_replies_pending(10));
        assert!(!app.is_loading());
        response(&mut app, 10, attempt, result);
        assert!(!app.comment_replies_open(10));
        assert_eq!(app.visible_comment_count(), 1);
        assert!(app.error().is_none());
        assert!(!app.is_loading());
    }
}

#[test]
fn retrying_child_load_clears_previous_child_error() {
    let mut app = discussion(&[10]);
    let attempt = start(&mut app, 10);
    response(&mut app, 10, attempt, Err(anyhow!("child failure")));
    assert!(!app.comment_replies_open(10));
    assert!(app.error().is_some());

    let attempt = start(&mut app, 10);
    assert!(app.error().is_none());
    assert!(app.is_loading());
    response(&mut app, 10, attempt, Ok(vec![test_comment(110)]));
    assert!(app.comment_replies_open(10));
    assert_eq!(app.visible_comment_count(), 2);
    assert!(app.error().is_none());
    assert!(!app.is_loading());
}

#[test]
fn ignores_stale_child_response_after_reexpanding_same_comment() {
    let mut app = discussion(&[10]);
    let first = start(&mut app, 10);
    app.collapse_current_thread();
    let second = start(&mut app, 10);
    assert_ne!(first, second);
    for result in [Err(anyhow!("stale failure")), Ok(vec![test_comment(110)])] {
        response(&mut app, 10, first, result);
        assert!(app.comment_replies_pending(10));
        assert!(app.error().is_none());
        assert!(app.is_loading());
        assert_eq!(app.visible_comment_count(), 1);
    }
    response(&mut app, 10, second, Ok(vec![test_comment(110)]));
    assert!(app.comment_replies_open(10));
    assert_eq!(app.visible_comment_count(), 2);
    assert!(app.error().is_none());
    assert!(!app.is_loading());
}

#[test]
fn collapsing_expanded_thread_clears_loading_from_hidden_descendant() {
    let mut app = discussion(&[10, 110]);
    let attempt = start(&mut app, 110);
    app.parent_comment();
    app.collapse_current_thread();
    assert!(app.thread.is_folded(10));
    assert_eq!(app.visible_comment_count(), 1);
    assert!(!app.is_loading());

    response(&mut app, 110, attempt, Ok(vec![test_comment(210)]));
    assert!(app.toggle_comment_replies().is_none());
    assert_eq!(app.visible_comment_count(), 2);
    assert!(app.thread.node(210).is_none());
    start(&mut app, 110);
}

#[test]
fn child_error_remains_visible_when_another_child_load_is_pending() {
    let mut app = discussion(&[10, 20]);
    let first = start(&mut app, 10);
    start(&mut app, 20);
    response(&mut app, 10, first, Err(anyhow!("first child failed")));

    assert!(!app.comment_replies_open(10));
    assert!(app.comment_replies_pending(20));
    assert_eq!(
        app.error(),
        Some("Failed to load comment children: first child failed")
    );
    assert!(app.is_loading());
}

#[test]
fn successful_child_load_preserves_existing_error_while_another_child_load_is_pending() {
    let mut app = discussion(&[10, 20, 30]);
    let first = start(&mut app, 10);
    let second = start(&mut app, 20);
    start(&mut app, 30);
    response(&mut app, 20, second, Err(anyhow!("previous child failed")));
    response(&mut app, 10, first, Ok(vec![test_comment(110)]));

    assert!(app.comment_replies_open(10));
    assert_eq!(
        app.error(),
        Some("Failed to load comment children: previous child failed")
    );
    assert!(app.is_loading());
}

#[test]
fn collapse_recompute_preserves_existing_error_while_another_child_load_is_pending() {
    let mut app = discussion(&[10, 110, 20, 30]);
    start(&mut app, 110);
    start(&mut app, 20);
    let failing = start(&mut app, 30);
    response(&mut app, 30, failing, Err(anyhow!("previous child failed")));
    assert!(app.thread.select(10));
    app.collapse_current_thread();

    assert!(app.thread.is_folded(10));
    assert_eq!(
        app.error(),
        Some("Failed to load comment children: previous child failed")
    );
    assert!(app.is_loading());
}

#[test]
fn concurrent_child_loads_complete_independently_in_either_order() {
    for order in [[10, 20], [20, 10]] {
        let mut app = discussion(&[10, 20]);
        let attempts = [start(&mut app, 10), start(&mut app, 20)];
        for (index, parent) in order.into_iter().enumerate() {
            response(
                &mut app,
                parent,
                attempts[(parent / 10 - 1) as usize],
                Ok(vec![test_comment(parent + 100)]),
            );
            assert!(app.comment_replies_open(parent));
            assert_eq!(app.visible_comment_count(), 3 + index);
            assert_eq!(app.is_loading(), index == 0);
        }
    }
}

#[test]
fn cached_root_reopens_nested_expansion_without_request() {
    let mut app = discussion(&[10, 110, 210]);
    assert!(app.toggle_comment_replies().is_none());
    assert_eq!(app.visible_comment_count(), 1);
    assert!(app.toggle_comment_replies().is_none());
    assert_eq!(
        app.thread
            .rows()
            .iter()
            .map(|row| (row.id, row.depth))
            .collect::<Vec<_>>(),
        vec![(10, 0), (110, 1), (210, 2)]
    );
    assert!(!app.is_loading());
}
