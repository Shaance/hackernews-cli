use super::render;
use crate::app::{App, Comment, CommentState};
use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};

fn test_comment(id: i32, text: &str) -> Comment {
    Comment {
        id,
        author: format!("user{id}"),
        text: text.to_string(),
        links: vec![],
        time_ago: "now".to_string(),
        state: CommentState::Collapsed,
        depth: 0,
        deleted: false,
        child_ids: vec![],
    }
}

fn buffer_text(buffer: &Buffer) -> String {
    buffer
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>()
}

#[test]
fn comments_remain_visible_when_nonfatal_error_exists() {
    let mut app = App::new();
    app.view_comments(1, "Story".to_string(), "https://example.com".to_string());
    app.set_comments(vec![test_comment(10, "visible comment body")]);
    app.set_comment_error("Failed to load comment children: child failed".to_string());

    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).expect("test terminal should initialize");

    terminal
        .draw(|frame| render(frame, &mut app, 0))
        .expect("comments should render");

    let text = buffer_text(terminal.backend().buffer());
    assert!(text.contains("visible comment body"));
    assert!(text.contains("Failed to load comment children"));
    assert!(!text.contains("Error: Failed to load comment children"));
}

#[test]
fn short_narrow_terminal_shows_help_and_comment_count() {
    let mut app = App::new();
    app.view_comments(
        1,
        "A long discussion title that must not hide the comment count".into(),
        "https://example.com".into(),
    );
    app.set_comments(vec![test_comment(10, "readable body")]);
    let mut terminal = Terminal::new(TestBackend::new(37, 12)).unwrap();
    terminal.draw(|f| render(f, &mut app, 0)).unwrap();
    let text = buffer_text(terminal.backend().buffer());
    assert!(text.contains("1 loaded comments") && text.contains("? help"));
    app.show_help = true;
    let frame = terminal.draw(|f| render(f, &mut app, 0)).unwrap();
    assert!(buffer_text(frame.buffer).contains("PgUp/PgDn"));
    // A clamped modal shows only the rows that fit. Check all groups at full height.
    let mut terminal = Terminal::new(TestBackend::new(50, 40)).unwrap();
    let frame = terminal.draw(|f| render(f, &mut app, 0)).unwrap();
    let text = buffer_text(frame.buffer);
    for shortcut in ["PgUp/PgDn", "Save/unsave", "Esc/q/h/←  Back", "?  Close"] {
        assert!(text.contains(shortcut), "missing {shortcut}");
    }
}

#[test]
fn feed_metadata_shows_external_subdomains_and_native_discussions() {
    for width in [80, 120, 160] {
        let mut app = App::new();
        app.set_stories(
            [
                "https://research.example.com/paper",
                "https://news.ycombinator.com/item?id=2",
                "",
            ]
            .iter()
            .enumerate()
            .map(|(id, url)| crate::HNCLIItem {
                id: id as i32,
                title: format!("Story {id}"),
                url: (*url).into(),
                author: "reader".into(),
                time: String::new(),
                time_ago: "now".into(),
                score: 1,
                text: String::new(),
                links: vec![],
                comments: Some(2),
            })
            .collect(),
        );
        let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
        terminal.draw(|frame| render(frame, &mut app, 0)).unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("research.example.com"));
        assert_eq!(text.matches("HN discussion").count(), 2);
    }
}

fn feed() -> App {
    let mut app = App::with_theme(crate::theme::Theme::TRUE_DARK);
    app.set_stories_for(
        crate::app::StoryType::Best,
        1,
        (1..=12)
            .map(|id| crate::HNCLIItem {
                id,
                title: format!("Story {id}"),
                url: "https://example.com".into(),
                author: "reader".into(),
                time: String::new(),
                time_ago: "2h".into(),
                score: 10,
                text: String::new(),
                links: vec![],
                comments: Some(2),
            })
            .collect(),
    );
    app
}

#[test]
fn feed_selection_has_full_width_tint_and_preserves_roles() {
    use ratatui::style::Modifier;
    let mut app = feed();
    let theme = app.theme();
    app.stories[0].title = "界".repeat(100);
    app.bookmarks.push(app.stories[0].clone());
    let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
    let frame = terminal.draw(|f| render(f, &mut app, 0)).unwrap();
    // Terminal output omits continuation cells hidden by wide glyphs.
    let buffer = frame.buffer;
    for y in 2..4 {
        for x in 0..80 {
            assert_eq!(
                buffer[(x, y)].bg,
                theme.sel.bg.unwrap(),
                "selected feed cell ({x},{y}) must have sel background"
            );
        }
        assert_eq!(buffer[(0, y)].symbol(), "▎");
        assert_eq!(buffer[(0, y)].fg, theme.accent.fg.unwrap());
    }
    assert!(
        (0..80).all(|x| buffer[(x, 4)].bg != theme.sel.bg.unwrap()),
        "spacer must remain unselected"
    );
    for (index, cell) in buffer.content().iter().enumerate() {
        assert!(
            !cell.modifier.contains(Modifier::REVERSED),
            "feed cell {index} must not use REVERSED"
        );
    }
    assert_eq!(buffer[(4, 2)].fg, theme.dim.fg.unwrap());
    assert_eq!(buffer[(7, 2)].fg, theme.fg.fg.unwrap());
    assert!(buffer[(7, 2)].modifier.contains(Modifier::BOLD));
    assert_eq!(buffer[(7, 3)].fg, theme.dim.fg.unwrap());
    assert_eq!(buffer[(42, 3)].fg, theme.fg.fg.unwrap());
    assert_eq!(buffer[(77, 2)].symbol(), "…");
    assert_eq!(buffer[(79, 2)].symbol(), "★");
    assert_eq!(buffer[(79, 2)].fg, theme.accent.fg.unwrap());
    assert_eq!(buffer[(1, 0)].bg, theme.brand.bg.unwrap());
    assert!((0..80).all(|x| buffer[(x, 1)].fg == theme.line.fg.unwrap()));
    app.set_story_type(crate::app::StoryType::New);
    app.set_story_error("offline".into());
    terminal.draw(|f| render(f, &mut app, 0)).unwrap();
    let buffer = terminal.backend().buffer();
    assert!(buffer_text(buffer).contains("failed New p1  Page 1"));
    assert_eq!(
        buffer[(24, 0)].fg,
        theme.accent.fg.unwrap(),
        "displayed Best tab stays active after failed New fetch"
    );
    assert!(buffer_text(buffer).contains("offline   j/k move"));
}

#[test]
fn feed_at_80_by_24_shows_ten_stories_and_whole_hints() {
    let mut app = feed();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| render(f, &mut app, 0)).unwrap();
    let buffer = terminal.backend().buffer();
    for rank in 1..=10 {
        let y = 2 * rank;
        let field: String = (2..5).map(|x| buffer[(x, y)].symbol()).collect();
        assert_eq!(
            field.trim(),
            rank.to_string(),
            "rank {rank} must be visible"
        );
    }
    let footer: String = (0..80).map(|x| buffer[(x, 23)].symbol()).collect();
    assert_eq!(
        footer.trim_end(),
        "  j/k move   Enter read   1–4 feeds   s save   ? keys   n/p page"
    );
    assert_eq!(buffer[(47, 23)].fg, app.theme().accent.fg.unwrap());
    assert_eq!(buffer[(49, 23)].fg, app.theme().dim.fg.unwrap());
    app.selected_index = 11;
    terminal.draw(|f| render(f, &mut app, 0)).unwrap();
    assert!(buffer_text(terminal.backend().buffer()).contains("▎  12  Story 12"));
}

#[test]
fn thread_selection_quotes_guides_and_replies_preserve_roles() {
    let mut app = App::with_theme(crate::theme::Theme::TRUE_DARK);
    let theme = app.theme();
    let mut leaf = test_comment(
        12,
        &format!(
            "  > quote https://example.com/{}\nreply http://example.org",
            "path".repeat(30)
        ),
    );
    leaf.child_ids = vec![13, 14];
    let mut child = test_comment(11, "child");
    child.state = CommentState::Expanded {
        children: vec![leaf],
    };
    child.child_ids = vec![12];
    let mut root = test_comment(10, "root");
    root.state = CommentState::Expanded {
        children: vec![child],
    };
    root.child_ids = vec![11];
    app.view_comments(1, "Thread".into(), String::new());
    app.set_comments(vec![root, test_comment(99, "last")]);
    app.next_comment();
    app.next_comment();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|f| super::comments::render_reader_content(f, f.area(), &mut app, 0, vec![], true))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let end = (6..24).find(|y| buffer[(2, *y)].symbol() == "u").unwrap();
    for y in 6..end {
        for x in 0..80 {
            assert_eq!(
                buffer[(x, y)].bg,
                theme.sel.bg.unwrap(),
                "selected comment cell ({x},{y}) must have sel background"
            );
        }
        assert_eq!(buffer[(0, y)].symbol(), "▎");
        assert_eq!(buffer[(0, y)].fg, theme.accent.fg.unwrap());
        if y < end - 1 {
            for x in [2, 4] {
                assert_eq!(buffer[(x, y)].symbol(), "│");
                assert_eq!(buffer[(x, y)].fg, theme.line.fg.unwrap());
            }
        }
    }
    assert_eq!(buffer[(6, 6)].fg, theme.fg.fg.unwrap());
    assert_eq!(buffer[(6, 7)].symbol(), "▏");
    assert_eq!(buffer[(6, 7)].fg, theme.line.fg.unwrap());
    assert_eq!(buffer[(8, 7)].fg, theme.dim.fg.unwrap());
    assert_eq!(buffer[(14, 7)].fg, theme.link.fg.unwrap());
    assert_eq!(
        buffer[(8, 8)].fg,
        theme.link.fg.unwrap(),
        "wrapped URL must stay link coloured"
    );
    let text = buffer_text(buffer);
    assert!(!text.contains("Collapse"));
    assert!(text.contains("+ 2 replies"));
    assert!(buffer
        .content()
        .iter()
        .all(|cell| !cell.modifier.contains(ratatui::style::Modifier::REVERSED)));
}

fn position(buffer: &Buffer, text: &str) -> (u16, u16) {
    for y in 0..buffer.area.height {
        let row: String = (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect();
        if let Some(index) = row.find(text) {
            return (row[..index].chars().count() as u16, y);
        }
    }
    panic!("missing {text:?} in completed frame");
}

#[test]
fn stories_help_keys_match_handlers_and_use_accent() {
    use crate::event::{handle_stories_key, StoryAction};
    use crossterm::event::KeyCode::{self, *};
    let theme = crate::theme::Theme::TRUE_DARK;
    let entries: &[(&str, &[KeyCode])] = &[
        ("j/k/↑/↓", &[Char('j'), Char('k'), Up, Down]),
        ("n/p/→/←", &[Char('n'), Char('p'), Right, Left]),
        ("1", &[Char('1')]),
        ("2", &[Char('2')]),
        ("3", &[Char('3')]),
        ("4", &[Char('4')]),
        ("Enter/o", &[Enter, Char('o')]),
        ("O", &[Char('O')]),
        ("c", &[Char('c')]),
        ("s", &[Char('s')]),
        ("r", &[Char('r')]),
        ("q/Esc", &[Char('q'), Esc]),
        ("?", &[Char('?')]),
    ];
    for width in [60, 77, 78, 80, 100] {
        let mut terminal = Terminal::new(TestBackend::new(width, 40)).unwrap();
        let frame = terminal
            .draw(|f| super::widgets::render_help(f, theme, false, None))
            .unwrap();
        for (label, keys) in entries {
            let (x, y) = position(frame.buffer, &format!(" {label}  "));
            assert_eq!(
                frame.buffer[(x + 1, y)].fg,
                theme.accent.fg.unwrap(),
                "{label}"
            );
            for key in *keys {
                assert!(
                    !matches!(handle_stories_key(*key), StoryAction::None),
                    "{label}: {key:?}"
                );
            }
        }
        for heading in ["STORIES", "ANYWHERE"] {
            assert_eq!(
                frame.buffer[position(frame.buffer, heading)].fg,
                theme.dim.fg.unwrap()
            );
        }
        let (_, first) = position(frame.buffer, "j/k/↑/↓");
        let (_, second) = position(frame.buffer, "n/p/→/←");
        assert_eq!(second - first, u16::from(width < 78));
    }
}

#[test]
fn links_modal_scrolls_selection_and_renders_empty_state() {
    let theme = crate::theme::Theme::TRUE_DARK;
    let mut app = App::with_theme(theme);
    app.view_comments(1, "Story".into(), String::new());
    let mut comment = test_comment(1, "body");
    comment.links = (0..20)
        .map(|n| format!("https://example.com/{n:02}"))
        .collect();
    app.set_comments(vec![comment]);
    let mut terminal = Terminal::new(TestBackend::new(80, 16)).unwrap();
    for cursor in [0, 19, 10] {
        app.link_cursor = cursor;
        let frame = terminal.draw(|f| super::links::render(f, &app)).unwrap();
        let (x, y) = position(frame.buffer, &format!("https://example.com/{cursor:02}"));
        let left = position(frame.buffer, "╭").0 + 1;
        let right = position(frame.buffer, "╮").0;
        assert!((left..right).all(|x| frame.buffer[(x, y)].bg == theme.sel.bg.unwrap()));
        assert_eq!(frame.buffer[(x, y)].fg, theme.link.fg.unwrap());
        assert_eq!(frame.buffer[(left, y)].symbol(), "▎");
        assert_eq!(frame.buffer[(left, y)].fg, theme.accent.fg.unwrap());
        assert!(buffer_text(frame.buffer).contains("Esc close"));
    }
    app.set_comments(vec![]);
    let frame = terminal.draw(|f| super::links::render(f, &app)).unwrap();
    let pos = position(frame.buffer, "No links in the post or the selected comment");
    assert_eq!(frame.buffer[pos].fg, theme.dim.fg.unwrap());
}

#[test]
fn feed_states_are_centered_and_preserve_roles() {
    use crate::{app::StoryType, theme::Theme};
    let theme = Theme::TRUE_DARK;
    let mut app = App::with_theme(theme);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    for (state, title, hint, style) in [
        (
            0,
            "No saved stories yet",
            "Press s on any story to save it",
            theme.fg,
        ),
        (1, "No stories found", "r refresh", theme.fg),
        (2, "Couldn't load Best stories", "r retry", theme.fg),
        (3, "⠋ Loading stories…", "", theme.dim),
    ] {
        app.set_story_type(if state == 0 {
            StoryType::Saved
        } else {
            StoryType::Best
        });
        app.set_story_loading(state == 3);
        if state == 2 {
            app.set_story_error("offline".into());
        }
        let frame = terminal.draw(|f| render(f, &mut app, 0)).unwrap();
        let (x, y) = position(frame.buffer, title);
        assert_eq!(x, 40 - title.chars().count() as u16 / 2);
        assert_eq!(y, if state == 2 { 10 } else { 11 });
        assert_eq!(frame.buffer[(x, y)].fg, style.fg.unwrap());
        if state == 2 {
            assert_eq!(position(frame.buffer, "offline"), (37, 11));
            assert_eq!(frame.buffer[(37, 11)].fg, theme.dim.fg.unwrap());
            assert!(!buffer_text(frame.buffer).contains("! "));
            assert_eq!(buffer_text(frame.buffer).matches("offline").count(), 1);
            assert_eq!(position(frame.buffer, "j/k move").1, 23);
        }
        if !hint.is_empty() {
            let (x, y) = position(frame.buffer, hint);
            assert_eq!((x, y), (40 - hint.len() as u16 / 2, 12));
            assert_eq!(
                frame.buffer[(x, y)].fg,
                if state == 0 { theme.dim } else { theme.accent }
                    .fg
                    .unwrap()
            );
            assert_eq!(frame.buffer[(x + 2, y)].fg, theme.dim.fg.unwrap());
        }
    }
    app.set_story_error("offline ".repeat(15));
    let frame = terminal.draw(|f| render(f, &mut app, 0)).unwrap();
    assert_eq!(
        position(frame.buffer, "Couldn't load Best stories").1,
        10,
        "title, wrapped error and retry form a centered four-row block"
    );
    assert_eq!(position(frame.buffer, "r retry").1, 13);
    for y in [11, 12] {
        let cells = &frame.buffer.content[y * 80..(y + 1) * 80];
        assert!(cells
            .iter()
            .any(|c| c.symbol() == "o" && c.fg == theme.dim.fg.unwrap()));
    }
    assert!(frame.buffer.content[23 * 80..]
        .iter()
        .all(|c| c.fg != theme.error.fg.unwrap()));
    app = feed();
    app.set_story_type(StoryType::New);
    app.set_story_error("offline".into());
    let frame = terminal.draw(|f| render(f, &mut app, 0)).unwrap();
    assert_eq!(position(frame.buffer, "offline").1, 23);
    app.stories.clear();
    app.bookmark_notice = Some("Save failed".into());
    let frame = terminal.draw(|f| render(f, &mut app, 0)).unwrap();
    assert_eq!(position(frame.buffer, "Save failed").1, 23);
}

#[test]
fn motion_backdrop_preserves_initial_foreground_and_immediate_modal() {
    use crate::theme::Theme;
    use std::time::{Duration, Instant};
    for motion in [true, false] {
        let theme = Theme {
            motion,
            ..Theme::TRUE_DARK
        };
        let mut app = App::with_theme(theme);
        app.toggle_help();
        assert_eq!(app.overlay_opened.is_some(), motion);
        for ms in [0, 120] {
            let start = if ms == 0 {
                Instant::now() + Duration::from_secs(1)
            } else {
                Instant::now() - Duration::from_millis(ms)
            };
            app.overlay_opened = Some(start);
            let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
            let frame = terminal
                .draw(|f| {
                    let area = f.area();
                    f.buffer_mut().set_style(area, theme.link.patch(theme.sel));
                    super::widgets::render_help(f, theme, false, app.overlay_opened);
                })
                .unwrap();
            assert_eq!(
                frame.buffer[(0, 0)].fg,
                if motion && ms == 0 {
                    theme.link.fg
                } else {
                    theme.scrim.fg
                }
                .unwrap()
            );
            assert_eq!(frame.buffer[(0, 0)].bg, ratatui::style::Color::Reset);
            assert_eq!(
                frame.buffer[position(frame.buffer, "Keys")].fg,
                theme.accent.fg.unwrap()
            );
            assert_eq!(app.has_active_motion(Instant::now()), motion && ms == 0);
        }
    }
}
