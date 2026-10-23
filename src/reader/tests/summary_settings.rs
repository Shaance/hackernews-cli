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
    for changed_key in [false, true] {
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
        p.key(KeyCode::Tab);
        p.key(KeyCode::Enter);
        assert!(
            p.auth_failed,
            "ordinary settings save is not a credential repair"
        );
        assert!(p.readings[&1].article.failed());
        p.key(KeyCode::Char('M'));
        if changed_key {
            p.key(KeyCode::Delete);
            for c in "replacement-key".chars() {
                p.key(KeyCode::Char(c));
            }
        }
        p.key(KeyCode::Enter);
        assert!(!p.auth_failed);
        assert!(p.readings[&1].article.pending());
    }
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
    let rendered = screen(&mut p, &mut app, 120, 40);
    assert!(rendered.contains("S summary") && !rendered.contains("Set up"));
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
    assert!(rendered.chars().collect::<Vec<_>>().chunks(80).any(|row| {
        let row = row.iter().collect::<String>();
        row.contains("Provider") && row.contains("Anthropic")
    }));
    assert!(rendered.contains("Anthropic key"));
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

#[tokio::test]
async fn comment_refresh_is_free_and_explicit_summary_retry_only_retries_failures() {
    for failed in [1, 2, 3] {
        let (network, mut requests, server) = network::tests::serve(|path, body, _| {
            let output = if path == "/chat/completions" {
                let input: Value = serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
                let content = if input.get("article").is_some() { json!({"summary":"Retried article"}) }
                    else { json!({"overview":"Context","points":[{"text":"Retried discussion","sources":[11]}]}) };
                json!({"choices":[{"message":{"content":content.to_string()}}]})
            } else if path.ends_with("/1.json") {
                json!({"id":1,"time":0,"type":"story","kids":[10]})
            } else { json!({"id":10,"time":0,"type":"comment","text":"Fresh readable comment"}) };
            (200, output.to_string())
        }).await;
        let mut p = ready();
        p.network = network;
        p.config.automatic = true;
        let folder =
            std::env::temp_dir().join(format!("hn-explicit-retry-{}-{failed}", std::process::id()));
        p.config_path = Some(folder.join("summary.json"));
        let attempt = Attempt {
            id: 50,
            profile: Profile::Fast,
            automatic: false,
            refresh: false,
        };
        let r = p.readings.get_mut(&1).unwrap();
        r.shown = failed == 1;
        if failed & 1 != 0 {
            r.article.progress = Progress::Failed(attempt, Failure::new("failed"));
        }
        if failed & 2 != 0 {
            r.discussion.progress = Progress::Failed(attempt, Failure::new("failed"));
        }
        let article_capture = r.article.capture.clone().unwrap();
        let discussion_capture = r.discussion.capture.clone().unwrap();
        let service = p.network.service.clone();
        let mut app = comments_app();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut bookmark_path = None;
        let keys = [
            KeyCode::Char('r'),
            KeyCode::Char('M'),
            KeyCode::Up,
            KeyCode::Up,
            KeyCode::Up,
            KeyCode::Enter, // ordinary settings save, no retry
            KeyCode::Char('M'),
            KeyCode::Up,
            KeyCode::Up,
            KeyCode::Char(' '),
            KeyCode::Down,
            KeyCode::Down,
            KeyCode::Enter,
        ]; // change preference but explicitly retry only failures
        for (step, key) in keys.into_iter().enumerate() {
            crate::reader_actions::handle(
                &mut p,
                &mut app,
                key,
                tx.clone(),
                service.clone(),
                &mut bookmark_path,
            )
            .await
            .unwrap();
            if step == 0 {
                let msg = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
                    .await
                    .unwrap()
                    .unwrap();
                crate::handle_app_message(&mut app, msg);
                assert_eq!(app.comments[0].text, "Fresh readable comment");
                assert!(p.jobs.is_empty());
                let mut count = 0;
                while let Ok((path, _)) = requests.try_recv() {
                    assert!(path.starts_with("/v0/item/"));
                    count += 1;
                }
                assert_eq!(count, 2, "r fetched only the story and visible root");
            }
            if step == 1 {
                assert!(screen(&mut p, &mut app, 240, 80).contains("▎ Retry failed summary"));
            }
            if step == 5 {
                assert!(p.jobs.is_empty());
                assert!(requests.try_recv().is_err());
            }
        }
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while let Some(result) = p.jobs.join_next().await {
                p.complete(result.unwrap());
            }
        })
        .await
        .unwrap();
        let r = &p.readings[&1];
        assert_eq!(
            r.article.result.as_ref().unwrap().value.text,
            if failed & 1 != 0 {
                "Retried article"
            } else {
                "Article claim."
            }
        );
        assert_eq!(
            r.discussion.result.as_ref().unwrap().value.points[0].text,
            if failed & 2 != 0 {
                "Retried discussion"
            } else {
                "A concrete disagreement."
            }
        );
        assert!(Arc::ptr_eq(
            &article_capture,
            r.article.capture.as_ref().unwrap()
        ));
        assert!(Arc::ptr_eq(
            &discussion_capture,
            r.discussion.capture.as_ref().unwrap()
        ));
        let mut count = 0;
        while let Ok((path, body)) = requests.try_recv() {
            assert_eq!(path, "/chat/completions");
            assert_eq!(body["model"], Profile::Thorough.model(Provider::OpenRouter));
            count += 1;
        }
        assert_eq!(count, (failed as u32).count_ones());
        server.abort();
        std::fs::remove_dir_all(folder).unwrap();
    }
}

#[tokio::test]
async fn refresh_fetches_expanded_branches_and_keeps_the_whole_tree_on_item_failure() {
    use hn_lib::app::CommentState;
    for fail_id in [0, 10, 11] {
        let (network, mut requests, server) = network::tests::serve(move |path, _, _| {
            let id: i32 = path
                .rsplit('/')
                .next()
                .unwrap()
                .trim_end_matches(".json")
                .parse()
                .unwrap();
            if id == fail_id {
                return (503, "null".into());
            }
            let kids = match id {
                1 => vec![10, 20],
                10 => vec![11, 12],
                20 => vec![21],
                _ => vec![],
            };
            (
                200,
                json!({"id":id,"time":0,"type":"comment","text":format!("fresh {id}"),"kids":kids})
                    .to_string(),
            )
        })
        .await;
        let mut p = ready();
        p.network = network;
        let service = p.network.service.clone();
        let mut app = comments_app();
        let mut root = app.comments[0].clone();
        root.id = 10;
        root.child_ids = vec![11];
        root.text = "old parent".into();
        let mut child = root.clone();
        child.id = 11;
        child.child_ids.clear();
        child.text = "old readable reply".into();
        root.state = CommentState::Expanded {
            children: vec![child],
        };
        app.set_comments(vec![root]);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        crate::reader_actions::handle(&mut p, &mut app, KeyCode::Char('r'), tx, service, &mut None)
            .await
            .unwrap();
        assert!(app.is_loading());
        let msg = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        crate::handle_app_message(&mut app, msg);
        assert!(!app.is_loading());
        if fail_id == 0 {
            assert_eq!(app.visible_comment_count(), 4);
            assert_eq!(app.visible_comment_at(2).unwrap().1.text, "fresh 12");
            assert!(matches!(app.comments[1].state, CommentState::Collapsed));
        } else {
            assert_eq!(app.visible_comment_count(), 2);
            assert_eq!(
                app.visible_comment_at(1).unwrap().1.text,
                "old readable reply"
            );
            assert!(app.error().is_some());
        }
        while let Ok((path, _)) = requests.try_recv() {
            assert!(path.starts_with("/v0/item/"));
            assert!(
                !path.ends_with("/21.json"),
                "unopened branches stay unfetched"
            );
        }
        server.abort();
    }
}
