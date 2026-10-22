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
