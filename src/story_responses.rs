use super::*;
use anyhow::anyhow;

fn test_story(id: i32) -> hn_lib::HNCLIItem {
    hn_lib::HNCLIItem {
        id,
        title: format!("Story {}", id),
        url: "http://example.com".to_string(),
        author: "user".to_string(),
        time: "2023-01-01".to_string(),
        time_ago: "1h ago".to_string(),
        score: 100,
        comments: Some(10),
    }
}

#[test]
fn ignores_older_story_error_after_newer_success_for_same_page() {
    let mut app = ready_app();
    let older_generation = app.next_story_request_generation();
    let newer_generation = app.next_story_request_generation();

    handle_app_message(
        &mut app,
        AppMessage::Stories {
            story_type: StoryType::Best,
            page: 1,
            request_generation: newer_generation,
            result: Ok(vec![test_story(1)]),
        },
    );

    assert_eq!(app.stories.len(), 1);
    assert_eq!(app.stories[0].id, 1);
    assert!(app.error().is_none());

    handle_app_message(
        &mut app,
        AppMessage::Stories {
            story_type: StoryType::Best,
            page: 1,
            request_generation: older_generation,
            result: Err(anyhow!("older failure")),
        },
    );

    assert_eq!(app.stories.len(), 1);
    assert_eq!(app.stories[0].id, 1);
    assert!(app.error().is_none());
}

#[test]
fn ignores_older_story_success_after_newer_error_for_same_page() {
    let mut app = ready_app();
    let older_generation = app.next_story_request_generation();
    let newer_generation = app.next_story_request_generation();

    handle_app_message(
        &mut app,
        AppMessage::Stories {
            story_type: StoryType::Best,
            page: 1,
            request_generation: newer_generation,
            result: Err(anyhow!("newer failure")),
        },
    );

    assert!(app.stories.is_empty());
    assert_eq!(app.error(), Some("Failed to load stories: newer failure"));

    handle_app_message(
        &mut app,
        AppMessage::Stories {
            story_type: StoryType::Best,
            page: 1,
            request_generation: older_generation,
            result: Ok(vec![test_story(1)]),
        },
    );

    assert!(app.stories.is_empty());
    assert_eq!(app.error(), Some("Failed to load stories: newer failure"));
}

#[test]
fn applies_current_story_success() {
    let mut app = ready_app();
    let request_generation = app.next_story_request_generation();

    handle_app_message(
        &mut app,
        AppMessage::Stories {
            story_type: StoryType::Best,
            page: 1,
            request_generation,
            result: Ok(vec![test_story(1)]),
        },
    );

    assert_eq!(app.stories.len(), 1);
    assert_eq!(app.stories[0].id, 1);
    assert_eq!(app.stories_for, Some((StoryType::Best, 1)));
    assert!(!app.is_loading());
}

#[test]
fn story_page_failure_preserves_existing_visible_stories() {
    let mut app = ready_app();
    app.set_stories_for(StoryType::Best, 1, vec![test_story(1)]);
    app.next_page();
    let request_generation = app.next_story_request_generation();

    handle_app_message(
        &mut app,
        AppMessage::Stories {
            story_type: StoryType::Best,
            page: 2,
            request_generation,
            result: Err(anyhow!("page failed")),
        },
    );

    assert_eq!(app.stories.len(), 1);
    assert_eq!(app.stories[0].id, 1);
    assert_eq!(app.stories_for, Some((StoryType::Best, 1)));
    assert!(app.showing_stale_stories());
    assert_eq!(app.error(), Some("Failed to load stories: page failed"));
    assert!(!app.is_loading());
}
