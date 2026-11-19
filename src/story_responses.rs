use super::*;
use anyhow::anyhow;

fn test_story(id: i32) -> crate::HNCLIItem {
    crate::HNCLIItem {
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

#[tokio::test]
async fn wide_feed_arrows_move_to_the_same_row_and_clamp() {
    let mut app = ready_app();
    app.set_stories_for(StoryType::Best, 1, (1..=5).map(test_story).collect());
    let (tx, mut rx) = mpsc::unbounded_channel();
    let service = HackerNewsCliServiceImpl::new();
    app.jump_selection(2);
    for (key, selected) in [(KeyCode::Right, 5), (KeyCode::Left, 2), (KeyCode::Right, 5)] {
        handle_feed_key(&mut app, key, 160, tx.clone(), service.clone(), &mut None)
            .await
            .unwrap();
        assert_eq!(app.selected_story().unwrap().id, selected);
        assert_eq!(app.current_page, 1);
    }
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn wide_feed_edges_use_cached_pages_and_land_at_start_and_end() {
    let mut app = ready_app();
    app.apply_stories_page(StoryType::Best, 2, (21..=40).map(test_story).collect());
    app.apply_stories_page(StoryType::Best, 1, (1..=20).map(test_story).collect());
    app.jump_selection(10);
    let (tx, _) = mpsc::unbounded_channel();
    let service = HackerNewsCliServiceImpl::new();
    app.set_story_loading(true);
    handle_feed_key(
        &mut app,
        KeyCode::Right,
        160,
        tx.clone(),
        service.clone(),
        &mut None,
    )
    .await
    .unwrap();
    assert_eq!(app.current_page, 1);
    assert!(app.page_landing.is_none());

    app.set_story_error("failed refresh".into());
    app.next_page(); // The visible page is stale once a page change is pending.
    handle_feed_key(
        &mut app,
        KeyCode::Right,
        160,
        tx.clone(),
        service.clone(),
        &mut None,
    )
    .await
    .unwrap();
    assert_eq!(app.current_page, 2);
    assert_eq!(app.stories_for, Some((StoryType::Best, 1)));

    app.prev_page();
    for (key, page, selected) in [(KeyCode::Right, 2, 21), (KeyCode::Left, 1, 20)] {
        handle_feed_key(&mut app, key, 160, tx.clone(), service.clone(), &mut None)
            .await
            .unwrap();
        assert_eq!(app.stories_for, Some((StoryType::Best, page)));
        assert_eq!(app.selected_story().unwrap().id, selected);
        assert!(app.page_landing.is_none());
    }
}

#[tokio::test]
async fn narrow_arrows_and_manual_page_keys_keep_their_paging() {
    let mut app = ready_app();
    let (tx, _) = mpsc::unbounded_channel();
    let service = HackerNewsCliServiceImpl::new();
    app.apply_stories_page(StoryType::Best, 1, (1..=20).map(test_story).collect());
    app.apply_stories_page(StoryType::Best, 2, (21..=40).map(test_story).collect());
    for (width, forward, back) in [
        (159, KeyCode::Right, KeyCode::Left),
        (160, KeyCode::Char('n'), KeyCode::Char('p')),
        (160, KeyCode::Char(' '), KeyCode::PageUp),
        (160, KeyCode::PageDown, KeyCode::PageUp),
    ] {
        app.jump_selection(2);
        for (key, page, selected) in [(forward, 2, 21), (back, 1, 1)] {
            handle_feed_key(&mut app, key, width, tx.clone(), service.clone(), &mut None)
                .await
                .unwrap();
            assert_eq!(app.stories_for, Some((StoryType::Best, page)));
            assert_eq!(app.selected_story().unwrap().id, selected);
            assert!(app.page_landing.is_none());
        }
    }
    for key in [KeyCode::Char('h'), KeyCode::Char('l')] {
        handle_feed_key(&mut app, key, 160, tx.clone(), service.clone(), &mut None)
            .await
            .unwrap();
        assert_eq!(app.selected_story().unwrap().id, 1);
    }
}

#[tokio::test]
async fn wide_saved_arrows_and_removal_work_beyond_twenty_bookmarks() {
    let mut app = ready_app();
    app.set_story_type(StoryType::Saved);
    app.bookmarks = (1..=23).map(test_story).collect();
    app.show_saved_stories();
    let mut path =
        Some(std::env::temp_dir().join(format!("hn-wide-saved-{}.json", std::process::id())));
    let (tx, _) = mpsc::unbounded_channel();
    let service = HackerNewsCliServiceImpl::new();
    app.jump_selection(11);
    for (key, selected) in [
        (KeyCode::Left, 12),
        (KeyCode::Right, 23),
        (KeyCode::Right, 23),
        (KeyCode::Char('s'), 22),
        (KeyCode::Left, 11),
    ] {
        handle_feed_key(&mut app, key, 160, tx.clone(), service.clone(), &mut path)
            .await
            .unwrap();
        assert_eq!(app.selected_story().unwrap().id, selected);
        assert_eq!(app.stories_for, Some((StoryType::Saved, 1)));
    }
    assert_eq!(app.stories.len(), 22);
    assert!(!app.is_bookmarked(23));
    std::fs::remove_file(path.unwrap()).unwrap();
}
