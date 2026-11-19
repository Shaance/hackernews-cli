use super::{
    super::tests::{comments_app, ready, screen},
    *,
};
use crate::{
    app::Comment,
    tui::{handle_app_message, AppMessage},
};

fn comment(id: i32, author: &str, child_ids: Vec<i32>) -> Comment {
    Comment {
        id,
        author: author.into(),
        text: format!("Text from {author}."),
        links: vec![],
        time_ago: "now".into(),
        deleted: false,
        child_ids,
    }
}

fn open(comments: Vec<Comment>) -> (Reader, App) {
    let mut reader = ready();
    reader
        .readings
        .get_mut(&1)
        .unwrap()
        .discussion
        .result
        .as_mut()
        .unwrap()
        .value
        .points[0]
        .sources
        .push(10);
    let mut app = comments_app();
    app.set_comments(comments);
    reader.key(KeyCode::Tab);
    reader.key(KeyCode::Enter);
    assert_eq!(reader.source_id(), Some(11));
    (reader, app)
}

fn source(reader: &Reader) -> &SourceView {
    reader.readings[&1].source.as_ref().unwrap()
}

fn complete(
    app: &mut App,
    view_generation: u64,
    (comment_id, child_load_generation, _): (i32, u64, Vec<i32>),
    result: anyhow::Result<Vec<Comment>>,
) -> Option<(i32, u64, Vec<i32>)> {
    handle_app_message(
        app,
        AppMessage::CommentChildren {
            story_id: 1,
            view_generation,
            comment_id,
            child_load_generation,
            result,
        },
    )
}

#[test]
fn shared_replies_keep_source_folds_focus_and_cursor_separate() {
    let (mut reader, mut app) = open(vec![
        comment(11, "cited", vec![12]),
        comment(20, "other", vec![]),
    ]);
    app.set_comment_position(20, 4);
    let request = app.request_citation(11, false).unwrap();
    assert_eq!(request.2, [12]);
    let generation = app.comment_view_generation();
    complete(
        &mut app,
        generation,
        request,
        Ok(vec![comment(12, "child", vec![])]),
    );
    reader.source_key(&mut app, KeyCode::Char('j'));
    assert_eq!(source(&reader).nav.cursor, Some(12));
    reader.source_key(&mut app, KeyCode::Char('c'));
    reader.source_key(&mut app, KeyCode::Char('f'));
    assert!(source(&reader).nav.folded.contains(&11));
    assert!(!source(&reader).nav.focus);
    assert!(!app.thread.is_folded(11));
    assert!(!app.thread.focus());
    assert_eq!((app.thread.cursor(), app.comment_scroll), (Some(20), 4));
    assert!(app.thread.rows().iter().any(|row| row.id == 12));
    assert!(app.request_citation(11, false).is_none());
    reader.key(KeyCode::Esc);
    assert!(reader.readings[&1].citation_focus);
}

#[test]
fn completion_after_switch_or_close_populates_cache_without_moving_either_view() {
    let (mut reader, mut app) = open(vec![
        comment(11, "cited", vec![12]),
        comment(10, "other citation", vec![13]),
    ]);
    app.set_comment_position(10, 4);
    let generation = app.comment_view_generation();
    let first = app.request_citation(11, false).unwrap();
    reader.key(KeyCode::Tab);
    let second = app.request_citation(10, false).unwrap();
    complete(
        &mut app,
        generation,
        first,
        Ok(vec![comment(12, "first reply", vec![])]),
    );
    assert_eq!(source(&reader).nav.cursor, Some(10));
    assert_eq!(source(&reader).scroll, 0);
    assert_eq!((app.thread.cursor(), app.comment_scroll), (Some(10), 4));
    reader.key(KeyCode::Esc);
    complete(
        &mut app,
        generation,
        second,
        Ok(vec![comment(13, "second reply", vec![])]),
    );
    assert_eq!(reader.source_id(), None);
    assert_eq!((app.thread.cursor(), app.comment_scroll), (Some(10), 4));
    assert_eq!(app.thread.children_of(11), [12]);
    assert_eq!(app.thread.children_of(10), [13]);
}

#[test]
fn refresh_and_story_exit_discard_in_flight_citation_requests() {
    let (_, mut app) = open(vec![comment(11, "cited", vec![12])]);
    let generation = app.comment_view_generation();
    let replies = app.request_citation(11, false).unwrap();
    let missing = app.request_citation(10, false).unwrap();
    app.begin_comment_refresh();
    app.set_comments(vec![comment(11, "cited", vec![12])]);
    complete(
        &mut app,
        generation,
        replies,
        Ok(vec![comment(12, "stale", vec![])]),
    );
    complete(
        &mut app,
        generation,
        missing,
        Ok(vec![comment(10, "stale", vec![])]),
    );
    assert!(app.thread.node(12).is_none() && app.thread.node(10).is_none());
    let generation = app.comment_view_generation();
    let pending = app.request_citation(10, false).unwrap();
    app.view_stories();
    app.view_comments(1, "An example story".into(), String::new());
    complete(
        &mut app,
        generation,
        pending,
        Ok(vec![comment(10, "stale", vec![])]),
    );
    assert!(app.thread.node(10).is_none());
}

#[test]
fn detached_citation_and_its_replies_attach_when_discussion_opens_parent() {
    let (mut reader, mut app) = open(vec![comment(10, "parent", vec![11])]);
    let generation = app.comment_view_generation();
    let lookup = app.request_citation(11, false).unwrap();
    assert_eq!(lookup.2, [11]);
    let replies = complete(
        &mut app,
        generation,
        lookup,
        Ok(vec![comment(11, "cited", vec![12])]),
    )
    .unwrap();
    assert_eq!(replies.2, [12]);
    complete(
        &mut app,
        generation,
        replies,
        Ok(vec![comment(12, "nested", vec![])]),
    );
    assert_eq!(app.visible_comment_count(), 1);
    assert!(screen(&mut reader, &mut app, 120, 40).contains("Text from nested."));
    assert!(app.toggle_comment_replies().is_none());
    assert_eq!(app.thread.children_of(10), [11]);
    assert_eq!(app.thread.children_of(11), [12]);
    assert_eq!(app.thread.tree_depth(12), Some(2));
}

#[test]
fn unavailable_cited_comment_keeps_capture_and_enter_retries() {
    let (mut reader, mut app) = open(vec![comment(10, "parent", vec![11])]);
    let generation = app.comment_view_generation();
    let lookup = app.request_citation(11, false).unwrap();
    assert!(complete(
        &mut app,
        generation,
        lookup,
        Err(anyhow::anyhow!("Item not captured"))
    )
    .is_none());
    let screen_before_retry = screen(&mut reader, &mut app, 120, 40);
    assert!(screen_before_retry.contains("Reply evidence."));
    assert!(screen_before_retry.contains("Cited comment unavailable · Enter retry"));
    assert!(app.request_citation(11, false).is_none());
    let retry = reader.source_key(&mut app, KeyCode::Enter).unwrap();
    assert_eq!(retry.2, [11]);
    complete(
        &mut app,
        generation,
        retry,
        Ok(vec![comment(11, "cited", vec![])]),
    );
    assert!(app.thread.node(11).is_some());
    assert!(!screen(&mut reader, &mut app, 120, 40).contains("Enter retry"));
}

#[test]
fn source_continue_and_backspace_never_go_above_cited_comment() {
    let (mut reader, mut app) = open(vec![
        comment(10, "outside source", vec![11]),
        comment(11, "cited", vec![12]),
        comment(12, "child", vec![13]),
        comment(13, "grandchild", vec![]),
    ]);
    screen(&mut reader, &mut app, 64, 32);
    reader.source_key(&mut app, KeyCode::Char('j'));
    reader.source_key(&mut app, KeyCode::Char('o'));
    assert_eq!(source(&reader).nav.root, Some(12));
    let text = screen(&mut reader, &mut app, 64, 32);
    assert!(text.contains("cited › child") && text.contains("grandchild"));
    assert!(!text.contains("outside source"));
    reader.source_key(&mut app, KeyCode::Backspace);
    reader.source_key(&mut app, KeyCode::Backspace);
    assert_eq!(source(&reader).nav.root, Some(11));
    assert_eq!(source(&reader).nav.cursor, Some(11));
    assert_eq!(app.thread.root(), None);
    assert_eq!(app.thread.cursor(), Some(10));
}

#[test]
fn narrowing_keeps_the_nearest_visible_ancestor_of_the_source_cursor() {
    let (mut reader, mut app) = open(vec![
        comment(10, "outside source", vec![11]),
        comment(11, "cited", vec![12]),
        comment(12, "child", vec![13]),
        comment(13, "grandchild", vec![14]),
        comment(14, "great grandchild", vec![]),
    ]);
    screen(&mut reader, &mut app, 160, 40);
    for _ in 0..3 {
        reader.source_key(&mut app, KeyCode::Char('j'));
    }
    assert_eq!(source(&reader).nav.cursor, Some(14));
    // 64 columns leave room for two levels: the cited comment and its child.
    screen(&mut reader, &mut app, 64, 32);
    assert_eq!(source(&reader).nav.cursor, Some(12));
}
