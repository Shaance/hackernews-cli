use super::*;
use anyhow::anyhow;

#[test]
fn applies_comment_children_only_for_active_story() {
    let mut app = ready_app();
    app.view_comments(2, "active story".to_string(), "https://active".to_string());
    let view_generation = app.comment_view_generation();
    app.set_comments(vec![test_comment(10, loading(1))]);

    handle_app_message(
        &mut app,
        AppMessage::CommentChildren {
            story_id: 1,
            view_generation,
            comment_id: 10,
            child_load_generation: 1,
            result: Ok(vec![test_comment(110, CommentState::Collapsed)]),
        },
    );

    assert!(matches!(
        app.comments[0].state,
        CommentState::Loading { .. }
    ));
    assert_eq!(app.visible_comment_count(), 1);

    handle_app_message(
        &mut app,
        AppMessage::CommentChildren {
            story_id: 2,
            view_generation,
            comment_id: 10,
            child_load_generation: 1,
            result: Ok(vec![test_comment(110, CommentState::Collapsed)]),
        },
    );

    assert!(matches!(
        app.comments[0].state,
        CommentState::Expanded { .. }
    ));
    assert_eq!(app.visible_comment_count(), 2);
}

#[test]
fn ignores_child_success_after_loading_comment_is_collapsed() {
    let mut app = ready_app();
    app.view_comments(1, "active story".to_string(), "https://active".to_string());
    let view_generation = app.comment_view_generation();
    app.set_comments(vec![test_comment(10, loading(1))]);
    app.set_comment_loading(true);

    app.collapse_current_thread();

    assert!(matches!(app.comments[0].state, CommentState::Collapsed));
    assert!(!app.is_loading());

    handle_app_message(
        &mut app,
        AppMessage::CommentChildren {
            story_id: 1,
            view_generation,
            comment_id: 10,
            child_load_generation: 1,
            result: Ok(vec![test_comment(110, CommentState::Collapsed)]),
        },
    );

    assert!(matches!(app.comments[0].state, CommentState::Collapsed));
    assert_eq!(app.visible_comment_count(), 1);
    assert!(app.error().is_none());
    assert!(!app.is_loading());
}

#[test]
fn ignores_child_error_after_loading_comment_is_collapsed() {
    let mut app = ready_app();
    app.view_comments(1, "active story".to_string(), "https://active".to_string());
    let view_generation = app.comment_view_generation();
    app.set_comments(vec![test_comment(10, loading(1))]);
    app.set_comment_loading(true);

    app.collapse_current_thread();

    handle_app_message(
        &mut app,
        AppMessage::CommentChildren {
            story_id: 1,
            view_generation,
            comment_id: 10,
            child_load_generation: 1,
            result: Err(anyhow!("late child failure")),
        },
    );

    assert!(matches!(app.comments[0].state, CommentState::Collapsed));
    assert!(app.error().is_none());
    assert!(!app.is_loading());
}

#[test]
fn retrying_child_load_clears_previous_child_error() {
    let mut app = ready_app();
    app.view_comments(1, "active story".to_string(), "https://active".to_string());
    let view_generation = app.comment_view_generation();
    app.set_comments(vec![test_comment(10, loading(1))]);
    app.set_comment_loading(true);

    handle_app_message(
        &mut app,
        AppMessage::CommentChildren {
            story_id: 1,
            view_generation,
            comment_id: 10,
            child_load_generation: 1,
            result: Err(anyhow!("child failure")),
        },
    );

    assert!(matches!(app.comments[0].state, CommentState::Collapsed));
    assert!(app.error().is_some());

    app.comments[0].state = loading(2);
    app.rebuild_visible_comments();
    app.set_comment_loading(true);

    assert!(app.error().is_none());
    assert!(app.is_loading());

    handle_app_message(
        &mut app,
        AppMessage::CommentChildren {
            story_id: 1,
            view_generation,
            comment_id: 10,
            child_load_generation: 2,
            result: Ok(vec![test_comment(110, CommentState::Collapsed)]),
        },
    );

    assert!(matches!(
        app.comments[0].state,
        CommentState::Expanded { .. }
    ));
    assert_eq!(app.visible_comment_count(), 2);
    assert!(app.error().is_none());
    assert!(!app.is_loading());
}

#[test]
fn ignores_stale_child_response_after_reexpanding_same_comment() {
    let mut app = ready_app();
    app.view_comments(1, "active story".to_string(), "https://active".to_string());
    let view_generation = app.comment_view_generation();
    app.set_comments(vec![test_comment(10, loading(1))]);
    app.set_comment_loading(true);

    app.collapse_current_thread();
    app.comments[0].state = loading(2);
    app.rebuild_visible_comments();
    app.set_comment_loading(true);

    handle_app_message(
        &mut app,
        AppMessage::CommentChildren {
            story_id: 1,
            view_generation,
            comment_id: 10,
            child_load_generation: 1,
            result: Err(anyhow!("stale failure")),
        },
    );

    assert!(matches!(
        app.comments[0].state,
        CommentState::Loading { generation: 2 }
    ));
    assert!(app.error().is_none());
    assert!(app.is_loading());

    handle_app_message(
        &mut app,
        AppMessage::CommentChildren {
            story_id: 1,
            view_generation,
            comment_id: 10,
            child_load_generation: 1,
            result: Ok(vec![test_comment(110, CommentState::Collapsed)]),
        },
    );

    assert!(matches!(
        app.comments[0].state,
        CommentState::Loading { generation: 2 }
    ));
    assert_eq!(app.visible_comment_count(), 1);

    handle_app_message(
        &mut app,
        AppMessage::CommentChildren {
            story_id: 1,
            view_generation,
            comment_id: 10,
            child_load_generation: 2,
            result: Ok(vec![test_comment(110, CommentState::Collapsed)]),
        },
    );

    assert!(matches!(
        app.comments[0].state,
        CommentState::Expanded { .. }
    ));
    assert_eq!(app.visible_comment_count(), 2);
    assert!(app.error().is_none());
    assert!(!app.is_loading());
}

#[test]
fn collapsing_expanded_thread_clears_loading_from_removed_descendant() {
    let mut app = ready_app();
    app.view_comments(1, "active story".to_string(), "https://active".to_string());
    app.set_comments(vec![test_comment(
        10,
        expanded(vec![test_comment(110, loading(1))]),
    )]);
    app.set_comment_loading(true);

    app.collapse_current_thread();

    assert!(matches!(app.comments[0].state, CommentState::Collapsed));
    assert_eq!(app.visible_comment_count(), 1);
    assert!(!app.is_loading());
}

#[test]
fn child_error_remains_visible_when_another_child_load_is_pending() {
    let mut app = ready_app();
    app.view_comments(1, "active story".to_string(), "https://active".to_string());
    let view_generation = app.comment_view_generation();
    app.set_comments(vec![
        test_comment(10, loading(1)),
        test_comment(20, loading(2)),
    ]);
    app.set_comment_loading(true);

    handle_app_message(
        &mut app,
        AppMessage::CommentChildren {
            story_id: 1,
            view_generation,
            comment_id: 10,
            child_load_generation: 1,
            result: Err(anyhow!("first child failed")),
        },
    );

    assert!(matches!(app.comments[0].state, CommentState::Collapsed));
    assert!(matches!(
        app.comments[1].state,
        CommentState::Loading { generation: 2 }
    ));
    assert_eq!(
        app.error(),
        Some("Failed to load comment children: first child failed")
    );
    assert!(app.is_loading());
}

#[test]
fn successful_child_load_preserves_existing_error_while_another_child_load_is_pending() {
    let mut app = ready_app();
    app.view_comments(1, "active story".to_string(), "https://active".to_string());
    let view_generation = app.comment_view_generation();
    app.set_comments(vec![
        test_comment(10, loading(1)),
        test_comment(20, loading(2)),
        test_comment(30, loading(3)),
    ]);
    app.set_comment_error("previous child failed".to_string());
    app.recompute_comment_loading(true);

    handle_app_message(
        &mut app,
        AppMessage::CommentChildren {
            story_id: 1,
            view_generation,
            comment_id: 10,
            child_load_generation: 1,
            result: Ok(vec![test_comment(110, CommentState::Collapsed)]),
        },
    );

    assert!(matches!(
        app.comments[0].state,
        CommentState::Expanded { .. }
    ));
    assert_eq!(app.error(), Some("previous child failed"));
    assert!(app.is_loading());
}

#[test]
fn collapse_recompute_preserves_existing_error_while_another_child_load_is_pending() {
    let mut app = ready_app();
    app.view_comments(1, "active story".to_string(), "https://active".to_string());
    app.set_comments(vec![
        test_comment(10, expanded(vec![test_comment(110, loading(1))])),
        test_comment(20, loading(2)),
    ]);
    app.set_comment_error("previous child failed".to_string());
    app.recompute_comment_loading(true);

    app.collapse_current_thread();

    assert!(matches!(app.comments[0].state, CommentState::Collapsed));
    assert_eq!(app.error(), Some("previous child failed"));
    assert!(app.is_loading());
}
