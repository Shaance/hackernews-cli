use super::render;
use crate::app::{App, Comment, CommentState};
use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};

fn test_comment(id: i32, text: &str) -> Comment {
    Comment {
        id,
        author: format!("user{}", id),
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
