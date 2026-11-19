use super::*;
use saved::tests::{article, discussion, temp_dir};
use serde_json::{json, Value};
use std::path::Path;

async fn fixture(
    comment: &'static str,
) -> (
    network::Network,
    mpsc::UnboundedReceiver<(String, Value)>,
    tokio::task::JoinHandle<()>,
) {
    network::tests::serve(move |path, body, _| {
        let response = match path {
            "/v0/item/1.json" => json!({
                "id":1,"time":0,"type":"story","text":"Refetched article","kids":[11],
            }),
            "/v0/item/11.json" => json!({
                "id":11,"time":0,"type":"comment","text":comment,"by":"reply",
            }),
            "/chat/completions" => {
                let input: Value = serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
                let content = if let Some(article) = input.get("article") {
                    json!({"summary": format!("Summary of {}", article.as_str().unwrap())})
                } else {
                    json!({"overview":"Discussion","points":[{"text":"Fresh point","sources":[11]}]})
                };
                json!({"choices":[{"message":{"content":content.to_string()}}]})
            }
            _ => panic!("unexpected request {path}"),
        };
        (200, response.to_string())
    }).await
}

fn reader(root: &Path, network: network::Network, profile: Profile) -> Reader {
    Reader {
        store: Some(SummaryStore::disk(root.to_path_buf())),
        network,
        config: Config {
            api_key: "fixture".into(),
            profile,
            ..Config::default()
        },
        ..Reader::default()
    }
}

async fn finish(reader: &mut Reader) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(completion) = reader.jobs.join_next().await {
            match completion {
                Ok(completion) => reader.complete(completion),
                Err(error) if error.is_cancelled() => {}
                Err(error) => panic!("summary task failed: {error}"),
            }
            reader.start_queued();
        }
    })
    .await
    .unwrap();
}

fn provider_requests(requests: &mut mpsc::UnboundedReceiver<(String, Value)>) -> Vec<Value> {
    std::iter::from_fn(|| requests.try_recv().ok())
        .filter_map(|(path, body)| (path == "/chat/completions").then_some(body))
        .collect()
}

#[tokio::test]
async fn restart_restores_both_halves_without_provider_requests() {
    let root = temp_dir();
    let (network, mut requests, server) = fixture("Cited evidence").await;
    let mut first = reader(&root, network.clone(), Profile::Fast);
    let mut item = story(1);
    item.text = "Initial article".into();
    first.open(item.clone());
    finish(&mut first).await;
    assert_eq!(provider_requests(&mut requests).len(), 2);
    drop(first);

    let mut restarted = reader(&root, network, Profile::Thorough);
    restarted.open(item);
    finish(&mut restarted).await;
    restarted.request(Request::Missing);
    finish(&mut restarted).await;
    assert_eq!(provider_requests(&mut requests).len(), 0);
    let reading = &restarted.readings[&1];
    assert_eq!(
        reading.article.result.as_ref().unwrap().value.text,
        "Summary of Initial article"
    );
    assert_eq!(
        reading.discussion.result.as_ref().unwrap().value.points[0].text,
        "Fresh point"
    );
    assert!(reading.article.capture.is_none() && reading.discussion.capture.is_none());
    assert_eq!(reading.citations(), [11]);
    assert!(reading
        .source_lines(11, 80)
        .iter()
        .any(|line| line.to_string().contains("Cited evidence")));
    server.abort();
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn restart_requests_only_the_missing_half() {
    for article_saved in [true, false] {
        let root = temp_dir();
        let mut store = SummaryStore::disk(root.clone());
        if article_saved {
            store.save_article(article(saved::now()), false).unwrap();
        } else {
            store.save_discussion(discussion(saved::now())).unwrap();
        }
        let (network, mut requests, server) = fixture("Fresh comment").await;
        let mut restarted = reader(&root, network, Profile::Thorough);
        let mut item = story(1);
        item.text = "New article".into();
        restarted.open(item);
        finish(&mut restarted).await;
        let requests = provider_requests(&mut requests);
        assert_eq!(requests.len(), 1);
        let input: Value =
            serde_json::from_str(requests[0]["messages"][1]["content"].as_str().unwrap()).unwrap();
        assert_eq!(input.get("article").is_some(), !article_saved);
        assert!(store.load_article(1).is_some() && store.load_discussion(1).is_some());
        server.abort();
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[tokio::test]
async fn generate_with_another_profile_refetches_and_replaces_saved_summary() {
    let root = temp_dir();
    let mut store = SummaryStore::disk(root.clone());
    store.save_article(article(saved::now()), false).unwrap();
    store.save_discussion(discussion(saved::now())).unwrap();
    let (network, mut requests, server) = fixture("Refetched comment").await;
    let mut restarted = reader(&root, network, Profile::Fast);
    restarted.open(story(1));
    restarted.request(Request::Generate);
    finish(&mut restarted).await;
    let requests = provider_requests(&mut requests);
    assert_eq!(
        requests.len(),
        1,
        "the restored Fast discussion also satisfies Generate"
    );
    assert_eq!(
        requests[0]["model"],
        Profile::Fast.model(Provider::OpenRouter)
    );
    let updated = store.load_article(1).unwrap();
    assert_eq!(updated.header.profile, Profile::Fast);
    assert_eq!(updated.data.article.text, "Summary of Refetched article");
    assert_eq!(
        store.load_discussion(1).unwrap().data.discussion.points[0].text,
        "Saved disagreement."
    );
    server.abort();
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn empty_article_and_all_empty_discussion_excerpts_are_not_saved() {
    let root = temp_dir();
    let (network, mut requests, server) = fixture("\u{1}").await;
    let mut p = reader(&root, network, Profile::Fast);
    p.open(story(1));
    finish(&mut p).await;
    assert_eq!(provider_requests(&mut requests).len(), 0);
    let reading = &p.readings[&1];
    assert_eq!(
        reading.article.result.as_ref().unwrap().value.text,
        "No article text to summarize."
    );
    assert_eq!(
        reading.discussion.result.as_ref().unwrap().value.overview,
        "No readable comments to summarize yet."
    );
    let sources = &reading.discussion.capture.as_ref().unwrap().input.sources;
    assert_eq!(sources.len(), 1);
    assert!(sources[0].excerpt.is_empty());
    assert!(!root.join("1.article.json").exists() && !root.join("1.discussion.json").exists());
    server.abort();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn restored_header_shows_age_new_comments_partial_and_coverage() {
    let root = temp_dir();
    let mut store = SummaryStore::disk(root.clone());
    let saved_at = saved::now() - 5 * 3600 - 60;
    store.save_article(article(saved_at), false).unwrap();
    store.save_discussion(discussion(saved_at)).unwrap();
    let mut p = Reader {
        store: Some(store),
        config: Config::default(),
        ..Reader::default()
    };
    let mut item = story(1);
    item.comments = Some(5);
    p.open(item);
    let rendered = screen(&mut p, &mut comments_app(), 120, 40);
    assert!(rendered.contains("ARTICLE  Thorough · Saved 5h ago"));
    assert!(rendered
        .contains("DISCUSSION  Fast · 2 excerpts (partial) · Saved 5h ago · 3 new comments"));
    assert!(rendered.contains("Partial source · opening portion only"));
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn staged_discussion_is_saved_only_if_it_becomes_displayed() {
    for action in ["escape", "hide", "reopen", "discard"] {
        let root = temp_dir();
        let mut store = SummaryStore::disk(root.clone());
        let mut saved_article = article(saved::now());
        saved_article.header.profile = Profile::Fast;
        store.save_article(saved_article, false).unwrap();
        store.save_discussion(discussion(saved::now())).unwrap();
        let original = std::fs::read(root.join("1.discussion.json")).unwrap();
        let (network, mut requests, server) = fixture("Fresh evidence").await;
        let mut p = reader(&root, network.clone(), Profile::Fast);
        p.open(story(1));
        p.key(KeyCode::Tab);
        p.key(KeyCode::Enter);
        assert_eq!(p.source_id(), Some(11));

        p.config.profile = Profile::Thorough;
        p.request(Request::Generate);
        finish(&mut p).await;
        assert_eq!(provider_requests(&mut requests).len(), 2);
        assert!(p.readings[&1].staged.is_some());
        assert_eq!(
            std::fs::read(root.join("1.discussion.json")).unwrap(),
            original
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
            "Saved disagreement."
        );

        p.config.provider = Provider::Anthropic;
        p.readings.get_mut(&1).unwrap().story.comments = Some(8);
        match action {
            "escape" => {
                p.key(KeyCode::Esc);
            }
            "hide" => {
                p.key(KeyCode::Char('S'));
            }
            "reopen" => p.open(story(1)),
            "discard" => {
                p.config.provider = Provider::OpenRouter;
                p.config.profile = Profile::Fast;
                p.request(Request::Generate);
                assert!(p.readings[&1].staged.is_none());
                p.key(KeyCode::Esc);
                finish(&mut p).await;
            }
            _ => unreachable!(),
        }
        let on_disk = store.load_discussion(1).unwrap();
        if action == "discard" {
            assert_eq!(
                std::fs::read(root.join("1.discussion.json")).unwrap(),
                original
            );
            let mut restarted = reader(&root, network, Profile::Fast);
            restarted.open(story(1));
            assert_eq!(
                restarted.readings[&1]
                    .discussion
                    .result
                    .as_ref()
                    .unwrap()
                    .value
                    .points[0]
                    .text,
                "Saved disagreement."
            );
        } else {
            assert_eq!(on_disk.data.discussion.points[0].text, "Fresh point");
            assert_eq!(on_disk.header.provider, Provider::OpenRouter);
            assert_eq!(on_disk.header.profile, Profile::Thorough);
            assert_eq!(on_disk.data.context.comments_at_save, 2);
            assert_eq!(
                p.readings[&1]
                    .discussion
                    .result
                    .as_ref()
                    .unwrap()
                    .value
                    .points[0]
                    .text,
                "Fresh point"
            );
        }
        server.abort();
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[tokio::test]
async fn switching_back_to_restored_profile_cancels_preparation_without_provider_calls() {
    let root = temp_dir();
    let mut store = SummaryStore::disk(root.clone());
    let mut saved_article = article(saved::now());
    saved_article.header.profile = Profile::Fast;
    store.save_article(saved_article, false).unwrap();
    store.save_discussion(discussion(saved::now())).unwrap();
    let (network, mut requests, server) = fixture("Fresh evidence").await;
    let mut p = reader(&root, network, Profile::Fast);
    p.open(story(1));
    p.config.profile = Profile::Thorough;
    p.request(Request::Generate);
    assert!(matches!(
        p.readings[&1].article.progress,
        Progress::Preparing(..)
    ));
    assert!(matches!(
        p.readings[&1].discussion.progress,
        Progress::Preparing(..)
    ));

    p.config.profile = Profile::Fast;
    p.request(Request::Generate);
    finish(&mut p).await;
    assert_eq!(provider_requests(&mut requests).len(), 0);
    assert!(matches!(p.readings[&1].article.progress, Progress::Idle));
    assert!(matches!(p.readings[&1].discussion.progress, Progress::Idle));
    assert_eq!(
        p.readings[&1].article.result.as_ref().unwrap().value.text,
        "Saved article"
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
        "Saved disagreement."
    );
    server.abort();
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn save_failure_shows_an_error_and_keeps_the_result_readable() {
    for half in ["article", "discussion", "staged", "both"] {
        let root = temp_dir();
        let mut store = SummaryStore::disk(root.clone());
        let mut saved_article = article(saved::now());
        let mut saved_discussion = discussion(saved::now());
        if half == "article" || half == "both" {
            saved_article.header.profile = Profile::Fast;
            if half == "article" {
                saved_discussion.header.profile = Profile::Thorough;
            }
        }
        store.save_article(saved_article, false).unwrap();
        store.save_discussion(saved_discussion).unwrap();
        let (network, mut requests, server) = fixture("Fresh evidence").await;
        let mut p = reader(&root, network, Profile::Fast);
        let mut item = story(1);
        item.text = "Fresh article".into();
        p.open(item);
        if half == "staged" || half == "both" {
            p.key(KeyCode::Tab);
            p.key(KeyCode::Enter);
        }
        let blocked = root.join("blocked");
        std::fs::write(&blocked, b"not a directory").unwrap();
        p.store = Some(SummaryStore::disk(blocked));
        p.config.profile = Profile::Thorough;
        p.request(Request::Generate);
        finish(&mut p).await;
        assert_eq!(
            provider_requests(&mut requests).len(),
            if half == "both" { 2 } else { 1 }
        );
        if half == "staged" || half == "both" {
            assert!(p.readings[&1].staged.is_some());
            if half == "staged" {
                assert!(p.browser_notice.is_empty());
            } else {
                let rendered = screen(&mut p, &mut comments_app(), 120, 40);
                assert!(rendered.contains("Summary save failed:"), "{rendered}");
            }
            p.key(KeyCode::Esc);
        }
        let rendered = screen(&mut p, &mut comments_app(), 120, 40);
        assert!(
            rendered.contains("Summary save failed:"),
            "{half}: {rendered}"
        );
        assert!(p.notice_started.is_none());
        if half == "article" {
            assert!(rendered.contains("Summary of Refetched article"));
        } else {
            assert!(rendered.contains("Fresh point"));
        }
        server.abort();
        std::fs::remove_dir_all(root).unwrap();
    }
}
