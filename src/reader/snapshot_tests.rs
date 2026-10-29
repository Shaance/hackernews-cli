use super::*;
use crate::reader::{config::Provider, network::tests::serve};
use hn_lib::snapshot::Snapshot;
use serde_json::{json, Value};

fn fixture() -> Snapshot {
    let mut snapshot: Snapshot =
        serde_json::from_str(include_str!("../../preview/fixtures/snapshot.json")).unwrap();
    for article in snapshot.articles.values_mut() {
        article.summary = None;
    }
    // Preserve a social link at rank one, followed by a failing article and an alternate.
    for feed in snapshot.feeds.values_mut() {
        *feed = vec![1, 50225695, 2];
    }
    snapshot.items.get_mut(&50225695).unwrap().title =
        "Why is this fingerprinting script creating bioweapons?".into();
    snapshot
}

#[tokio::test]
async fn failed_or_short_featured_summary_uses_next_article_without_reordering() {
    for first_summary in [None, Some("Author Salvatore Sanfilippo... ")] {
        let mut snapshot = fixture();
        let feeds = snapshot.feeds.clone();
        let first_title = snapshot.items[&50225695].title.clone();
        let next_title = snapshot.items[&2].title.clone();
        let good_summary = snapshot.articles[&2].text.clone();
        let expected = good_summary.clone();
        let (network, mut requests, server) = serve(move |_, body, _| {
            let input: Value =
                serde_json::from_str(body["messages"][0]["content"].as_str().unwrap()).unwrap();
            let summary = if input["title"] == first_title {
                first_summary
            } else {
                Some(good_summary.as_str())
            };
            let content: Vec<_> = summary
                .into_iter()
                .map(|text| json!({"type":"text","text":json!({"summary":text}).to_string()}))
                .collect();
            (
                200,
                json!({"stop_reason":"end_turn","content":content}).to_string(),
            )
        })
        .await;
        let config = Config {
            provider: Provider::Anthropic,
            anthropic_api_key: "test-key".into(),
            ..Config::default()
        };
        prepare_with_network(&mut snapshot, &config, &network)
            .await
            .unwrap();
        assert_eq!(snapshot.feeds, feeds);
        assert!(snapshot.articles[&50225695].summary.is_none());
        assert_eq!(
            snapshot.articles[&2].summary.as_deref(),
            Some(expected.as_str())
        );
        snapshot.validate().unwrap();
        // The first candidate is tried once per feed; the good alternate is reused.
        let mut titles = Vec::new();
        while let Ok((_, body)) = requests.try_recv() {
            let input: Value =
                serde_json::from_str(body["messages"][0]["content"].as_str().unwrap()).unwrap();
            titles.push(input["title"].as_str().unwrap().to_owned());
        }
        assert_eq!(
            titles,
            vec![
                snapshot.items[&50225695].title.clone(),
                next_title,
                snapshot.items[&50225695].title.clone(),
                snapshot.items[&50225695].title.clone()
            ]
        );
        server.abort();
    }
}

#[tokio::test]
async fn failed_candidates_stop_at_two() {
    let mut snapshot = fixture();
    // A third eligible candidate must not trigger another provider call.
    let mut item = snapshot.items[&2].clone();
    item.id = 4;
    snapshot.items.insert(4, item);
    snapshot.articles.insert(4, snapshot.articles[&2].clone());
    snapshot.feeds.get_mut("top").unwrap().push(4);
    let (network, mut requests, server) =
        serve(|_, _, _| (200, json!({"content":[]}).to_string())).await;
    let config = Config {
        provider: Provider::Anthropic,
        anthropic_api_key: "test-key".into(),
        ..Config::default()
    };
    let result = prepare_with_network(&mut snapshot, &config, &network).await;
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("No valid featured summary for top"));
    assert!(requests.try_recv().is_ok());
    assert!(requests.try_recv().is_ok());
    assert!(requests.try_recv().is_err(), "no third paid attempt");
    server.abort();
}
