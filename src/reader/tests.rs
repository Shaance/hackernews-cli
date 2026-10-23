use super::*;
use crossterm::event::KeyCode;
use ratatui::{backend::TestBackend, Terminal};

pub(super) fn story(id: i32) -> HNCLIItem {
    HNCLIItem {
        id,
        title: "An example story".into(),
        url: String::new(),
        author: "author".into(),
        time: String::new(),
        time_ago: String::new(),
        score: 1,
        text: String::new(),
        links: vec![],
        comments: Some(2),
    }
}
pub(super) fn ready() -> Reader {
    let mut p = Reader {
        config: Config {
            api_key: "test-key".into(),
            automatic: false,
            ..Config::default()
        },
        ..Reader::default()
    };
    p.theme = Theme::TRUE_DARK;
    let mut reading = Reading::new(story(1));
    reading.theme = p.theme;
    reading.shown = true;
    reading.article.result = Some(done(
        Article {
            text: "Article claim.".into(),
        },
        Profile::Fast,
        ArticleCapture {
            title: "An example story".into(),
            text: "Captured article".into(),
            partial: false,
        },
    ));
    reading.discussion.result = Some(done(
        discussion("A concrete disagreement."),
        Profile::Fast,
        discussion_capture(),
    ));
    reading.article.capture = reading.article.result.as_ref().map(|r| r.capture.clone());
    reading.discussion.capture = reading
        .discussion
        .result
        .as_ref()
        .map(|r| r.capture.clone());
    p.readings.insert(1, reading);
    p.active = Some(1);
    p
}
fn done<T, C>(value: T, profile: Profile, input: C) -> Completed<T, C> {
    Completed {
        value,
        profile,
        capture: Arc::new(Capture {
            input,
            captured: "12:00".into(),
        }),
    }
}
pub(super) fn discussion(text: &str) -> Discussion {
    Discussion {
        overview: "Two viewpoints.".into(),
        points: vec![Point {
            text: text.into(),
            sources: vec![11],
        }],
    }
}
pub(super) fn discussion_capture() -> DiscussionCapture {
    DiscussionCapture {
        title: "An example story".into(),
        post: String::new(),
        sources: vec![
            Source {
                id: 10,
                parent: None,
                author: "parent".into(),
                text: "Parent argument.".into(),
                excerpt: "Parent argument.".into(),
            },
            Source {
                id: 11,
                parent: Some(10),
                author: "reply".into(),
                text: "Reply evidence.".into(),
                excerpt: "Reply evidence.".into(),
            },
        ],
        coverage: "2 comments".into(),
    }
}
pub(super) fn screen(
    p: &mut Reader,
    app: &mut hn_lib::app::App,
    width: u16,
    height: u16,
) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| p.render(f, app, 0)).unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect()
}
pub(super) fn comments_app() -> hn_lib::app::App {
    use hn_lib::app::{App, Comment, CommentState};
    let mut app = App::with_theme(Theme::TRUE_DARK);
    app.view_comments(1, "An example story".into(), "https://example.com".into());
    app.set_comments(
        (0..3)
            .map(|id| Comment {
                id,
                author: format!("reader{id}"),
                text: (0..30)
                    .map(|n| format!("Comment {id} line {n}\n"))
                    .collect(),
                links: vec![],
                time_ago: "now".into(),
                state: CommentState::Collapsed,
                depth: 0,
                deleted: false,
                child_ids: vec![],
            })
            .collect(),
    );
    app
}

mod reader_navigation;
mod summary_lifecycle;
mod summary_settings;
