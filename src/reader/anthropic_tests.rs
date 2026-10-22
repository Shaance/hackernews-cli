use super::*;

#[tokio::test]
async fn native_profiles_use_matching_models_prompts_evidence_and_authentication() {
    let captured = crate::reader::tests::discussion_capture();
    for (profile, model, prompt, count) in [
        (
            Profile::Fast,
            "claude-haiku-5-5",
            summary_prompts::FAST_DISCUSSION,
            3,
        ),
        (
            Profile::Thorough,
            "claude-sonnet-5-5",
            summary_prompts::THOROUGH_DISCUSSION,
            4,
        ),
    ] {
        let points: Vec<_> = (0..count)
            .map(|i| json!({"text":format!("Insight {i}"),"sources":[11]}))
            .collect();
        let result = json!({"overview":"Context", "points":points});
        let (network, mut requests, server) = tests::serve(move |path, _, headers| {
            assert_eq!(path, "/messages");
            let headers = headers.to_lowercase();
            assert!(headers.contains("authorization: bearer native-test-key\r\n"));
            assert!(headers.contains("anthropic-version: 2026-10-21\r\n"));
            (
                200,
                json!({"stop_reason":"end_turn","content":[
                    {"type":"thinking","thinking":"omitted"},
                    {"type":"text","text":result.to_string()}
                ]})
                .to_string(),
            )
        })
        .await;
        let result = network
            .discussion(Provider::Anthropic, "native-test-key", profile, &captured)
            .await
            .unwrap();
        assert_eq!(result.points.len(), count);
        assert_eq!(result.points[0].sources, [11]);
        let (_, sent) = requests.recv().await.unwrap();
        assert_eq!(sent["model"], model);
        assert_eq!(sent["system"], prompt);
        assert_eq!(
            sent["thinking"],
            json!({"type":"adaptive","display":"omitted"})
        );
        assert_eq!(sent["output_config"]["effort"], "low");
        assert_eq!(sent["output_config"]["format"]["type"], "json_schema");
        assert_eq!(sent["max_tokens"], 4096);
        assert!(sent.get("response_format").is_none());
        let schema = &sent["output_config"]["format"]["schema"];
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["properties"]["points"]["minItems"], 1);
        assert!(
            schema["properties"]["points"].get("maxItems").is_none(),
            "Unsupported by native JSON outputs"
        );
        assert_eq!(
            schema["properties"]["points"]["items"]["properties"]["sources"]["items"]["enum"],
            json!([10, 11])
        );
        assert_eq!(sent["messages"].as_array().unwrap().len(), 1);
        assert_eq!(sent["messages"][0]["role"], "user");
        let input: Value =
            serde_json::from_str(sent["messages"][0]["content"].as_str().unwrap()).unwrap();
        let comments: Vec<_> = captured
            .sources
            .iter()
            .map(|s| json!({"id":s.id,"parent":s.parent,"author":s.author,"text":s.excerpt}))
            .collect();
        assert_eq!(
            input,
            json!({"title":captured.title,"post":captured.post,"comments":comments})
        );
        assert!(requests.try_recv().is_err(), "No second inference call");
        server.abort();
    }
}

#[tokio::test]
async fn native_article_and_failures_keep_diagnostics_and_never_retry() {
    let key = "native-secret-key";
    let dir = std::env::temp_dir().join(format!(
        "hn-native-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let cases = [
        (200, json!({"id":"msg-test","stop_reason":"end_turn","content":[{"type":"text","text":"{\"summary\":\"Article claim\"}"}]}).to_string(), None),
        (401, json!({"error":{"message":key}}).to_string(), Some((true,"Edit key"))),
        (429, "private body".into(), Some((false,"rate limited"))),
        (500, "private body".into(), Some((false,"HTTP 500"))),
        (200, "not JSON".into(), Some((false,"could not be read"))),
        (200, json!({"stop_reason":"max_tokens","content":[{"type":"text","text":"{}"}]}).to_string(), Some((false,"cut short"))),
        (200, json!({"stop_reason":"refusal","content":[]}).to_string(), Some((false,"no summary"))),
        (200, json!({"content":[{"type":"text","text":"not JSON"}]}).to_string(), Some((false,"invalid"))),
    ];
    for (i, (status, body, error)) in cases.into_iter().enumerate() {
        let raw = body.clone();
        let (mut network, mut requests, server) = tests::serve(move |path, _, _| {
            assert_eq!(path, "/messages");
            (status, body.clone())
        })
        .await;
        let logs = dir.join(i.to_string());
        network.response_log_dir = Some(logs.clone());
        let output = network
            .article(
                Provider::Anthropic,
                key,
                Profile::Fast,
                &ArticleCapture {
                    title: "Story".into(),
                    text: "Evidence".into(),
                    partial: false,
                },
            )
            .await;
        match error {
            None => assert_eq!(output.unwrap().text, "Article claim"),
            Some((auth, message)) => {
                let failure = output.unwrap_err();
                assert_eq!(failure.auth, auth);
                assert!(failure.message.contains(message), "{}", failure.message);
                assert!(!failure.message.contains(key));
            }
        }
        let (_, sent) = requests.recv().await.unwrap();
        assert_eq!(sent["system"], summary_prompts::ARTICLE);
        assert!(requests.try_recv().is_err());
        let entries: Vec<_> = std::fs::read_dir(&logs)
            .unwrap()
            .map(|p| p.unwrap().path())
            .collect();
        assert_eq!(entries.len(), 1);
        let record: Value = serde_json::from_slice(&std::fs::read(&entries[0]).unwrap()).unwrap();
        assert_eq!(record["provider"], "Anthropic");
        assert_eq!(record["request_id"], "native-request-test-1");
        assert_eq!(record["model"], "claude-haiku-5-5");
        assert_eq!(record["http_status"], status);
        assert_eq!(record["raw_response"], raw.replace(key, "[REDACTED]"));
        assert!(!record.to_string().contains(key));
        server.abort();
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn native_discussion_rejects_citations_outside_captured_evidence() {
    let (network, mut requests, server) = tests::serve(|_, _, _| (200, json!({"content":[{"type":"text","text":
        json!({"overview":"Context","points":[{"text":"Unsupported claim","sources":[999]}]}).to_string()
    }]}).to_string())).await;
    let failure = network
        .discussion(
            Provider::Anthropic,
            "key",
            Profile::Fast,
            &crate::reader::tests::discussion_capture(),
        )
        .await
        .unwrap_err();
    assert!(failure.message.contains("citations"));
    requests.recv().await.unwrap();
    assert!(requests.try_recv().is_err());
    server.abort();
}
