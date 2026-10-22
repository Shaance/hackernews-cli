use super::{
    tests::{ready, story},
    *,
};
use crossterm::event::KeyCode;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU16, Ordering};

async fn finish(
    p: &mut Reader,
    requests: &mut mpsc::UnboundedReceiver<(String, Value)>,
) -> Vec<(String, Value)> {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !p.jobs.is_empty() {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            p.poll();
        }
    })
    .await
    .unwrap();
    let mut sent = vec![];
    while let Ok(request) = requests.try_recv() {
        sent.push(request);
    }
    sent
}
fn inputs(sent: &[(String, Value)]) -> Vec<String> {
    let mut values: Vec<_> = sent
        .iter()
        .filter(|(path, _)| path == "/chat/completions")
        .map(|(_, body)| body["messages"][1]["content"].as_str().unwrap().to_owned())
        .collect();
    values.sort();
    values
}

#[tokio::test]
async fn captures_survive_model_choices_revisit_retry_and_key_repair() {
    let status = Arc::new(AtomicU16::new(200));
    let response_status = status.clone();
    let (network, mut requests, server) = network::tests::serve(move |path, body| {
        if path == "/chat/completions" {
            let input: Value = serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
            let result = if input.get("article").is_some() { json!({"summary":"Article result"}) }
                else { json!({"overview":"Discussion result", "points":[{"text":"Reply", "sources":[11]}]}) };
            return (response_status.load(Ordering::SeqCst), json!({"choices":[{"message":{"content":result.to_string()}}]}).to_string());
        }
        let id: i32 = path.trim_start_matches("/v0/item/").trim_end_matches(".json").parse().unwrap();
        (200, json!({"id":id, "time":0, "type":if id < 10 {"story"} else {"comment"}, "title":"Updated title", "text":"Updated text", "by":"author", "kids":if id<10 {vec![10]} else if id==10 {vec![11]} else {vec![]}}).to_string())
    }).await;
    let mut p = ready();
    p.network = network;
    p.readings.clear();
    p.config.automatic = true;
    let mut original = story(1);
    original.text = "Original body".into();
    p.open(original.clone());
    let initial = p.next_attempt;
    p.config.profile = Profile::Thorough;
    p.request(Request::Generate);
    p.config.profile = Profile::Fast;
    p.request(Request::Generate);
    assert_eq!(
        p.next_attempt, initial,
        "quality choice must retain in-progress capture identity"
    );
    let first = finish(&mut p, &mut requests).await;
    assert_eq!(first.len(), 5); // Three HN reads, two model calls; self-post is already captured.
    assert_eq!(inputs(&first).len(), 2);
    p.config.profile = Profile::Thorough;
    p.open(story(2));
    finish(&mut p, &mut requests).await;
    original.title = "Live changed title".into();
    original.text = "Live changed post".into();
    p.open(original);
    assert!(finish(&mut p, &mut requests).await.is_empty());
    assert_eq!(p.readings[&1].story.title, "Live changed title");
    let dir = std::env::temp_dir().join(format!("hn-capture-settings-{}", std::process::id()));
    p.config_path = Some(dir.join("summary.json"));
    p.key(KeyCode::Char('M'));
    p.key(KeyCode::Tab);
    p.key(KeyCode::Enter);
    let upgraded = finish(&mut p, &mut requests).await;
    assert_eq!(upgraded.len(), 2);
    assert_eq!(inputs(&upgraded), inputs(&first));
    assert_eq!(
        p.readings[&1].article.result.as_ref().unwrap().profile,
        Profile::Thorough
    );

    status.store(500, Ordering::SeqCst);
    p.request(Request::Refresh);
    let refreshed = finish(&mut p, &mut requests).await;
    assert_eq!(refreshed.len(), 6); // Both sections refetch metadata; discussion also fetches its comments.
    assert_ne!(inputs(&refreshed), inputs(&first));
    assert!(p.readings[&1].article.failed());
    for profile in [Profile::Fast, Profile::Thorough] {
        p.config.profile = profile;
        p.request(Request::Generate);
        let sent = finish(&mut p, &mut requests).await;
        assert_eq!(sent.len(), 2);
        assert_eq!(inputs(&sent), inputs(&refreshed));
    }
    status.store(200, Ordering::SeqCst);
    p.request(Request::Retry);
    let retried = finish(&mut p, &mut requests).await;
    assert_eq!(retried.len(), 2);
    assert_eq!(inputs(&retried), inputs(&refreshed));
    status.store(401, Ordering::SeqCst);
    p.request(Request::Refresh);
    finish(&mut p, &mut requests).await;
    assert!(p.auth_failed);
    status.store(200, Ordering::SeqCst);
    p.key(KeyCode::Char('M'));
    p.key(KeyCode::Delete);
    for key in "replacement-key".chars() {
        p.key(KeyCode::Char(key));
    }
    p.key(KeyCode::Enter);
    let repaired = finish(&mut p, &mut requests).await;
    assert_eq!(repaired.len(), 2);
    assert_eq!(inputs(&repaired), inputs(&refreshed));
    assert!(!p.auth_failed);
    assert!(!p.readings[&1].article.failed());
    std::fs::remove_dir_all(dir).unwrap();
    server.abort();
}

#[tokio::test]
async fn failed_refresh_does_not_publish_staged_result_or_reuse_old_capture() {
    let mut p = ready();
    p.key(KeyCode::Tab);
    p.key(KeyCode::Enter);
    let reading = p.readings.get_mut(&1).unwrap();
    reading.staged = Some(Completed {
        value: tests::discussion("Obsolete"),
        profile: Profile::Thorough,
        capture: reading.discussion.capture.clone().unwrap(),
    });
    p.request(Request::Refresh);
    let attempt = match p.readings[&1].discussion.progress {
        Progress::Preparing(a, _, _) => a,
        _ => panic!(),
    };
    p.complete(jobs::Completion {
        story: 1,
        attempt,
        output: jobs::Output::DiscussionSource(Err(Failure::new("Capture unavailable"))),
    });
    p.key(KeyCode::Esc);
    assert!(p.readings[&1].staged.is_none());
    assert_eq!(
        p.readings[&1]
            .discussion
            .result
            .as_ref()
            .unwrap()
            .value
            .points[0]
            .text,
        "A concrete disagreement."
    );
    assert!(p.readings[&1].discussion.failed());
    assert!(p.readings[&1].discussion.capture.is_none());
    p.key(KeyCode::Esc);
    p.config.automatic = true;
    p.open(story(1));
    assert!(p.readings[&1].discussion.failed());
    p.config.profile = Profile::Thorough;
    p.request(Request::Generate);
    assert!(
        matches!(p.readings[&1].discussion.progress, Progress::Queued(a) | Progress::Preparing(a, _, _) if a.refresh)
    );
}
