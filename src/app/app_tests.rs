use super::*;

fn test_story(id: i32) -> HNCLIItem {
    HNCLIItem {
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
fn test_story_type_conversion() {
    assert_eq!(StoryType::Best.as_str(), "best");
    assert_eq!(StoryType::New.as_str(), "new");
    assert_eq!(StoryType::Top.as_str(), "top");
}

#[test]
fn test_app_navigation() {
    let mut app = App::new();
    app.stories = vec![test_story(1), test_story(2)];

    assert_eq!(app.selected_index, 0);
    app.next_story();
    assert_eq!(app.selected_index, 1);
    app.next_story(); // Should not go beyond bounds
    assert_eq!(app.selected_index, 1);
    app.prev_story();
    assert_eq!(app.selected_index, 0);
}

#[test]
fn test_page_navigation() {
    let mut app = App::new();
    assert_eq!(app.current_page, 1);

    app.next_page();
    assert_eq!(app.current_page, 2);

    app.prev_page();
    assert_eq!(app.current_page, 1);

    app.prev_page(); // Should not go below 1
    assert_eq!(app.current_page, 1);
}

#[test]
fn test_story_type_switch() {
    let mut app = App::new();
    app.selected_index = 5;
    app.current_page = 3;

    app.set_story_type(StoryType::New);
    assert_eq!(app.story_type, StoryType::New);
    assert_eq!(app.selected_index, 0); // Reset
    assert_eq!(app.current_page, 1); // Reset
}

#[test]
fn story_error_is_preserved_while_comments_are_active() {
    let mut app = App::new();
    app.set_stories(vec![test_story(1)]);
    app.view_comments(1, "story".to_string(), "http://example.com".to_string());

    app.set_story_error("story failed".to_string());

    assert!(matches!(app.view, View::Comments { .. }));
    assert!(app.error().is_none());
    assert!(app.is_loading());

    app.view_stories();

    assert_eq!(app.error(), Some("story failed"));
    assert!(!app.is_loading());
}
