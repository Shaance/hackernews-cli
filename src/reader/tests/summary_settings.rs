use super::*;

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
