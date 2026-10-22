use super::*;
use serde_json::json;

#[tokio::test]
async fn sampling_reserves_replies_redistributes_budget_and_keeps_unavailable_parents() {
    for (roots, replies, expected_roots, expected_replies) in [
        (80, 2, 40, 20),
        (80, 0, 60, 0),
        (2, 40, 2, 58),
        (2, 1, 2, 3),
    ] {
        let (network, mut requests, server) = network::tests::serve(move |path, _, _| {
            let id: i32 = path.trim_start_matches("/v0/item/").trim_end_matches(".json").parse().unwrap();
            if roots == 80 && replies == 2 && id == 4 { return (404, "null".into()); }
            let kids = if id == 101 && replies == 1 { vec![10001] } else if id == 999 { (1..=roots).collect::<Vec<_>>() }
                else if id < 100 { (1..=replies).map(|reply| id * 100 + reply).collect() } else { vec![] };
            (200, json!({"id":id,"time":0,"type":"comment","by":"author","text":"文".repeat(1801),"deleted":id==1,"kids":kids}).to_string())
        }).await;
        let capture = sources::discussion(&network.service, tests::story(999), false)
            .await
            .unwrap();
        let selected = &capture.input.sources;
        assert_eq!(
            selected.iter().filter(|s| s.parent.is_none()).count(),
            expected_roots - usize::from(roots == 80 && replies == 2)
        );
        assert_eq!(
            selected.iter().filter(|s| s.parent.is_some()).count(),
            expected_replies
        );
        for (index, source) in selected.iter().enumerate() {
            if let Some(parent) = source.parent {
                assert!(selected[..index].iter().any(|s| s.id == parent));
            }
        }
        let replies: Vec<_> = selected
            .iter()
            .filter(|s| s.parent.is_some())
            .map(|s| s.id)
            .collect();
        if !replies.is_empty() {
            assert_eq!(&replies[..2], &[101, 201]);
        }
        assert!(selected
            .iter()
            .find(|s| s.id == 1)
            .unwrap()
            .excerpt
            .is_empty());
        let mut count = 0;
        while requests.try_recv().is_ok() {
            count += 1;
        }
        assert_eq!(count, 1 + expected_roots + expected_replies);
        let sent = selected.iter().find(|s| !s.excerpt.is_empty()).unwrap();
        assert_eq!(sent.excerpt.chars().count(), 1800);
        let mut p = tests::ready();
        p.readings
            .get_mut(&1)
            .unwrap()
            .discussion
            .result
            .as_mut()
            .unwrap()
            .capture = Arc::new(capture);
        if !replies.is_empty() {
            let text = p.readings[&1]
                .source_lines(101, 120)
                .iter()
                .map(|l| l.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(text.contains("Additional context · not sent to the model"));
            assert!(text.contains("Parent comment unavailable in captured sources"));
        }
        server.abort();
    }
}
