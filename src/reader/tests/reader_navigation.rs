use super::*;

#[test]
fn pinned_summary_keeps_comment_anchor_and_source_back_at_all_sizes() {
    for (w, h) in [
        (40, 12),
        (80, 24),
        (120, 40),
        (160, 50),
        (200, 60),
        (240, 80),
    ] {
        let mut p = ready();
        let mut app = comments_app();
        p.browser_notice = "Article open in split pane".into();
        let initial = screen(&mut p, &mut app, w, h);
        if w >= 120 {
            assert!(initial.contains("Tab sources"));
            assert!(initial.contains("Article open in split pane"));
        }
        assert!(initial.find("ARTICLE").unwrap() < initial.find("COMMENTS  3").unwrap());
        app.set_comment_position(1, p.readings[&1].prefix_height + 37);
        let anchor = (app.comment_cursor, app.comment_scroll);
        let text = screen(&mut p, &mut app, w, h);
        assert!(text.contains("ARTICLE"));
        assert!(text.contains("│ "));
        p.key(KeyCode::Char('S'));
        screen(&mut p, &mut app, w, h);
        assert_eq!((app.comment_cursor, app.comment_scroll), anchor);
        p.key(KeyCode::Char('S'));
        p.key(KeyCode::Tab);
        p.key(KeyCode::Enter);
        assert!(screen(&mut p, &mut app, w, h).contains("Reply evidence."));
        p.key(KeyCode::Char('S'));
        assert!(p.readings[&1].source.is_none() && !p.readings[&1].shown);
        screen(&mut p, &mut app, w, h);
        p.key(KeyCode::Char('S'));
        assert!(p.readings[&1].shown);
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
    let short_end = short.find("COMMENTS  3").unwrap();
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
    assert!(full.find("COMMENTS  3").unwrap() > short_end);
    assert!(full.contains("J/K scroll"));
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
    p.config.api_key.clear();
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

#[test]
fn focused_citation_token_is_visible_after_wrapping_and_source_comparison() {
    use ratatui::style::Modifier;
    for (w, h) in [(80, 24), (120, 40), (200, 60), (240, 80)] {
        let mut p = ready();
        let r = p.readings.get_mut(&1).unwrap();
        let result = r.discussion.result.as_mut().unwrap();
        result.value.points[0].text = "A long observation with Unicode café and 界.\n".repeat(70);
        result.value.points[0].sources = vec![11, 10, 12, 11];
        let mut capture = discussion_capture();
        capture.sources.push(Source {
            id: 12,
            parent: None,
            author: "third".into(),
            text: "Third evidence.\n".repeat(60),
            excerpt: "Third evidence.\n".repeat(60),
        });
        result.capture = Arc::new(Capture::new(capture));
        let mut app = comments_app();
        screen(&mut p, &mut app, w, h);
        app.set_comment_position(1, p.readings[&1].prefix_height + 37);
        let anchor = (app.comment_cursor, app.comment_scroll);
        for n in 1..=3 {
            p.key(KeyCode::Tab);
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|f| p.render(f, &mut app, 0)).unwrap();
            let focused: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .filter(|c| {
                    c.fg == Theme::TRUE_DARK.accent.fg.unwrap()
                        && c.modifier.contains(Modifier::BOLD | Modifier::UNDERLINED)
                })
                .map(|c| c.symbol())
                .collect();
            assert_eq!(focused, format!("[{n}]"));
            let (lines, target) = p.readings[&1].summary_lines(usize::from(w));
            assert!(lines[target.unwrap()].spans.iter().any(|span| span.content
                == format!("[{n}]")
                && span
                    .style
                    .add_modifier
                    .contains(Modifier::BOLD | Modifier::UNDERLINED)));
        }
        p.key(KeyCode::Enter);
        assert!(screen(&mut p, &mut app, w, h).contains("source 3 of 3 · third · Fast"));
        p.key(KeyCode::PageDown);
        assert!(p.readings[&1].source_scroll > 0);
        p.key(KeyCode::Tab);
        assert_eq!(p.readings[&1].source_scroll, 0);
        assert!(screen(&mut p, &mut app, w, h).contains("source 1 of 3 · reply · Fast"));
        p.key(KeyCode::BackTab);
        assert!(screen(&mut p, &mut app, w, h).contains("Third evidence."));
        p.key(KeyCode::Esc);
        assert!(screen(&mut p, &mut app, w, h).contains("[3]"));
        assert_eq!(p.readings[&1].citation, 2);
        assert_eq!((app.comment_cursor, app.comment_scroll), anchor);
        assert!(p.jobs.is_empty());
    }
}

fn buffer(p: &mut Reader, app: &mut hn_lib::app::App) -> ratatui::buffer::Buffer {
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    // Terminal output omits continuation cells hidden by wide glyphs; inspect the frame.
    terminal
        .draw(|f| p.render(f, app, 0))
        .unwrap()
        .buffer
        .clone()
}

fn row(buffer: &ratatui::buffer::Buffer, y: u16) -> String {
    (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol())
        .collect()
}

fn pending_reader() -> Reader {
    let mut p = ready();
    p.open(story(1));
    let r = p.readings.get_mut(&1).unwrap();
    r.article.result = None;
    r.discussion.result = None;
    let attempt = Attempt {
        id: 42,
        profile: Profile::Fast,
        automatic: false,
        refresh: false,
    };
    r.article.progress = Progress::Running(attempt);
    r.discussion.progress = Progress::Running(attempt);
    p
}

fn complete_section(p: &mut Reader, article: bool, text: &str) {
    let Progress::Running(attempt) = (if article {
        &p.readings[&1].article.progress
    } else {
        &p.readings[&1].discussion.progress
    }) else {
        panic!("section must be running")
    };
    p.complete(jobs::Completion {
        story: 1,
        attempt: *attempt,
        output: if article {
            jobs::Output::Article(Ok(Article { text: text.into() }))
        } else {
            jobs::Output::Discussion(Ok(discussion(text)))
        },
    });
}

async fn move_reader(p: &mut Reader, app: &mut hn_lib::app::App, key: KeyCode) {
    let (tx, _rx) = mpsc::unbounded_channel();
    crate::reader_actions::handle(
        p,
        app,
        key,
        tx,
        hn_lib::HackerNewsCliServiceImpl::new(),
        &mut None,
    )
    .await
    .unwrap();
}

#[test]
fn reader_chip_table_and_chrome_use_theme_roles() {
    use ratatui::style::Modifier;
    for (case, expected) in [
        ("no key", ""),
        ("rejected", "! OpenRouter key rejected · M edit"),
        ("idle", "S summarize"),
        ("pending", "⠋ summarizing · Fast"),
        ("partial", "⠋ 1/2 ready · Fast"),
        ("ready", "✓ summary · Fast"),
        ("hidden", "✓ summary · Fast · S show"),
        ("mixed", "✓ summary · Article Fast · Discussion Thorough"),
        ("incomplete", "! summary incomplete · M retry"),
        ("failed", "! summary incomplete · M retry"),
        ("updating", "⠋ updating · Thorough"),
        ("source", "source 1 of 1 · reply · Fast"),
    ] {
        let mut p = ready();
        let r = p.readings.get_mut(&1).unwrap();
        let attempt = Attempt {
            id: 42,
            profile: Profile::Fast,
            automatic: false,
            refresh: false,
        };
        match case {
            "no key" => p.config.api_key.clear(),
            "rejected" => p.auth_failed = true,
            "idle" => {
                r.article = Section::default();
                r.discussion = Section::default();
            }
            "pending" => {
                r.article.result = None;
                r.discussion.result = None;
                r.article.progress = Progress::Running(attempt);
            }
            "partial" => {
                r.discussion.result = None;
                r.discussion.progress = Progress::Running(attempt);
            }
            "hidden" => r.shown = false,
            "mixed" => r.discussion.result.as_mut().unwrap().profile = Profile::Thorough,
            "incomplete" => r.discussion = Section::default(),
            "failed" => {
                r.discussion.progress = Progress::Failed(attempt, Failure::new("Unavailable"))
            }
            "updating" => {
                r.article.progress = Progress::Running(Attempt {
                    profile: Profile::Thorough,
                    ..attempt
                })
            }
            "source" => r.source = Some(11),
            _ => {}
        }
        let mut app = comments_app();
        let b = buffer(&mut p, &mut app);
        let meta = row(&b, 1);
        assert!(meta.trim_end().ends_with(expected), "{case}: {meta}");
        assert_eq!(&meta[meta.len() - 2..], "  ", "chip right margin: {case}");
        assert_eq!(b[(2, 0)].fg, Theme::TRUE_DARK.fg.fg.unwrap());
        assert!(b[(2, 0)].modifier.contains(Modifier::BOLD));
        assert!((0..120).all(|x| b[(x, 2)].fg == Theme::TRUE_DARK.line.fg.unwrap()));
        assert!((0..120).all(|x| b[(x, 38)].fg == Theme::TRUE_DARK.line.fg.unwrap()));
        if expected.is_empty() {
            assert_eq!(meta.trim(), "news.ycombinator.com · 1 points · 2 comments");
            assert!(!row(&b, 39).contains("Set up"));
            assert!(row(&b, 39).contains("S summary"));
        } else {
            let x = 118 - expected.chars().count() as u16;
            let style = if expected.starts_with('!') {
                Theme::TRUE_DARK.error
            } else if expected.starts_with(['✓', 'S']) {
                Theme::TRUE_DARK.accent
            } else {
                Theme::TRUE_DARK.dim
            };
            assert_eq!(b[(x, 1)].fg, style.fg.unwrap(), "{case}");
        }
    }
}

#[tokio::test]
async fn moved_then_arrival_keeps_panel_empty_and_comment_cells_identical() {
    for key in [KeyCode::Char('j'), KeyCode::PageDown, KeyCode::Char('J')] {
        let mut p = pending_reader();
        let mut app = comments_app();
        buffer(&mut p, &mut app);
        move_reader(&mut p, &mut app, key).await;
        let before = buffer(&mut p, &mut app);
        assert!(p.readings[&1].moved, "{key:?} must record movement");
        complete_section(&mut p, true, "Article claim.");
        complete_section(&mut p, false, "Discussion claim.");
        let after = buffer(&mut p, &mut app);
        assert_eq!(
            p.readings[&1].growth_cap,
            Some(0),
            "arrival must not grow the empty panel"
        );
        assert!(row(&after, 1).trim_end().ends_with("S show"));
        assert_eq!(
            &before.content[360..4560],
            &after.content[360..4560],
            "arriving summary moved or restyled comment rows after {key:?}"
        );
        move_reader(&mut p, &mut app, KeyCode::Char('S')).await;
        assert!(screen(&mut p, &mut app, 120, 40).contains("ARTICLE  Fast"));
        assert_eq!(
            p.readings[&1].growth_cap, None,
            "explicit show lifts the cap"
        );
        p.open(story(1));
        assert!(!p.readings[&1].moved, "reopen resets movement per visit");
    }
}

#[tokio::test]
async fn tab_after_movement_reveals_the_hidden_summary() {
    let mut p = pending_reader();
    let mut app = comments_app();
    buffer(&mut p, &mut app);
    move_reader(&mut p, &mut app, KeyCode::Char('j')).await;
    complete_section(&mut p, true, "Article claim.");
    complete_section(&mut p, false, "Discussion claim.");
    buffer(&mut p, &mut app);
    move_reader(&mut p, &mut app, KeyCode::Tab).await;
    assert_eq!(p.readings[&1].growth_cap, None, "Tab lifts the cap like S");
    assert!(
        screen(&mut p, &mut app, 120, 40).contains("DISCUSSION  Fast"),
        "the focused citation must be visible"
    );
}

#[test]
fn unmoved_arrival_expands_the_panel() {
    let mut p = pending_reader();
    let mut app = comments_app();
    buffer(&mut p, &mut app);
    assert_eq!(p.readings[&1].growth_cap, Some(0));
    assert!(!p.key(KeyCode::Char('k')));
    app.prev_comment();
    complete_section(&mut p, true, "Article claim.");
    complete_section(&mut p, false, "Discussion claim.");
    let b = buffer(&mut p, &mut app);
    assert!(p.readings[&1].growth_cap.unwrap() > 0);
    assert!(row(&b, 3).contains("ARTICLE  Fast · captured 12:00"));
    assert!(!row(&b, 1).contains("S show"));
}

#[tokio::test]
async fn moved_panel_keeps_its_height_when_results_grow_or_shrink() {
    let mut p = pending_reader();
    let mut app = comments_app();
    complete_section(&mut p, true, "Article claim.");
    buffer(&mut p, &mut app);
    let height = p.readings[&1].growth_cap.unwrap();
    assert!(height > 0);
    move_reader(&mut p, &mut app, KeyCode::PageDown).await;
    let before = buffer(&mut p, &mut app);
    complete_section(&mut p, false, &"Discussion claim. ".repeat(150));
    let after = buffer(&mut p, &mut app);
    let start = (3 + height) * 120;
    assert_eq!(
        &before.content[start..4560],
        &after.content[start..4560],
        "discussion arrival changed the comments' screen position"
    );
    assert!(row(&after, 2 + height as u16).contains("J/K scroll"));
    p.readings
        .get_mut(&1)
        .unwrap()
        .discussion
        .result
        .as_mut()
        .unwrap()
        .value
        .points
        .clear();
    p.readings
        .get_mut(&1)
        .unwrap()
        .discussion
        .result
        .as_mut()
        .unwrap()
        .value
        .overview
        .clear();
    let shorter = buffer(&mut p, &mut app);
    assert_eq!(
        &after.content[start..4560],
        &shorter.content[start..4560],
        "shorter replacement moved comments upward"
    );
}

#[test]
fn citation_focus_tints_full_point_rows_and_underlines_only_the_focused_token() {
    use ratatui::style::Modifier;
    let mut p = ready();
    let point = &mut p
        .readings
        .get_mut(&1)
        .unwrap()
        .discussion
        .result
        .as_mut()
        .unwrap()
        .value
        .points[0];
    point.text = "Wide 界 and café evidence. ".repeat(12);
    point.sources = vec![11, 10];
    p.key(KeyCode::Tab);
    let mut app = comments_app();
    let b = buffer(&mut p, &mut app);
    let theme = Theme::TRUE_DARK;
    let mut active_rows = 0;
    for y in 3..38 {
        if b[(2, y)].symbol() != "┃" {
            continue;
        }
        active_rows += 1;
        assert_eq!(b[(2, y)].fg, theme.accent.fg.unwrap());
        assert!((2..120).all(|x| b[(x, y)].bg == theme.sel.bg.unwrap()));
        assert!((0..2).all(|x| b[(x, y)].bg != theme.sel.bg.unwrap()));
    }
    assert!(active_rows > 1, "wrapped point must tint every row");
    let focused: String = b
        .content
        .iter()
        .filter(|c| c.modifier.contains(Modifier::UNDERLINED | Modifier::BOLD))
        .map(|c| {
            assert_eq!(c.fg, theme.accent.fg.unwrap());
            c.symbol()
        })
        .collect();
    assert_eq!(focused, "[1]");
    let linked: String = b
        .content
        .iter()
        .filter(|c| c.fg == theme.link.fg.unwrap())
        .map(|c| c.symbol())
        .collect();
    assert_eq!(linked.trim(), "[2]");
    assert!(b
        .content
        .iter()
        .all(|c| !c.modifier.contains(Modifier::REVERSED)));
}
