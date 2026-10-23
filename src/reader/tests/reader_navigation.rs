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
        let comments = initial.find("COMMENTS  3").unwrap();
        // Even the smallest size keeps the summary reachable above the comments.
        assert!(initial.find("ARTICLE").unwrap() < comments);
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
        r.story.time_ago = "2h".into();
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
            assert_eq!(
                meta.trim(),
                "news.ycombinator.com · author · 1 points · 2 comments · 2h"
            );
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
            p.readings[&1].growth_cap, 0,
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
        move_reader(&mut p, &mut app, key).await;
        let before = buffer(&mut p, &mut app);
        let start = (3..38)
            .find(|&y| row(&before, y).contains("Comment "))
            .expect("visible comment text") as usize
            * 120;
        p.readings
            .get_mut(&1)
            .unwrap()
            .article
            .result
            .as_mut()
            .unwrap()
            .value
            .text = "Replacement article. ".repeat(150);
        let after = buffer(&mut p, &mut app);
        assert_eq!(
            &before.content[start..4560],
            &after.content[start..4560],
            "replacement after explicit S and movement changed comment cells"
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
    assert!(
        screen(&mut p, &mut app, 120, 40).contains("DISCUSSION  Fast"),
        "the focused citation must be visible"
    );
    move_reader(&mut p, &mut app, KeyCode::PageDown).await;
    let before = buffer(&mut p, &mut app);
    let start = (3..38)
        .find(|&y| row(&before, y).contains("Comment "))
        .expect("visible comment text") as usize
        * 120;
    p.readings
        .get_mut(&1)
        .unwrap()
        .article
        .result
        .as_mut()
        .unwrap()
        .value
        .text = "Replacement article. ".repeat(150);
    let after = buffer(&mut p, &mut app);
    assert_eq!(
        &before.content[start..4560],
        &after.content[start..4560],
        "replacement after Tab and movement changed comment cells"
    );
}

#[test]
fn unmoved_arrival_expands_the_panel() {
    let mut p = pending_reader();
    let mut app = comments_app();
    buffer(&mut p, &mut app);
    assert_eq!(p.readings[&1].growth_cap, 0);
    assert!(!p.key(KeyCode::Char('k')));
    app.prev_comment();
    complete_section(&mut p, true, "Article claim.");
    complete_section(&mut p, false, "Discussion claim.");
    let b = buffer(&mut p, &mut app);
    assert!(p.readings[&1].growth_cap > 0);
    assert!(row(&b, 3).contains("ARTICLE  Fast · captured 12:00"));
    assert!(!row(&b, 1).contains("S show"));
}

#[tokio::test]
async fn moved_panel_keeps_its_height_when_results_grow_or_shrink() {
    let mut p = pending_reader();
    let mut app = comments_app();
    complete_section(&mut p, true, "Article claim.");
    buffer(&mut p, &mut app);
    let height = p.readings[&1].growth_cap;
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
    assert!(row(&after, 1 + height as u16).contains("J/K scroll"));
    assert!(
        row(&after, 2 + height as u16).trim().is_empty(),
        "a blank row must separate the panel from the comments"
    );
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

#[test]
fn citation_focus_suppresses_comment_selection_until_comment_navigation() {
    let mut p = ready();
    let mut app = comments_app();
    let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
    for (key, selected, source) in [
        (KeyCode::Tab, false, false),
        (KeyCode::Enter, false, true),
        (KeyCode::Esc, false, false),
        (KeyCode::Char('j'), true, false),
    ] {
        p.key(key);
        let frame = terminal.draw(|f| p.render(f, &mut app, 0)).unwrap();
        let buffer = frame.buffer;
        let text: String = buffer.content.iter().map(|c| c.symbol()).collect();
        assert_eq!(text.contains("reader0"), !source);
        let start = buffer
            .content
            .chunks(100)
            .position(|row| {
                row.iter()
                    .map(|c| c.symbol())
                    .collect::<String>()
                    .contains("COMMENTS")
            })
            .map_or(0, |y| (y + 1) * 100);
        assert_eq!(
            buffer.content[start..]
                .iter()
                .any(|c| c.bg == p.theme.sel.bg.unwrap()),
            selected,
            "selection after {key:?}"
        );
        assert_eq!(
            buffer.content.iter().any(|c| c.symbol() == "▎"),
            selected,
            "comment bar after {key:?}"
        );
    }
    p.key(KeyCode::Tab);
    p.key(KeyCode::Esc);
    assert!(
        !p.is_open(),
        "Esc from citation focus retains existing exit behavior"
    );
}

#[test]
fn reader_help_keys_match_handlers_and_use_accent() {
    use crossterm::event::KeyCode::{self, *};
    use hn_lib::event::{handle_comments_key, CommentAction};
    let theme = Theme::TRUE_DARK;
    let entries: &[(&str, &[KeyCode])] = &[
        ("j/k/↑/↓", &[Char('j'), Char('k'), Up, Down]),
        ("PgUp/PgDn", &[PageUp, PageDown]),
        ("Enter/l/→", &[Enter, Char('l'), Right]),
        ("c", &[Char('c')]),
        ("[/]", &[Char('['), Char(']')]),
        ("u", &[Char('u')]),
        ("g/G", &[Char('g'), Char('G')]),
        ("S", &[Char('S')]),
        ("J/K", &[Char('J'), Char('K')]),
        ("M", &[Char('M')]),
        ("Tab/Shift-Tab", &[Tab, BackTab]),
        ("Enter", &[Enter]),
        ("Esc", &[Esc]),
        ("o", &[Char('o')]),
        ("O", &[Char('O')]),
        ("L", &[Char('L')]),
        ("s", &[Char('s')]),
        ("r", &[Char('r')]),
        ("Esc/q/h/←", &[Esc, Char('q'), Char('h'), Left]),
        ("?", &[Char('?')]),
    ];
    for width in [60, 100] {
        let mut terminal = Terminal::new(TestBackend::new(width, 40)).unwrap();
        let frame = terminal
            .draw(|f| hn_lib::ui::widgets::render_help(f, theme, true, None))
            .unwrap();
        for (label, keys) in entries {
            let cells = frame
                .buffer
                .content
                .chunks(usize::from(width))
                .find_map(|row| {
                    let text: String = row.iter().map(|c| c.symbol()).collect();
                    text.find(&format!(" {label}  "))
                        .map(|index| &row[text[..index].chars().count() + 1])
                })
                .unwrap_or_else(|| panic!("missing {label}"));
            assert_eq!(cells.fg, theme.accent.fg.unwrap(), "{label}");
            for key in *keys {
                if *key == Char('O') {
                    assert!(include_str!("../../reader_actions.rs")
                        .contains("if key == KeyCode::Char('O')"));
                } else {
                    assert!(
                        !matches!(handle_comments_key(*key), CommentAction::None)
                            || ready().key(*key),
                        "unhandled {key:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn motion_effects_stay_on_rgb_line_at_mid_duration() {
    use ratatui::style::Color;
    let theme = Theme::TRUE_DARK;
    let rgb = |color| match color {
        Color::Rgb(r, g, b) => [r, g, b].map(f32::from),
        _ => panic!("expected RGB, got {color:?}"),
    };
    for effect in ["backdrop", "save", "notice", "panel"] {
        let mut app = hn_lib::app::App::with_theme(theme);
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let (cell, orange, grey) = match effect {
            // The save flash's fixed start is a pale orange; paint a grey endpoint.
            "save" => ((0, 0), Color::Rgb(0xff, 0xf6, 0xea), theme.dim.fg.unwrap()),
            "notice" => ((2, 39), theme.accent.fg.unwrap(), theme.dim.fg.unwrap()),
            "panel" => ((4, 1), theme.accent.fg.unwrap(), theme.dim.fg.unwrap()),
            _ => ((0, 0), theme.accent.fg.unwrap(), theme.scrim.fg.unwrap()),
        };
        let frame = terminal
            .draw(|f| {
                let area = f.area();
                match effect {
                    "save" => {
                        app.bookmarks.push(story(1));
                        app.save_flash = Some((1, Instant::now() - Duration::from_millis(150)));
                        f.buffer_mut().set_style(area, theme.dim);
                        app.render_save_flash(f.buffer_mut(), area, 1);
                    }
                    "notice" => {
                        let mut p = ready();
                        p.theme = theme;
                        p.browser_notice = "Article opened".into();
                        p.notice_started = Some(Instant::now() - Duration::from_millis(2200));
                        p.render(f, &mut app, 0);
                    }
                    "panel" => {
                        let mut p = ready();
                        let r = p.readings.get_mut(&1).unwrap();
                        r.theme = theme;
                        let result = r.article.result.as_mut().unwrap();
                        result.capture = Arc::new(Capture::new(ArticleCapture {
                            partial: true,
                            title: "Title".into(),
                            text: "Source".into(),
                        }));
                        let height = (r.summary_lines(120).0.len() + 1).min(26) as u16;
                        r.panel_motion =
                            Some((Instant::now() - Duration::from_millis(125), 0, height));
                        r.render_summary(f, area);
                    }
                    _ => {
                        f.buffer_mut().set_style(area, theme.accent);
                        hn_lib::ui::widgets::render_help(
                            f,
                            theme,
                            false,
                            Some(Instant::now() - Duration::from_millis(60)),
                        );
                    }
                }
            })
            .unwrap();
        let sampled = rgb(frame.buffer[cell].fg);
        let (a, b) = (rgb(orange), rgb(grey));
        let fraction = (sampled[0] - a[0]) / (b[0] - a[0]);
        assert!(
            (0.05..0.95).contains(&fraction),
            "{effect}: not mid-fade {sampled:?}"
        );
        for channel in 0..3 {
            assert!(
                (a[channel].min(b[channel])..=a[channel].max(b[channel]))
                    .contains(&sampled[channel]),
                "{effect}: channel {channel} escaped endpoints: {sampled:?}"
            );
            assert!(
                (sampled[channel] - (a[channel] + fraction * (b[channel] - a[channel]))).abs()
                    <= 2.0,
                "{effect}: off RGB line: {sampled:?}"
            );
        }
    }
}

#[test]
fn motion_browser_notice_timing_and_persistent_errors() {
    for motion in [true, false] {
        for (ms, busy) in [
            (0, false),
            (1990, false),
            (2400, false),
            (4000, false),
            (5000, true),
        ] {
            let mut p = ready();
            p.theme.motion = motion;
            p.browser_notice = "Article opened".into();
            p.notice_started = Some(Instant::now() - Duration::from_millis(ms));
            p.browser_busy = busy;
            let mut app = comments_app();
            p.poll();
            let b = buffer(&mut p, &mut app);
            if ms >= 4000 && !busy {
                assert!(p.browser_notice.is_empty());
                assert!(!row(&b, 39).contains("Article opened"));
                assert!(!p.has_active_motion(Instant::now()));
            } else {
                assert!(row(&b, 39).contains("Article opened"));
                assert_eq!(
                    b[(2, 39)].fg,
                    if busy || (motion && ms < 2000) {
                        p.theme.accent.fg
                    } else {
                        p.theme.dim.fg
                    }
                    .unwrap(),
                    "motion={motion}, ms={ms}, busy={busy}"
                );
                assert_eq!(p.has_active_motion(Instant::now()), !busy);
            }
        }
        let mut p = ready();
        p.theme.motion = motion;
        let mut app = comments_app();
        p.browser_notice = "Article opened".into();
        p.notice_started = Some(Instant::now() - Duration::from_secs(5));
        app.bookmark_notice = Some("Save failed: retry".into());
        p.poll();
        let b = buffer(&mut p, &mut app);
        assert!(row(&b, 39).contains("Save failed: retry"));
        assert_eq!(b[(2, 39)].fg, p.theme.error.fg.unwrap());
        app.bookmark_notice = None;
        app.set_comment_error("Comments failed".into());
        assert!(row(&buffer(&mut p, &mut app), 39).contains("Comments failed"));
        app.set_comments(vec![]);
        p.readings.get_mut(&1).unwrap().staged =
            p.readings.get_mut(&1).unwrap().discussion.result.take();
        assert!(row(&buffer(&mut p, &mut app), 39).contains("Summary updated"));
    }
}

#[test]
fn motion_save_star_in_feed_and_reader_only_on_successful_save() {
    use ratatui::style::Color;
    let path = std::env::temp_dir().join(format!(
        "hn-motion-save-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    for motion in [true, false] {
        let theme = Theme {
            motion,
            ..Theme::TRUE_DARK
        };
        let mut app = hn_lib::app::App::with_theme(theme);
        app.set_stories(vec![story(1)]);
        crate::toggle_bookmark(&mut app, Some(&path));
        assert_eq!(app.save_flash.is_some(), motion);
        assert!(app.is_bookmarked(1));
        for ms in [0, 300] {
            app.save_flash = Some((
                1,
                if ms == 0 {
                    Instant::now() + Duration::from_secs(1)
                } else {
                    Instant::now() - Duration::from_millis(ms)
                },
            ));
            let expected = if motion && ms == 0 {
                Color::Rgb(0xff, 0xf6, 0xea)
            } else {
                theme.accent.fg.unwrap()
            };
            let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
            let frame = terminal
                .draw(|f| hn_lib::ui::stories::render(f, &mut app, 0))
                .unwrap();
            assert_eq!(
                frame
                    .buffer
                    .content
                    .iter()
                    .find(|c| c.symbol() == "★")
                    .unwrap()
                    .fg,
                expected
            );
            let mut p = ready();
            assert_eq!(buffer(&mut p, &mut app)[(2, 0)].fg, expected);
            assert_eq!(app.has_active_motion(Instant::now()), motion && ms == 0);
        }
        crate::toggle_bookmark(&mut app, Some(&path));
        assert!(!app.is_bookmarked(1));
        assert!(app.save_flash.is_none(), "unsaving must not flash");
        crate::toggle_bookmark(&mut app, None);
        assert!(!app.is_bookmarked(1));
        assert!(app.save_flash.is_none(), "failed saves must not flash");
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn motion_panel_height_text_retarget_and_reduced_motion() {
    let mut reading = Reading::new(story(1));
    reading.article.result = Some(done(
        Article {
            text: "Article claim.\n".repeat(30),
        },
        Profile::Fast,
        ArticleCapture {
            title: "Title".into(),
            text: "Source".into(),
            partial: false,
        },
    ));
    let draw = |r: &mut Reading, height| {
        let mut terminal = Terminal::new(TestBackend::new(80, height)).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                f.buffer_mut().set_style(area, r.theme.fg);
                let comments = r.render_summary(f, area);
                f.render_widget(ratatui::widgets::Paragraph::new("◆"), comments);
            })
            .unwrap()
            .buffer
            .clone()
    };
    let panel_height = |b: &ratatui::buffer::Buffer| {
        b.content.iter().position(|c| c.symbol() == "◆").unwrap() as u16 / b.area.width
    };
    for motion in [true, false] {
        reading.theme = Theme {
            motion,
            ..Theme::TRUE_DARK
        };
        for ms in [0, 90, 180, 250] {
            let start = if ms == 0 {
                Instant::now() + Duration::from_secs(1)
            } else {
                Instant::now() - Duration::from_millis(ms)
            };
            reading.panel_motion = Some((start, 4, 20));
            let b = draw(&mut reading, 30);
            let height = panel_height(&b);
            if !motion || ms >= 180 {
                assert_eq!(height, 20, "panel must reach target height at {ms} ms");
            } else if ms == 0 {
                assert_eq!(
                    height, 4,
                    "panel at 0 ms must draw its start height, not the target"
                );
            } else {
                assert!(
                    height > 4 && height < 20,
                    "panel at 90 ms must be between start and target, got {height}"
                );
            }
            assert_eq!(b[(4, 1)].symbol(), "A");
            if !motion || ms == 0 || ms >= 250 {
                assert_eq!(
                    b[(4, 1)].fg,
                    if motion && ms == 0 {
                        reading.theme.dim.fg
                    } else {
                        reading.theme.fg.fg
                    }
                    .unwrap()
                );
            }
            assert_eq!(
                reading.has_active_motion(Instant::now()),
                motion && ms < 250
            );
            if !motion {
                assert!(reading.panel_motion.is_none());
            }
        }
    }
    reading.theme = Theme::TRUE_DARK;
    reading.panel_motion = None;
    assert_eq!(
        panel_height(&draw(&mut reading, 30)),
        0,
        "opening starts at zero"
    );
    reading.panel_motion = Some((Instant::now() + Duration::from_secs(1), 4, 20));
    let before = panel_height(&draw(&mut reading, 30));
    assert_eq!(
        panel_height(&draw(&mut reading, 36)),
        before,
        "retarget must start at the current drawn height"
    );
    assert_eq!(reading.panel_motion.unwrap().2, 24);
    assert_eq!(
        panel_height(&draw(&mut reading, 15)),
        7,
        "shrinking snaps immediately to half of a short body"
    );
    assert!(!reading.has_active_motion(Instant::now()));
    reading.shown = false;
    assert_eq!(panel_height(&draw(&mut reading, 30)), 0);
    assert!(!reading.has_active_motion(Instant::now()));

    let mut p = ready();
    p.readings
        .get_mut(&1)
        .unwrap()
        .article
        .result
        .as_mut()
        .unwrap()
        .value
        .text = "A long summary.\n".repeat(40);
    let mut app = comments_app();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let frame = terminal.draw(|f| p.render(f, &mut app, 0)).unwrap();
    let heading = (0..22)
        .find(|&y| row(frame.buffer, y).contains("COMMENTS"))
        .unwrap();
    assert!(22 - heading >= 10);
    assert!(row(frame.buffer, heading - 1).trim().is_empty());
    assert!(row(frame.buffer, heading - 2).contains("J/K scroll"));
}

#[tokio::test]
async fn movement_mid_animation_freezes_drawn_height_and_comment_cells() {
    for key in [KeyCode::Char('j'), KeyCode::Char('J')] {
        let mut p = ready();
        let mut app = comments_app();
        buffer(&mut p, &mut app);
        let r = p.readings.get_mut(&1).unwrap();
        r.theme.motion = true;
        let target = r.growth_cap as u16;
        r.panel_motion = Some((Instant::now() - Duration::from_millis(60), 0, target));
        move_reader(&mut p, &mut app, key).await;
        let before = buffer(&mut p, &mut app);
        let r = p.readings.get_mut(&1).unwrap();
        let (_, from, to) = r.panel_motion.unwrap();
        assert!(from > 0 && from < target, "must freeze mid-opening");
        assert_eq!(
            from, to,
            "movement must snap the animation in the same frame"
        );
        assert_eq!(r.growth_cap, usize::from(from));
        r.panel_motion.as_mut().unwrap().0 = Instant::now() - Duration::from_secs(1);
        r.article.result.as_mut().unwrap().value.text = "Long replacement. ".repeat(150);
        let after = buffer(&mut p, &mut app);
        let start = (3 + usize::from(from)) * 120;
        assert_eq!(&before.content[start..4560], &after.content[start..4560]);
    }
}

#[test]
fn short_panel_shows_summary_content_without_zero_scroll_range() {
    let mut p = ready();
    let mut app = comments_app();
    let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
    let frame = terminal.draw(|f| p.render(f, &mut app, 0)).unwrap();
    assert!(row(frame.buffer, 4).contains("Article claim."));
    for y in 0..10 {
        let text = row(frame.buffer, y);
        assert!(!text.contains("–0 of"), "{text}");
        assert!(!text.contains("J/K scroll"), "{text}");
    }
}

#[test]
fn browser_channel_error_persists_in_error_role_after_five_seconds() {
    let mut readers = [ready(), ready()];
    for (p, motion) in readers.iter_mut().zip([true, false]) {
        p.theme.motion = motion;
        p.browser_busy = true;
        p.tx.send(Err("Browser failed".into())).unwrap();
        p.poll();
    }
    // Use real elapsed time: production notices use std::time::Instant.
    std::thread::sleep(Duration::from_secs(5));
    for p in &mut readers {
        let mut app = comments_app();
        p.poll();
        let b = buffer(p, &mut app);
        assert!(row(&b, 39).contains("Browser failed"));
        assert_eq!(b[(2, 39)].fg, p.theme.error.fg.unwrap());
        assert!(!p.has_active_motion(Instant::now()));
        p.tx.send(Ok((None, "Article opened".into()))).unwrap();
        p.poll();
        assert!(row(&buffer(p, &mut app), 39).contains("Article opened"));
        p.notice_started = Some(Instant::now() - Duration::from_secs(4));
        p.poll();
        assert!(p.browser_notice.is_empty());
        assert!(!row(&buffer(p, &mut app), 39).contains("Article opened"));
    }
}
