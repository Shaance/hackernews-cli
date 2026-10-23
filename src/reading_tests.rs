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
    app.apply_comment_error("Refresh failed".into());
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
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let frame = terminal.draw(|f| ui::render(f, &mut restarted, 0)).unwrap();
    let buffer = frame.buffer;
    let saved_screen: String = buffer.content().iter().map(|cell| cell.symbol()).collect();
    assert!(saved_screen.contains("4 Saved"));
    for x in 33..38 {
        assert_eq!(buffer[(x, 0)].fg, restarted.theme().accent.fg.unwrap());
        assert!(buffer[(x, 0)]
            .modifier
            .contains(ratatui::style::Modifier::BOLD | ratatui::style::Modifier::UNDERLINED));
    }
    assert!(saved_screen.contains("s save") && saved_screen.contains("? keys"));
    assert!(!saved_screen.contains('★') && !saved_screen.contains("n/p"));
    assert!(render(&mut restarted, 80, 24).contains("Ask HN: Reading"));
    restarted.view_comments(1, "Ask HN: Reading".into(), "https://example.com".into());
    restarted.set_comments(vec![]);
    bookmarks::toggle(&mut restarted, Some(&path)).unwrap();
    assert!(bookmarks::load(&path).unwrap().is_empty());
    // Unsave from Saved while reading, then save again without losing the current story.
    bookmarks::toggle(&mut restarted, Some(&path)).unwrap();
    assert_eq!(bookmarks::load(&path).unwrap()[0].text, story().text);
    restarted.view_stories();
    let frame = terminal.draw(|f| ui::render(f, &mut restarted, 0)).unwrap();
    let text: String = frame.buffer.content.iter().map(|c| c.symbol()).collect();
    assert!(text.contains("Ask HN: Reading") && restarted.is_bookmarked(1));
    assert!(!text.contains('★') && !text.contains("n/p"));
    for displayed in [StoryType::Best, StoryType::Saved] {
        restarted.set_stories_for(displayed, 1, vec![story()]);
        // Render by the displayed feed even while another feed is requested.
        restarted.set_story_type(if displayed == StoryType::Saved {
            StoryType::Best
        } else {
            StoryType::Saved
        });
        let frame = terminal.draw(|f| ui::render(f, &mut restarted, 0)).unwrap();
        let text: String = frame.buffer.content.iter().map(|c| c.symbol()).collect();
        assert!(text.contains("Ask HN: Reading"));
        assert_eq!(text.contains('★'), displayed == StoryType::Best);
        assert_eq!(text.contains("n/p page"), displayed == StoryType::Best);
    }
    restarted.set_story_type(StoryType::Saved);
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

#[test]
fn feed_positions_follow_successful_pages_and_current_bookmarks() {
    let mut app = App::new();
    let stories: Vec<_> = (1..=30).map(|id| HNCLIItem { id, ..story() }).collect();
    app.set_story_type(StoryType::Top);
    app.current_page = 3;
    app.apply_stories_page(StoryType::Top, 3, stories.clone());
    app.selected_index = 25;
    render(&mut app, 200, 60);
    let scroll = app.story_scroll;
    assert!(
        scroll > 0,
        "retain the offset chosen by the rendered multi-line list"
    );
    app.set_story_type(StoryType::New);
    assert_eq!((app.selected_index, app.story_scroll), (25, scroll));
    app.apply_stories_error("unavailable".into());
    app.bookmarks = stories[..3].to_vec();
    app.set_story_type(StoryType::Saved);
    app.show_saved_stories();
    app.next_story();
    app.set_story_type(StoryType::Top);
    assert_eq!(app.current_page, 3);
    app.set_stories_for(StoryType::Top, 3, app.cached_stories().unwrap());
    assert_eq!(app.selected_story().unwrap().id, 26);
    assert_eq!(app.story_scroll, scroll);
    app.bookmarks.insert(0, stories[10].clone());
    app.set_story_type(StoryType::Saved);
    app.show_saved_stories();
    assert_eq!(app.stories[0].id, 11);
    assert_eq!(app.selected_story().unwrap().id, 2);
    app.bookmarks.retain(|s| s.id != 2);
    app.show_saved_stories();
    assert_eq!(app.selected_story().unwrap().id, 3);
    app.set_story_type(StoryType::Top);
    app.set_stories_for(StoryType::Top, 3, app.cached_stories().unwrap());
    app.next_page();
    assert_eq!(app.selected_story().unwrap().id, 26);
    app.set_story_type(StoryType::Saved);
    app.show_saved_stories();
    app.set_story_type(StoryType::Top);
    assert_eq!(app.current_page, 3, "an uncompleted page is not remembered");
}

#[test]
fn refresh_keeps_the_current_visible_reply_separate_from_selection() {
    for (width, height) in [(200, 60), (240, 80)] {
        let body = (0..400)
            .map(|n| format!("passage-{n:03}\n"))
            .collect::<String>();
        let reply = comment(11, body, CommentState::Collapsed);
        let parent = comment(
            10,
            "parent".into(),
            CommentState::Expanded {
                children: vec![reply.clone()],
            },
        );
        let mut app = reader(vec![parent]);
        render(&mut app, width, height);
        app.scroll_comment_page(true);
        render(&mut app, width, height);
        app.begin_comment_refresh();
        app.scroll_comment_page(true);
        render(&mut app, width, height);
        let scroll = app.comment_scroll;
        app.scroll_comment_page(true);
        let expected = render(&mut app, width, height);
        let marker = |s: &str| s.split("passage-").nth(1).unwrap()[..3].to_owned();
        app.set_comment_position(0, scroll);
        app.scroll_comment_page(true); // input immediately before completion, without a redraw
        let fresh = vec![
            comment(99, "inserted root".repeat(80), CommentState::Collapsed),
            comment(
                10,
                "parent".into(),
                CommentState::Expanded {
                    children: vec![
                        comment(12, "inserted reply".repeat(80), CommentState::Collapsed),
                        reply,
                    ],
                },
            ),
        ];
        app.set_comments(fresh);
        let refreshed = render(&mut app, width, height);
        assert_eq!(marker(&refreshed), marker(&expected));
        assert_eq!(app.visible_comment_at(app.comment_cursor).unwrap().1.id, 10);
        app.scroll_comment_page(true);
        assert_ne!(marker(&render(&mut app, width, height)), marker(&refreshed));
        app.begin_comment_refresh();
        let mut shortened = app.comments.clone();
        if let CommentState::Expanded { children } = &mut shortened[1].state {
            children[1].text = "shortened reply".into();
            children.push(comment(
                13,
                "following-reply".into(),
                CommentState::Collapsed,
            ));
        }
        app.set_comments(shortened);
        assert!(
            render(&mut app, width, height).contains("following-reply"),
            "clamp the offset inside a shortened comment"
        );
        app.begin_comment_refresh();
        let mut fallback = vec![
            comment(
                99,
                format!("neighbor-start\n{}", "neighbor tail\n".repeat(200)),
                CommentState::Collapsed,
            ),
            comment(
                10,
                "parent".into(),
                CommentState::Expanded { children: vec![] },
            ),
        ];
        if width == 240 {
            fallback.pop();
        }
        app.set_comments(fallback);
        assert!(
            render(&mut app, width, height).contains(if width == 240 {
                "neighbor-start"
            } else {
                "parent"
            }),
            "use the surviving parent or nearest neighbor"
        );
    }
}

#[test]
fn refresh_merges_expansion_choices_made_during_loading() {
    for mode in 0..3 {
        let child = comment(11, "current reply".into(), CommentState::Collapsed);
        let mut app = reader(vec![comment(
            10,
            "parent".into(),
            CommentState::Expanded {
                children: vec![child.clone()],
            },
        )]);
        render(&mut app, 200, 60);
        app.begin_comment_refresh();
        app.collapse_current_thread();
        assert!(
            app.is_loading(),
            "collapsing a branch cannot finish the whole refresh"
        );
        if mode == 1 {
            app.comments[0].state = CommentState::Loading { generation: 42 };
        }
        if mode == 2 {
            app.comments[0].state = CommentState::Expanded {
                children: vec![child],
            };
        }
        app.rebuild_visible_comments();
        let fresh_state = if mode == 2 {
            CommentState::Collapsed
        } else {
            CommentState::Expanded {
                children: vec![comment(12, "fresh reply".into(), CommentState::Collapsed)],
            }
        };
        app.set_comments(vec![comment(10, "updated parent".into(), fresh_state)]);
        match mode {
            0 => assert!(matches!(app.comments[0].state, CommentState::Collapsed)),
            1 => {
                assert!(matches!(
                    app.comments[0].state,
                    CommentState::Loading { generation: 42 }
                ));
                assert!(app.is_loading());
                app.begin_comment_refresh();
                app.comments[0].state = CommentState::Loading { generation: 43 };
                app.apply_comment_error("refresh failed".into());
                assert!(app.is_loading());
                assert!(app.replace_loading_comment_state(
                    10,
                    43,
                    CommentState::Expanded { children: vec![] }
                ));
                app.recompute_comment_loading(app.has_loading_comments());
                assert!(!app.is_loading());
                assert_eq!(app.error(), Some("refresh failed"));
            }
            _ => assert_eq!(app.visible_comment_at(1).unwrap().1.text, "current reply"),
        }
    }
}
