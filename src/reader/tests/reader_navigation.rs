use super::*;

#[test]
fn pinned_summary_keeps_comment_anchor_and_source_back_at_all_sizes() {
    for (w, h) in [(40, 12), (80, 24), (120, 40), (160, 50)] {
        let mut p = ready();
        let mut app = comments_app();
        p.browser_notice = "Article open in split pane".into();
        let initial = screen(&mut p, &mut app, w, h);
        if w >= 120 {
            assert!(initial.contains("Tab Sources · Enter Open"));
            assert!(initial.contains("Article open in split pane"));
        }
        assert!(initial.find('╰').unwrap() < initial.find("DISCUSSION · 3 visible").unwrap());
        app.set_comment_position(1, p.readings[&1].prefix_height + 37);
        let anchor = (app.comment_cursor, app.comment_scroll);
        let text = screen(&mut p, &mut app, w, h);
        assert!(text.contains("ARTICLE"));
        assert!(text.contains('╰'));
        p.key(KeyCode::Char('S'));
        screen(&mut p, &mut app, w, h);
        assert_eq!((app.comment_cursor, app.comment_scroll), anchor);
        p.key(KeyCode::Char('S'));
        p.key(KeyCode::Tab);
        p.key(KeyCode::Enter);
        assert!(screen(&mut p, &mut app, w, h).contains("Reply evidence."));
        p.key(KeyCode::Esc);
        screen(&mut p, &mut app, w, h);
        assert_eq!((app.comment_cursor, app.comment_scroll), anchor);
        assert!(!p.key(KeyCode::Char('j')));
        app.next_comment();
        assert_eq!(app.comment_cursor, 2);
    }
}

#[test]
fn short_summary_returns_space_and_long_summary_scrolls_independently() {
    let mut p = ready();
    let mut app = comments_app();
    let short = screen(&mut p, &mut app, 120, 40);
    assert!(!short.contains("Partial source"));
    let short_end = short.find('╰').unwrap();
    p.readings
        .get_mut(&1)
        .unwrap()
        .article
        .result
        .as_mut()
        .unwrap()
        .value
        .text = (0..40).map(|i| format!("Sentence {i}.\n")).collect();
    let full = screen(&mut p, &mut app, 120, 40);
    assert!(full.find('╰').unwrap() > short_end);
    assert!(full.contains("J/K Summary"));
    let anchor = (app.comment_cursor, app.comment_scroll);
    p.key(KeyCode::Char('J'));
    assert_ne!(screen(&mut p, &mut app, 120, 40), full);
    assert_eq!((app.comment_cursor, app.comment_scroll), anchor);

    p = ready();
    p.readings
        .get_mut(&1)
        .unwrap()
        .article
        .result
        .as_mut()
        .unwrap()
        .capture = Arc::new(Capture::new(ArticleCapture {
        title: "An example story".into(),
        text: "Opening portion of the article".into(),
        partial: true,
    }));
    assert!(screen(&mut p, &mut app, 120, 40).contains("Partial source · opening portion only"));
}

#[tokio::test]
async fn coordinator_reopens_fresh_metadata_without_moving_comments_or_evidence() {
    let mut p = ready();
    let mut app = comments_app();
    app.set_stories(vec![story(1)]);
    let (tx, _rx) = mpsc::unbounded_channel();
    let service = hn_lib::HackerNewsCliServiceImpl::new();
    let mut bookmarks = None;
    for key in [KeyCode::Esc, KeyCode::Char('c'), KeyCode::PageDown] {
        assert!(crate::reader_actions::handle(
            &mut p,
            &mut app,
            key,
            tx.clone(),
            service.clone(),
            &mut bookmarks
        )
        .await
        .unwrap());
        if p.is_open() {
            screen(&mut p, &mut app, 120, 40);
        }
    }
    let anchor = (app.comment_cursor, app.comment_scroll);
    assert!(anchor.1 > 0);
    crate::reader_actions::handle(
        &mut p,
        &mut app,
        KeyCode::Esc,
        tx.clone(),
        service.clone(),
        &mut bookmarks,
    )
    .await
    .unwrap();
    let mut updated = story(1);
    updated.title = "Fresh feed metadata".into();
    updated.score = 99;
    app.set_stories(vec![updated]);
    for key in [
        KeyCode::Char('c'),
        KeyCode::Char('S'),
        KeyCode::Char('S'),
        KeyCode::Tab,
        KeyCode::Enter,
        KeyCode::Esc,
    ] {
        assert!(crate::reader_actions::handle(
            &mut p,
            &mut app,
            key,
            tx.clone(),
            service.clone(),
            &mut bookmarks
        )
        .await
        .unwrap());
        let rendered = screen(&mut p, &mut app, 120, 40);
        assert!(rendered.contains("Fresh feed metadata"));
        assert!(rendered.contains("99 points"));
        if key == KeyCode::Enter {
            assert!(rendered.contains("Reply evidence."));
        }
    }
    assert_eq!((app.comment_cursor, app.comment_scroll), anchor);
    assert_eq!(
        p.readings[&1]
            .article
            .result
            .as_ref()
            .unwrap()
            .capture
            .input
            .title,
        "An example story"
    );
    assert!(p.jobs.is_empty());
}
