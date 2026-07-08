use super::*;
use anyhow::anyhow;

fn test_comment(id: i32, state: CommentState) -> hn_lib::app::Comment {
    hn_lib::app::Comment {
        id,
        author: format!("user{}", id),
        text: format!("comment {}", id),
        time_ago: "now".to_string(),
        state,
        depth: 0,
        deleted: false,
        child_ids: vec![id + 100],
    }
}

fn ready_app() -> App {
    let mut app = App::new();
    app.set_stories(Vec::new());
    app
}

#[test]
fn ignores_comments_loaded_for_inactive_story() {
    let mut app = ready_app();
    app.view_comments(2, "active story".to_string(), "https://active".to_string());
    let view_generation = app.comment_view_generation();

    handle_app_message(
        &mut app,
        AppMessage::Comments {
            story_id: 1,
            view_generation,
            result: Ok(vec![test_comment(10, CommentState::Collapsed)]),
        },
    );

    assert!(matches!(app.view, View::Comments { story_id: 2, .. }));
    assert!(app.comments.is_empty());
    assert!(app.is_loading());
    assert!(app.error().is_none());
}

#[test]
fn ignores_comment_errors_after_returning_to_stories() {
    let mut app = ready_app();
    app.view_comments(1, "old story".to_string(), "https://old".to_string());
    let view_generation = app.comment_view_generation();
    app.view_stories();

    handle_app_message(
        &mut app,
        AppMessage::Comments {
            story_id: 1,
            view_generation,
            result: Err(anyhow!("late failure")),
        },
    );

    assert!(matches!(app.view, View::Stories));
    assert!(app.comments.is_empty());
    assert!(!app.is_loading());
    assert!(app.error().is_none());
}

#[test]
fn clears_active_comment_error_when_returning_to_stories() {
    let mut app = ready_app();
    app.view_comments(1, "old story".to_string(), "https://old".to_string());
    let view_generation = app.comment_view_generation();

    handle_app_message(
        &mut app,
        AppMessage::Comments {
            story_id: 1,
            view_generation,
            result: Err(anyhow!("load failed")),
        },
    );

    assert!(app.error().is_some());

    app.view_stories();

    assert!(matches!(app.view, View::Stories));
    assert!(app.error().is_none());
    assert!(!app.is_loading());
}

#[test]
fn preserves_story_error_that_arrives_while_comments_are_active() {
    let mut app = ready_app();
    app.view_comments(1, "old story".to_string(), "https://old".to_string());

    handle_app_message(
        &mut app,
        AppMessage::Stories {
            story_type: StoryType::Best,
            page: 1,
            result: Err(anyhow!("story fetch failed")),
        },
    );

    assert!(matches!(app.view, View::Comments { .. }));
    assert!(app.error().is_none());
    assert!(app.is_loading());

    app.view_stories();

    assert_eq!(
        app.error(),
        Some("Failed to load stories: story fetch failed")
    );
    assert!(!app.is_loading());
}

#[test]
fn applies_comment_children_only_for_active_story() {
    let mut app = ready_app();
    app.view_comments(2, "active story".to_string(), "https://active".to_string());
    let view_generation = app.comment_view_generation();
    app.set_comments(vec![test_comment(10, CommentState::Loading)]);

    handle_app_message(
        &mut app,
        AppMessage::CommentChildren {
            story_id: 1,
            view_generation,
            comment_id: 10,
            result: Ok(vec![test_comment(110, CommentState::Collapsed)]),
        },
    );

    assert!(matches!(app.comments[0].state, CommentState::Loading));
    assert_eq!(app.visible_comment_count(), 1);

    handle_app_message(
        &mut app,
        AppMessage::CommentChildren {
            story_id: 2,
            view_generation,
            comment_id: 10,
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
fn ignores_comment_results_from_previous_visit_to_same_story() {
    let mut app = ready_app();
    app.view_comments(1, "first visit".to_string(), "https://story".to_string());
    let first_generation = app.comment_view_generation();
    app.view_stories();
    app.view_comments(1, "second visit".to_string(), "https://story".to_string());
    let second_generation = app.comment_view_generation();

    handle_app_message(
        &mut app,
        AppMessage::Comments {
            story_id: 1,
            view_generation: first_generation,
            result: Ok(vec![test_comment(10, CommentState::Collapsed)]),
        },
    );

    assert_ne!(first_generation, second_generation);
    assert!(app.comments.is_empty());
    assert!(app.is_loading());

    handle_app_message(
        &mut app,
        AppMessage::Comments {
            story_id: 1,
            view_generation: second_generation,
            result: Ok(vec![test_comment(20, CommentState::Collapsed)]),
        },
    );

    assert_eq!(app.comments.len(), 1);
    assert_eq!(app.comments[0].id, 20);
    assert!(!app.is_loading());
}
