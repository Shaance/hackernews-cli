use super::*;

mod child_responses;
mod story_responses;
mod view_scope;

pub(super) fn test_comment(id: i32, state: CommentState) -> hn_lib::app::Comment {
    hn_lib::app::Comment {
        id,
        author: format!("user{}", id),
        text: format!("comment {}", id),
        links: vec![],
        time_ago: "now".to_string(),
        state,
        depth: 0,
        deleted: false,
        child_ids: vec![id + 100],
    }
}

pub(super) fn loading(generation: u64) -> CommentState {
    CommentState::Loading { generation }
}

pub(super) fn expanded(children: Vec<hn_lib::app::Comment>) -> CommentState {
    CommentState::Expanded { children }
}

pub(super) fn ready_app() -> App {
    let mut app = App::new();
    app.set_stories(Vec::new());
    app
}

#[tokio::test]
async fn reader_keys_page_text_and_link_picker_can_be_closed() {
    let mut app = ready_app();
    app.view_comments(1, "story".into(), "https://example.com".into());
    let mut comment = test_comment(10, CommentState::Collapsed);
    comment.links = vec![
        "https://example.com/first".into(),
        "https://example.com/second".into(),
    ];
    app.set_comments(vec![comment]);
    let (tx, _) = mpsc::unbounded_channel();
    let service = HackerNewsCliServiceImpl::new();
    handle_comment_action(
        &mut app,
        handle_comments_key(KeyCode::PageDown),
        tx.clone(),
        service.clone(),
        None,
    )
    .await
    .unwrap();
    app.update_comment_scroll(&[(0, 60)], 19);
    assert_eq!(app.comment_scroll, 18);
    handle_comment_action(
        &mut app,
        handle_comments_key(KeyCode::Char('L')),
        tx,
        service,
        None,
    )
    .await
    .unwrap();
    assert!(app.show_links);
    handle_link_key(&mut app, KeyCode::Char('j'));
    assert_eq!(app.link_cursor, 1);
    handle_link_key(&mut app, KeyCode::Esc);
    assert!(!app.show_links);
    assert_eq!(app.comment_scroll, 18);
}
