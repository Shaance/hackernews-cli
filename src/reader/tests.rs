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
    let mut reading = Reading::new(story(1));
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
    let mut app = App::new();
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
}
#[tokio::test]
async fn automatic_entry_manual_mode_and_return_reuse_requests() {
    let mut p = ready();
    p.config.automatic = true;
    p.open(story(2));
    let first = match p.readings[&2].article.progress {
        Progress::Preparing(a, _, _) | Progress::Running(a) => a,
        _ => panic!("auto entry did not start"),
    };
    p.readings.get_mut(&2).unwrap().article.progress = Progress::Running(first);
    let discussion_attempt = match p.readings[&2].discussion.progress {
        Progress::Preparing(a, _, _) => a,
        _ => panic!(),
    };
    p.readings.get_mut(&2).unwrap().discussion.progress = Progress::Running(discussion_attempt);
    assert_eq!(p.jobs.len(), 2);
    p.key(KeyCode::Char('S'));
    p.key(KeyCode::Esc);
    p.open(story(2));
    assert!(matches!(p.readings[&2].article.progress,Progress::Running(a) if a==first));
    assert_eq!(p.jobs.len(), 2);
    assert!(!p.readings[&2].shown);
    p.config.automatic = false;
    p.open(story(3));
    assert!(!p.readings[&3].article.requested());
    p.key(KeyCode::Char('S'));
    assert!(p.readings[&3].article.pending());
    assert_eq!(p.jobs.len(), 4);
    p.config.automatic = true;
    p.open(story(4));
    assert!(matches!(
        p.readings[&4].article.progress,
        Progress::Queued(_)
    ));
    assert_eq!(p.jobs.len(), 4);
    p.key(KeyCode::Esc);
    assert!(matches!(p.readings[&4].article.progress, Progress::Idle));
}
#[tokio::test]
async fn refresh_survives_revisit_and_profile_switch_ignores_old_completion() {
    let mut p = ready();
    p.config.automatic = true;
    p.request(Request::Refresh);
    let old = match p.readings[&1].article.progress {
        Progress::Preparing(a, _, _) | Progress::Running(a) => a,
        _ => panic!(),
    };
    p.readings.get_mut(&1).unwrap().article.capture = p.readings[&1]
        .article
        .result
        .as_ref()
        .map(|r| r.capture.clone());
    p.readings.get_mut(&1).unwrap().article.progress = Progress::Running(old);
    p.key(KeyCode::Esc);
    p.open(story(1));
    assert!(matches!(p.readings[&1].article.progress,Progress::Running(a) if a==old));
    p.config.profile = Profile::Thorough;
    p.request(Request::Generate);
    let new = match p.readings[&1].article.progress {
        Progress::Preparing(a, _, _) | Progress::Running(a) => a,
        _ => panic!(),
    };
    p.readings.get_mut(&1).unwrap().article.progress = Progress::Running(new);
    p.complete(jobs::Completion {
        story: 1,
        attempt: old,
        output: jobs::Output::Article(Ok(Article {
            text: "stale".into(),
        })),
    });
    assert_eq!(
        p.readings[&1].article.result.as_ref().unwrap().value.text,
        "Article claim."
    );
    p.complete(jobs::Completion {
        story: 1,
        attempt: new,
        output: jobs::Output::Article(Err(Failure::new("Unavailable"))),
    });
    assert!(p.readings[&1].article.failed());
    assert_eq!(
        p.readings[&1].article.result.as_ref().unwrap().profile,
        Profile::Fast
    );
    let count = p.next_attempt;
    p.request(Request::Automatic);
    assert_eq!(p.next_attempt, count, "revisit must not retry a failure");
    p.request(Request::Retry);
    assert!(p.next_attempt > count);
}
#[test]
fn source_snapshot_and_parent_publish_together_on_return() {
    let mut p = ready();
    p.key(KeyCode::Tab);
    p.key(KeyCode::Enter);
    let attempt = Attempt {
        id: 42,
        profile: Profile::Thorough,
        automatic: false,
        refresh: false,
    };
    p.readings.get_mut(&1).unwrap().discussion.progress = Progress::Running(attempt);
    let next = discussion("Replacement");
    let mut capture = discussion_capture();
    capture.sources[1].text = "New captured text".into();
    capture.sources[1].excerpt = "New captured text".into();
    p.readings.get_mut(&1).unwrap().discussion.capture = Some(Arc::new(Capture::new(capture)));
    p.complete(jobs::Completion {
        story: 1,
        attempt,
        output: jobs::Output::Discussion(Ok(next)),
    });
    assert!(p.readings[&1]
        .source_lines(11, 80)
        .iter()
        .any(|l| l.to_string().contains("Reply evidence.")));
    p.key(KeyCode::Esc);
    assert!(!p.readings[&1].citation_focus);
    assert!(!p.key(KeyCode::Enter));
    assert_eq!(
        p.readings[&1]
            .discussion
            .result
            .as_ref()
            .unwrap()
            .capture
            .input
            .sources[1]
            .text,
        "New captured text"
    );
}
#[tokio::test]
async fn setup_cancel_save_and_rejected_key_repair_resume_same_story() {
    let mut p = ready();
    p.config = Config::default();
    p.readings.insert(1, Reading::new(story(1)));
    let dir = std::env::temp_dir().join(format!("hn-settings-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    p.config_path = Some(dir.join("summary.json"));
    p.key(KeyCode::Char('S'));
    assert!(p.settings_open());
    p.key(KeyCode::Esc);
    assert!(!p.readings[&1].article.requested());
    p.key(KeyCode::Char('S'));
    for c in "test-key".chars() {
        p.key(KeyCode::Char(c));
    }
    p.key(KeyCode::Enter);
    assert!(!p.settings_open());
    assert_eq!(p.active, Some(1));
    assert_eq!(p.jobs.len(), 2);
    let attempt = match p.readings[&1].article.progress {
        Progress::Preparing(a, _, _) | Progress::Running(a) => a,
        _ => panic!(),
    };
    p.readings.get_mut(&1).unwrap().article.progress = Progress::Running(attempt);
    p.complete(jobs::Completion {
        story: 1,
        attempt,
        output: jobs::Output::Article(Err(Failure {
            message: "bad key".into(),
            auth: true,
        })),
    });
    p.key(KeyCode::Char('M'));
    p.key(KeyCode::Delete);
    for c in "replacement-key".chars() {
        p.key(KeyCode::Char(c));
    }
    p.key(KeyCode::Enter);
    assert!(!p.auth_failed);
    assert!(p.readings[&1].article.pending());
    let saved = std::fs::read_to_string(p.config_path.as_ref().unwrap()).unwrap();
    assert!(saved.contains("replacement-key"));
    assert!(!saved.contains("environment_key"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(p.config_path.as_ref().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn missing_key_and_save_failure_never_block_reading_or_expose_key() {
    let mut p = ready();
    p.config = Config::default();
    p.open(story(5));
    assert!(!p.readings[&5].article.requested());
    let mut app = comments_app();
    assert!(screen(&mut p, &mut app, 120, 40).contains("Set up summaries"));
    p.config_path = None;
    p.key(KeyCode::Char('M'));
    for c in "secret-value".chars() {
        p.key(KeyCode::Char(c));
    }
    p.key(KeyCode::Enter);
    let output = screen(&mut p, &mut app, 80, 24);
    assert!(!output.contains("secret-value"));
    assert!(p.settings_open());
    p.key(KeyCode::Esc);
    assert!(!p.key(KeyCode::Char('j')));
}
#[test]
fn extraction_discards_markup_and_control_characters() {
    let paragraph =
        "This article explains a specific engineering tradeoff, with evidence and examples. "
            .repeat(30);
    let html=format!("<html><head><title>Story</title></head><body><nav>MENU HOME LOGIN</nav><article><h1>Story</h1><p>{paragraph}</p><script>secretNoise()</script></article></body></html>");
    let text = sources::extract(
        html.as_bytes(),
        &reqwest::Url::parse("https://example.com").unwrap(),
    )
    .unwrap();
    assert!(text.contains("engineering tradeoff"));
    assert!(!text.contains("<p>"));
    assert!(!text.contains("secretNoise"));
    assert_eq!(sources::clean("safe\u{1b}[31m\ntext"), "safe[31m\ntext");
}

#[tokio::test]
async fn abandoning_preparation_never_submits_and_retry_does_not_start_manual_work() {
    let mut p = ready();
    p.config.automatic = true;
    p.open(story(2));
    let attempt = match p.readings[&2].article.progress {
        Progress::Preparing(a, _, _) => a,
        _ => panic!(),
    };
    p.key(KeyCode::Esc);
    assert!(matches!(p.readings[&2].article.progress, Progress::Idle));
    // Even a prepared result already in the completion queue cannot submit now.
    p.complete(jobs::Completion {
        story: 2,
        attempt,
        output: jobs::Output::ArticleSource(Ok(Capture::new(ArticleCapture {
            title: "Story".into(),
            text: "Text".into(),
            partial: false,
        }))),
    });
    assert!(matches!(p.readings[&2].article.progress, Progress::Idle));
    while let Some(result) = p.jobs.join_next().await {
        assert!(matches!(result, Err(error) if error.is_cancelled()));
    }
    p.config.automatic = false;
    p.open(story(3));
    p.key(KeyCode::Char('r'));
    assert!(!p.readings[&3].article.requested());
    assert!(p.jobs.is_empty());
}
#[tokio::test]
async fn disabling_auto_cancels_unsubmitted_work_and_preserves_manual_work() {
    let mut p = ready();
    let dir = std::env::temp_dir().join(format!("hn-auto-test-{}", std::process::id()));
    p.config_path = Some(dir.join("summary.json"));
    p.config.automatic = true;
    p.open(story(2));
    p.key(KeyCode::Char('M'));
    p.key(KeyCode::Char(' '));
    p.key(KeyCode::Enter);
    assert!(!p.readings[&2].article.pending());
    assert!(!p.config.automatic);
    while let Some(result) = p.jobs.join_next().await {
        assert!(matches!(result, Err(error) if error.is_cancelled()));
    }
    p.key(KeyCode::Char('S'));
    assert!(p.readings[&2].article.pending());
    p.key(KeyCode::Char('M'));
    p.key(KeyCode::Enter);
    assert!(p.readings[&2].article.pending());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn manual_generation_fills_missing_section_without_replacing_existing_result() {
    let mut p = ready();
    p.readings.get_mut(&1).unwrap().discussion = Section::default();
    p.config.profile = Profile::Thorough;
    p.key(KeyCode::Char('S'));
    assert!(p.readings[&1].discussion.pending());
    assert!(!p.readings[&1].article.pending());
    assert_eq!(
        p.readings[&1].article.result.as_ref().unwrap().profile,
        Profile::Fast
    );
}
#[tokio::test]
async fn changing_profile_while_disabling_auto_still_upgrades_open_story() {
    let mut p = ready();
    p.config.automatic = true;
    p.open(story(2));
    let dir = std::env::temp_dir().join(format!("hn-profile-test-{}", std::process::id()));
    p.config_path = Some(dir.join("summary.json"));
    p.key(KeyCode::Char('M'));
    p.key(KeyCode::Char(' '));
    p.key(KeyCode::Tab);
    p.key(KeyCode::Char(' '));
    p.key(KeyCode::Enter);
    assert!(!p.config.automatic);
    assert_eq!(p.config.profile, Profile::Thorough);
    assert!(
        matches!(p.readings[&2].article.progress, Progress::Queued(a) | Progress::Preparing(a, _, _) if a.profile == Profile::Thorough && !a.automatic)
    );
    std::fs::remove_dir_all(dir).unwrap();
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
