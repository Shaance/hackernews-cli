use super::*;

fn test_story(id: i32) -> HNCLIItem {
    HNCLIItem {
        id,
        title: format!("Story {id}"),
        url: "http://example.com".to_string(),
        author: "user".to_string(),
        time: "2023-01-01".to_string(),
        time_ago: "1h ago".to_string(),
        score: 100,
        text: String::new(),
        links: vec![],
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
fn snapshot_starts_on_summary_without_promoting_social_links_or_changing_ranks() {
    use crate::snapshot::{CapturedArticle, Snapshot};
    use std::{collections::BTreeMap, sync::Arc};
    for (host, expected) in [
        ("x.com", 2),
        ("twitter.com", 2),
        ("mobile.twitter.com", 2),
        ("WWW.X.COM", 2),
        ("x.com.", 2),
        ("notx.com", 0),
        ("x.com.example.org", 0),
    ] {
        let mut stories = vec![test_story(1), test_story(2), test_story(3)];
        stories[0].url = format!("https://{host}/story");
        stories[2].url = "https://danluu.com/ambitious/".into();
        let mut snapshot = Snapshot {
            version: 1,
            generated_at: "2026-10-26T00:00:00Z".into(),
            feeds: BTreeMap::new(),
            items: BTreeMap::new(),
            articles: [1, 3]
                .into_iter()
                .map(|id| {
                    (
                        id,
                        CapturedArticle {
                            text: "Captured article".into(),
                            error: None,
                            partial: false,
                            summary: Some("Captured summary".into()),
                        },
                    )
                })
                .collect(),
        };
        let mut app = App::new();
        app.snapshot = Some(Arc::new(snapshot.clone()));
        app.set_stories_for(StoryType::Best, 1, stories.clone());
        assert_eq!(app.selected_index, expected, "{host}");
        assert_eq!(
            app.stories.iter().map(|s| s.id).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        // A visitor can still choose the social story and keep that choice on return.
        app.selected_index = 0;
        app.set_stories_for(StoryType::New, 1, vec![test_story(4)]);
        app.set_stories_for(StoryType::Best, 1, stories.clone());
        assert_eq!(app.selected_index, 0);
        snapshot.articles.clear();
        let mut no_summary = App::new();
        no_summary.snapshot = Some(Arc::new(snapshot));
        no_summary.set_stories_for(StoryType::Best, 1, stories.clone());
        assert_eq!(no_summary.selected_index, if expected == 2 { 1 } else { 0 });
        let mut native = App::new();
        native.set_stories_for(StoryType::Best, 1, stories);
        assert_eq!(native.selected_index, 0);
    }
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
    assert_eq!(app.selected_index, 5); // Keep the displayed selection until success
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
