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

use crate::reader::config::Provider;
use serde_json::{json, Value};

#[tokio::test]
async fn provider_selection_keeps_keys_separate_and_completed_reading_intact() {
    let mut p = ready();
    p.config.api_key.clear();
    p.config.environment_key = "router-env-secret".into();
    p.config.anthropic_environment_key = "native-env-secret".into();
    let dir = std::env::temp_dir().join(format!("hn-provider-settings-{}", std::process::id()));
    let path = dir.join("summary.json");
    p.config_path = Some(path.clone());
    let capture = p.readings[&1].article.capture.clone().unwrap();
    let mut app = comments_app();
    p.key(KeyCode::Char('M'));
    p.key(KeyCode::Up); // key
    p.key(KeyCode::BackTab); // provider
    p.key(KeyCode::Char(' '));
    let rendered = screen(&mut p, &mut app, 80, 24);
    assert!(rendered.contains("Provider: Anthropic"));
    assert!(rendered.contains("Anthropic key:"));
    assert!(!rendered.contains("env-secret"));
    p.key(KeyCode::Tab); // native key
    p.key(KeyCode::Delete);
    for c in "draft-key".chars() {
        p.key(KeyCode::Char(c));
    }
    p.key(KeyCode::Esc);
    assert_eq!(p.config.provider, Provider::OpenRouter);
    assert_eq!(p.config.key_for(Provider::Anthropic), "native-env-secret");
    assert!(!path.exists());

    p.key(KeyCode::Char('M'));
    p.key(KeyCode::Up);
    p.key(KeyCode::BackTab);
    p.key(KeyCode::Char(' '));
    p.key(KeyCode::Enter);
    assert_eq!(p.config.provider, Provider::Anthropic);
    assert_eq!(p.config.key(), "native-env-secret");
    assert_eq!(
        p.jobs.len(),
        0,
        "Switching billing provider alone must not charge for cached results"
    );
    assert!(Arc::ptr_eq(
        &capture,
        p.readings[&1].article.capture.as_ref().unwrap()
    ));
    assert_eq!(
        p.readings[&1].article.result.as_ref().unwrap().value.text,
        "Article claim."
    );
    let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["provider"], "anthropic");
    assert_eq!(saved["api_key"], "");
    assert_eq!(saved["anthropic_api_key"], "");
    assert!(!saved.to_string().contains("secret"));
    let loaded = Config::load(&path).unwrap();
    assert_eq!(loaded.provider, Provider::Anthropic);

    p.key(KeyCode::Char('M'));
    p.key(KeyCode::Up);
    p.key(KeyCode::Delete);
    for c in "saved-native-key".chars() {
        p.key(KeyCode::Char(c));
    }
    p.key(KeyCode::Enter);
    assert_eq!(p.config.key(), "saved-native-key");
    assert_eq!(p.config.key_for(Provider::OpenRouter), "router-env-secret");
    let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["anthropic_api_key"], "saved-native-key");
    assert_eq!(saved["api_key"], "");
    let old: Config = serde_json::from_value(
        json!({"api_key":"old-router-key","automatic":false,"profile":"thorough"}),
    )
    .unwrap();
    assert_eq!(old.provider, Provider::OpenRouter);
    assert_eq!(old.key(), "old-router-key");
    assert_eq!(old.profile, Profile::Thorough);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn switching_provider_reuses_captures_and_ignores_old_pending_completion() {
    let (network, mut requests, server) = network::tests::serve(|path, body, headers| {
        assert_eq!(
            path, "/messages",
            "Only native inference, no source refetch or OpenRouter call"
        );
        assert!(headers.contains("Bearer native-key"));
        let input: Value =
            serde_json::from_str(body["messages"][0]["content"].as_str().unwrap()).unwrap();
        let output = if input.get("article").is_some() {
            json!({"summary":"Native article"})
        } else {
            json!({"overview":"Context","points":[{"text":"Native discussion","sources":[11]}]})
        };
        (
            200,
            json!({"content":[{"type":"text","text":output.to_string()}]}).to_string(),
        )
    })
    .await;
    let mut p = ready();
    p.network = network;
    p.config.anthropic_api_key = "native-key".into();
    let dir = std::env::temp_dir().join(format!("hn-provider-pending-{}", std::process::id()));
    p.config_path = Some(dir.join("summary.json"));
    let old = Attempt {
        id: 100,
        profile: Profile::Thorough,
        automatic: false,
        refresh: false,
    };
    p.next_attempt = 100;
    p.readings.get_mut(&1).unwrap().article.progress = Progress::Running(old);
    p.key(KeyCode::Char('M'));
    p.key(KeyCode::Tab); // model, select Thorough to replace Fast from retained input
    p.key(KeyCode::Char(' '));
    p.key(KeyCode::Tab); // refresh
    p.key(KeyCode::Tab); // provider
    p.key(KeyCode::Char(' '));
    p.key(KeyCode::Enter);
    p.complete(jobs::Completion {
        story: 1,
        attempt: old,
        output: jobs::Output::Article(Err(Failure {
            message: "old key".into(),
            auth: true,
        })),
    });
    assert!(
        !p.auth_failed,
        "An old provider response must not reject the current key"
    );
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !p.jobs.is_empty() {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            p.poll();
        }
    })
    .await
    .unwrap();
    let mut sent = Vec::new();
    while let Ok(request) = requests.try_recv() {
        sent.push(request);
    }
    assert_eq!(sent.len(), 2);
    assert!(sent.iter().all(|(_, b)| b["model"] == "claude-sonnet-5-5"));
    assert_eq!(
        p.readings[&1].article.result.as_ref().unwrap().value.text,
        "Native article"
    );
    assert_eq!(
        p.readings[&1]
            .discussion
            .result
            .as_ref()
            .unwrap()
            .value
            .points[0]
            .text,
        "Native discussion"
    );
    p.open(story(1));
    assert!(requests.try_recv().is_err());
    assert!(p.jobs.is_empty());
    server.abort();
    std::fs::remove_dir_all(dir).unwrap();
}
