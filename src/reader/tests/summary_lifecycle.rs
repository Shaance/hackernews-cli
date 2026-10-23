use super::*;

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
    let mut app = comments_app();
    let mut original = discussion_capture();
    original.sources[1].text = "Reply evidence.\n".repeat(60);
    original.sources[1].excerpt = original.sources[1].text.clone();
    let reading = p.readings.get_mut(&1).unwrap();
    reading.discussion.result.as_mut().unwrap().capture = Arc::new(Capture::new(original));
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
    let reading = p.readings.get_mut(&1).unwrap();
    reading.article.result.as_mut().unwrap().profile = Profile::Thorough;
    let source = screen(&mut p, &mut app, 120, 40);
    assert!(source.contains("source 1 of 1 · reply · Fast"));
    assert!(source.contains("Summary updated"));
    p.key(KeyCode::PageDown);
    screen(&mut p, &mut app, 120, 40);
    let scroll = p.readings[&1].source_scroll;
    assert!(scroll > 0);
    for key in [KeyCode::Tab, KeyCode::BackTab] {
        p.key(key);
        assert!(screen(&mut p, &mut app, 120, 40).contains("Reply evidence."));
        assert_eq!(p.readings[&1].source_scroll, scroll);
    }
    p.key(KeyCode::Esc);
    assert!(!p.readings[&1].reveal_citation);
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

#[test]
fn s_toggles_partial_results_without_work_even_when_credentials_need_repair() {
    for (article_only, credentials) in [
        (true, 0),
        (true, 1),
        (true, 2),
        (false, 0),
        (false, 1),
        (false, 2),
    ] {
        let mut p = ready();
        let r = p.readings.get_mut(&1).unwrap();
        if article_only {
            r.discussion = Section::default();
        } else {
            r.article = Section::default();
        }
        if credentials == 0 {
            p.config.api_key.clear();
        } else if credentials == 1 {
            p.auth_failed = true;
        }
        let mut app = comments_app();
        for shown in [false, true] {
            p.key(KeyCode::Char('S'));
            assert_eq!(p.readings[&1].shown, shown);
            assert!(!p.settings_open());
            let rendered = screen(&mut p, &mut app, 120, 40);
            assert!(
                !rendered.contains("M → Enter Retry"),
                "An idle section is not a failure"
            );
            assert_eq!(
                rendered.contains(if article_only {
                    "Article claim."
                } else {
                    "A concrete disagreement."
                }),
                shown
            );
        }
        assert_eq!(p.next_attempt, 0);
        assert!(p.jobs.is_empty());
    }
}

#[test]
fn revisiting_retained_results_names_actual_profiles_and_partial_updates() {
    let mut p = ready();
    p.open(story(2));
    p.config.profile = Profile::Thorough;
    p.open(story(1));
    let mut app = comments_app();
    for _ in 0..2 {
        let rendered = screen(&mut p, &mut app, 120, 40);
        assert!(rendered.contains("✓ summary · Fast"));
        assert!(rendered.contains("Fast"));
        assert!(!rendered.contains("Thorough"));
        p.key(KeyCode::Char('S'));
    }
    let reading = p.readings.get_mut(&1).unwrap();
    reading.article.result.as_mut().unwrap().profile = Profile::Thorough;
    let rendered = screen(&mut p, &mut app, 120, 40);
    assert!(rendered.contains("✓ summary · Article Thorough · Discussion Fast"));
    assert!(rendered.contains("Article Thorough · Discussion Fast"));
    assert!(!rendered.contains("✓ summary · Fast"));
    assert_eq!(p.next_attempt, 0);
    assert!(p.jobs.is_empty());
}
