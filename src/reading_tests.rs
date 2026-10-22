use crate::{
    app::{App, Comment, CommentState, StoryType},
    bookmarks, ui, HNCLIItem,
};
use ratatui::{backend::TestBackend, Terminal};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

fn story() -> HNCLIItem {
    HNCLIItem {
        id: 1,
        title: "Ask HN: Reading".into(),
        url: "https://example.com".into(),
        author: "author".into(),
        time: "now".into(),
        time_ago: "now".into(),
        score: 5,
        comments: Some(1),
        text: "Original question\n\n    code block".into(),
        links: vec!["https://example.com/post-link".into()],
    }
}

fn comment(id: i32, text: String, state: CommentState) -> Comment {
    Comment {
        id,
        author: "reader".into(),
        text,
        links: vec!["https://example.com/comment-link".into()],
        time_ago: "now".into(),
        state,
        depth: 0,
        deleted: false,
        child_ids: vec![id + 1],
    }
}

fn reader(comments: Vec<Comment>) -> App {
    let mut app = App::new();
    app.set_stories(vec![story()]);
    app.view_comments(1, "Ask HN: Reading".into(), "https://example.com".into());
    app.set_comments(comments);
    app
}

fn render(app: &mut App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| ui::render(f, app, 0)).unwrap();
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

#[test]
fn reader_pages_through_every_line_and_restores_the_same_screen() {
    let body = (0..60)
        .map(|n| format!("reading-line-{n:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut app = reader(vec![comment(10, body, CommentState::Collapsed)]);
    let first = render(&mut app, 80, 24);
    assert!(first.contains("Original question"));
    assert!(first.contains("    code block"));
    assert!(first.contains("reading-line-00"));
    app.scroll_comment_page(true);
    let middle = render(&mut app, 80, 24);
    assert!(!middle.contains("reading-line-00"));
    assert!(middle.contains("reading-line-25"));
    assert_eq!(
        middle,
        render(&mut app, 80, 24),
        "redraw must not undo paging"
    );
    let offset = app.comment_scroll;
    app.view_stories();
    app.view_comments(1, "Ask HN: Reading".into(), "https://example.com".into());
    assert_eq!(app.comment_scroll, offset);
    assert_eq!(middle, render(&mut app, 80, 24));
    app.scroll_comment_page(true);
    app.scroll_comment_page(true);
    assert!(render(&mut app, 80, 24).contains("reading-line-59"));
    app.scroll_comment_page(false);
    assert!(!render(&mut app, 80, 24).contains("reading-line-59"));
    app.first_comment();
    assert_eq!(first, render(&mut app, 80, 24));
}

#[test]
fn reopened_discussion_restores_expanded_branch_and_selection() {
    let child = comment(11, "child".into(), CommentState::Collapsed);
    let mut app = reader(vec![comment(
        10,
        "parent".into(),
        CommentState::Expanded {
            children: vec![child],
        },
    )]);
    app.next_comment();
    render(&mut app, 80, 24);
    app.view_stories();
    app.view_comments(1, "Ask HN: Reading".into(), "https://example.com".into());
    assert!(!app.is_loading());
    assert_eq!(app.visible_comment_count(), 2);
    assert_eq!(app.visible_comment_at(app.comment_cursor).unwrap().1.id, 11);
    assert!(render(&mut app, 80, 24).contains("child"));
}

#[test]
fn interrupted_reply_load_can_be_expanded_again_after_return() {
    let mut app = reader(vec![comment(
        10,
        "parent".into(),
        CommentState::Loading { generation: 1 },
    )]);
    app.view_stories();
    app.view_comments(1, "Ask HN: Reading".into(), "https://example.com".into());
    assert!(matches!(
        app.selected_comment_mut().unwrap().state,
        CommentState::Collapsed
    ));
    assert!(!app.is_loading());
    let mut fresh = App::new();
    fresh.view_comments(1, "story".into(), "https://example.com".into());
    fresh.view_stories();
    fresh.view_comments(1, "story".into(), "https://example.com".into());
    assert!(fresh.is_loading(), "interrupted initial fetch must retry");

    app.selected_comment_mut().unwrap().state = CommentState::Loading { generation: 2 };
    app.begin_comment_refresh();
    app.set_comment_error("Refresh failed".into());
    assert!(matches!(
        app.selected_comment_mut().unwrap().state,
        CommentState::Collapsed
    ));
    assert!(!app.replace_loading_comment_state(10, 2, CommentState::Expanded { children: vec![] }));
}

#[test]
fn post_without_replies_is_readable_and_links_are_available() {
    let mut app = reader(vec![]);
    assert!(render(&mut app, 80, 24).contains("Original question"));
    app.show_links = true;
    assert!(render(&mut app, 80, 24).contains("https://example.com/post-link"));
    app.show_links = false;
    app.set_comments(vec![comment(
        10,
        "named link".into(),
        CommentState::Collapsed,
    )]);
    assert_eq!(
        app.current_links(),
        vec![
            "https://example.com/post-link",
            "https://example.com/comment-link"
        ]
    );
    app.show_links = true;
    assert!(render(&mut app, 80, 24).contains("https://example.com/comment-link"));
}

struct TempFolder(PathBuf);
impl TempFolder {
    fn new() -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "hn-reading-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for TempFolder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn bookmarks_toggle_in_both_views_and_survive_reload() {
    let folder = TempFolder::new();
    let path = folder.0.join("saved.json");
    let mut app = App::new();
    app.set_stories(vec![story()]);
    bookmarks::toggle(&mut app, Some(&path)).unwrap();
    assert!(render(&mut app, 80, 24).contains("★"));
    let stored: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(stored[0]["id"], 1);
    let mut restarted = App::new();
    restarted.bookmarks = bookmarks::load(&path).unwrap();
    restarted.set_story_type(StoryType::Saved);
    restarted.show_saved_stories();
    let saved_screen = render(&mut restarted, 80, 24);
    assert!(saved_screen.contains("Saved stories"));
    assert!(saved_screen.contains("s save") && saved_screen.contains("? help"));
    assert!(render(&mut restarted, 80, 24).contains("Ask HN: Reading"));
    restarted.view_comments(1, "Ask HN: Reading".into(), "https://example.com".into());
    restarted.set_comments(vec![]);
    bookmarks::toggle(&mut restarted, Some(&path)).unwrap();
    assert!(bookmarks::load(&path).unwrap().is_empty());
    // Unsave from Saved while reading, then save again without losing the current story.
    bookmarks::toggle(&mut restarted, Some(&path)).unwrap();
    assert_eq!(bookmarks::load(&path).unwrap()[0].text, story().text);
    restarted.view_stories();
    assert!(render(&mut restarted, 80, 24).contains("★"));
    bookmarks::toggle(&mut restarted, Some(&path)).unwrap();
    assert!(render(&mut restarted, 80, 24).contains("No saved stories yet"));
}

#[test]
fn failed_bookmark_write_preserves_memory_and_unreadable_file() {
    let folder = TempFolder::new();
    let path = folder.0.join("saved.json");
    let mut app = App::new();
    app.set_stories(vec![story()]);
    fs::write(&path, b"unreadable bookmark data").unwrap();
    assert!(bookmarks::load(&path).is_err());
    assert!(bookmarks::toggle(&mut app, None).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"unreadable bookmark data");
    let blocker = folder.0.join("blocker");
    fs::write(&blocker, b"a file, not a directory").unwrap();
    assert!(bookmarks::toggle(&mut app, Some(&blocker.join("saved.json"))).is_err());
    assert!(app.bookmarks.is_empty());
}
