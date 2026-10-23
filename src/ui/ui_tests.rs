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
    terminal.draw(|f| render(f, &mut app, 0)).unwrap();
    let text = buffer_text(terminal.backend().buffer());
    for shortcut in ["PgUp/PgDn", "Save/unsave", "Esc/q/h/← Back", "? Close help"] {
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
        .draw(|f| super::comments::render_reader_content(f, f.area(), &mut app, 0, vec![]))
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
