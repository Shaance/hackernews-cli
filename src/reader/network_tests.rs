use super::*;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

// Only the external HTTP boundary is replaced; serialization and parsing are real.
async fn server(status: u16, body: String) -> (Network, tokio::task::JoinHandle<Value>) {
    let (network, mut requests, server) = serve(move |_, _, _| (status, body.clone())).await;
    let task = tokio::spawn(async move {
        let (_, request) = requests.recv().await.unwrap();
        server.abort();
        request
    });
    (network, task)
}
pub(crate) async fn serve(
    handler: impl Fn(&str, &Value, &str) -> (u16, String) + Send + 'static,
) -> (
    Network,
    tokio::sync::mpsc::UnboundedReceiver<(String, Value)>,
    tokio::task::JoinHandle<()>,
) {
    use hn_lib::hn_client::{HackerNewsClientConfig, HackerNewsClientImpl};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let (offset, length) = loop {
                let mut bytes = [0; 4096];
                let read = stream.read(&mut bytes).await.unwrap();
                assert!(read > 0);
                request.extend_from_slice(&bytes[..read]);
                if let Some(offset) = request.windows(4).position(|s| s == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..offset]).to_lowercase();
                    let length: usize = headers
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length: "))
                        .unwrap_or("0")
                        .parse()
                        .unwrap();
                    if request.len() >= offset + 4 + length {
                        break (offset + 4, length);
                    }
                }
            };
            let path = String::from_utf8_lossy(&request[..offset])
                .split_whitespace()
                .nth(1)
                .unwrap()
                .to_owned();
            let value = if length == 0 {
                Value::Null
            } else {
                serde_json::from_slice(&request[offset..offset + length]).unwrap()
            };
            let (status, body) =
                handler(&path, &value, &String::from_utf8_lossy(&request[..offset]));
            let response = format!("HTTP/1.1 {status} Result\r\nContent-Type: application/json\r\nX-Request-Id: request-test-1\r\nRequest-Id: native-request-test-1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            stream.write_all(response.as_bytes()).await.unwrap();
            tx.send((path, value)).unwrap();
        }
    });
    let network = Network {
        client: Client::new(),
        endpoint: format!("{base}chat/completions"),
        anthropic_endpoint: format!("{base}messages"),
        response_log_dir: None,
        service: HackerNewsCliServiceImpl::new_with_client(HackerNewsClientImpl::with_config(
            HackerNewsClientConfig {
                api_url: base,
                ..Default::default()
            },
        )),
    };
    (network, rx, task)
}
#[tokio::test]
async fn profiles_send_structured_requests_and_parse_provider_content() {
    for (profile, model) in [
        (Profile::Fast, "anthropic/claude-haiku-5.5"),
        (Profile::Thorough, "anthropic/claude-sonnet-5.5"),
    ] {
        let body = json!({"choices":[{"finish_reason":"stop","message":{"content":"{\"summary\":\"A source-backed claim.\"}"}}]}).to_string();
        let (network, request) = server(200, body).await;
        let output = network
            .complete(
                Provider::OpenRouter,
                "test-key",
                profile,
                "Summarize",
                json!({"article":"Source"}),
                json!({"type":"object"}),
            )
            .await
            .unwrap();
        assert_eq!(output["summary"], "A source-backed claim.");
        let sent = request.await.unwrap();
        assert_eq!(sent["model"], model);
        assert_eq!(sent["reasoning"]["effort"], "low");
        assert_eq!(sent["response_format"]["type"], "json_schema");
        assert_eq!(sent["response_format"]["json_schema"]["strict"], true);
        assert_eq!(sent["provider"]["require_parameters"], true);
        assert_eq!(sent["messages"][1]["content"], "{\"article\":\"Source\"}");
    }
}
#[tokio::test]
async fn discussion_profiles_select_tested_prompts_and_keep_source_input_independent() {
    let captured = crate::reader::tests::discussion_capture();
    let mut inputs = Vec::new();
    // Start with Thorough: it must also work without generating Fast first.
    for (profile, count, model) in [
        (Profile::Thorough, 4, "anthropic/claude-sonnet-5.5"),
        (Profile::Fast, 3, "anthropic/claude-haiku-5.5"),
    ] {
        let points: Vec<_> = (0..count)
            .map(|i| json!({"text":format!("Useful insight {i}."),"sources":[10]}))
            .collect();
        let content = json!({"overview":"Discussion topic.","points":points});
        let body = json!({"choices":[{"message":{"content":content.to_string()}}]}).to_string();
        let (network, request) = server(200, body).await;
        let output = network
            .discussion(Provider::OpenRouter, "test-key", profile, &captured)
            .await
            .unwrap();
        assert_eq!(output.points.len(), count, "Keep every returned insight");
        let sent = request.await.unwrap();
        assert_eq!(sent["model"], model);
        assert_eq!(sent["reasoning"]["effort"], "low");
        let messages = sent["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2, "Only instructions and captured evidence");
        let prompt = messages[0]["content"].as_str().unwrap();
        let point_schema =
            &sent["response_format"]["json_schema"]["schema"]["properties"]["points"];
        match profile {
            Profile::Fast => {
                assert!(prompt.contains("1–3 useful insights, NEVER more than three"));
                assert!(!prompt.contains("include an additional point"));
                assert_eq!(point_schema["maxItems"], 3);
            }
            Profile::Thorough => {
                assert!(prompt.contains("include an additional point when needed"));
                assert!(!prompt.contains("NEVER more than three"));
                assert!(point_schema.get("maxItems").is_none());
            }
        }
        let input: Value = serde_json::from_str(messages[1]["content"].as_str().unwrap()).unwrap();
        let comments: Vec<_> = captured
            .sources
            .iter()
            .map(|s| json!({"id":s.id,"parent":s.parent,"author":s.author,"text":s.excerpt}))
            .collect();
        assert_eq!(
            input,
            json!({"title":captured.title,"post":captured.post,"comments":comments})
        );
        inputs.push(input);
    }
    assert_eq!(inputs[0], inputs[1]);
}

#[tokio::test]
async fn provider_failures_offer_key_repair_or_retry_without_leaking_response() {
    let cases = [
        (401, "secret provider details", true, "Edit key"),
        (429, "secret provider details", false, "rate limited"),
        (200, "not JSON", false, "could not be read"),
        (
            200,
            r#"{"error":{"code":502,"message":"private upstream details"}}"#,
            false,
            "provider could not complete",
        ),
        (
            200,
            r#"{"choices":[{"finish_reason":"length","message":{"content":"{}"}}]}"#,
            false,
            "cut short",
        ),
        (
            200,
            r#"{"choices":[{"message":{"content":"not JSON"}}]}"#,
            false,
            "invalid",
        ),
    ];
    for (status, body, auth, message) in cases {
        let (network, request) = server(status, body.into()).await;
        let error = network
            .complete(
                Provider::OpenRouter,
                "test-key",
                Profile::Fast,
                "Summarize",
                json!({}),
                json!({}),
            )
            .await
            .unwrap_err();
        assert_eq!(error.auth, auth);
        assert!(error.message.contains(message), "{}", error.message);
        assert!(!error.message.contains("secret"));
        request.await.unwrap();
    }
}
#[tokio::test]
async fn self_post_input_is_unicode_bounded_and_marked_partial() {
    let mut story = crate::reader::tests::story(1);
    story.text = "文".repeat(24_001);
    let (text, partial) = sources::article(&Client::new(), &story).await.unwrap();
    assert_eq!(text.chars().count(), 24_000);
    assert!(partial);
}

#[tokio::test]
async fn discussion_publishes_only_citations_from_its_captured_sources() {
    for source_id in [11, 10, 999] {
        let content = json!({"overview":"Two viewpoints.","points":[{"text":"A disagreement.","sources":[source_id]}]});
        let body = json!({"choices":[{"message":{"content":content.to_string()}}]}).to_string();
        let (network, request) = server(200, body).await;
        let mut captured = crate::reader::tests::discussion_capture();
        captured.sources[0].excerpt.clear(); // Unavailable parents are context, never citation candidates.
        captured.sources[1].text = "文".repeat(1801);
        captured.sources[1].excerpt = "文".repeat(1800);
        let result = network
            .discussion(Provider::OpenRouter, "test-key", Profile::Fast, &captured)
            .await;
        if source_id == 11 {
            let result = result.unwrap();
            assert_eq!(result.points[0].sources, vec![11]);
        } else {
            assert!(result.unwrap_err().message.contains("citations"));
        }
        let sent = request.await.unwrap();
        let input: Value =
            serde_json::from_str(sent["messages"][1]["content"].as_str().unwrap()).unwrap();
        assert_eq!(input["comments"][0]["id"], 11);
        assert_eq!(input["comments"][0]["text"], captured.sources[1].excerpt);
        assert_eq!(
            sent["response_format"]["json_schema"]["schema"]["properties"]["points"]["items"]
                ["properties"]["sources"]["items"]["enum"],
            json!([11])
        );
    }
}
#[tokio::test]
async fn article_parses_provider_summary() {
    let body = json!({"choices":[{"message":{"content":"{\"summary\":\"The author proposes a tradeoff.\"}"}}]}).to_string();
    let (network, request) = server(200, body).await;
    let result = network
        .article(
            Provider::OpenRouter,
            "test-key",
            Profile::Thorough,
            &ArticleCapture {
                title: "An example story".into(),
                text: "Source text".into(),
                partial: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(result.text, "The author proposes a tradeoff.");
    request.await.unwrap();
}

#[tokio::test]
async fn response_logs_preserve_bodies_and_ids_without_credentials_or_behavior_changes() {
    let directory = std::env::temp_dir().join(format!(
        "hn-response-logs-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let key = "sk-or-test-\"secret\\suffix";
    let success = json!({"id":"gen-placeholder","provider":"Azure","choices":[{
        "finish_reason":"stop","message":{"content":"{\"summary\":\"placeholder\"}"}
    }]})
    .to_string();
    let cases = [
        (200, success.clone(), None),
        (
            401,
            json!({"error":{"message":format!("Rejected {key}")}}).to_string(),
            Some("Edit key"),
        ),
        (200, format!("not JSON: {key}"), Some("could not be read")),
        (
            200,
            json!({"error":{"message":format!("Provider echoed {key}")}}).to_string(),
            Some("provider could not complete"),
        ),
    ];
    for (index, (status, body, failure)) in cases.into_iter().enumerate() {
        let path = directory.join(index.to_string());
        let (mut network, request) = server(status, body.clone()).await;
        network.response_log_dir = Some(path.clone());
        let output = network
            .complete(
                Provider::OpenRouter,
                key,
                Profile::Thorough,
                "Summarize",
                json!({"title":key,"comments":[]}),
                json!({}),
            )
            .await;
        request.await.unwrap();
        match failure {
            Some(message) => assert!(output.unwrap_err().message.contains(message)),
            None => assert_eq!(output.unwrap()["summary"], "placeholder"),
        }
        let entries: Vec<_> = std::fs::read_dir(&path)
            .unwrap()
            .map(|f| f.unwrap().path())
            .collect();
        assert_eq!(entries.len(), 1);
        let text = std::fs::read_to_string(&entries[0]).unwrap();
        let record: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(record["http_status"], status);
        assert_eq!(record["request_id"], "request-test-1");
        assert_eq!(
            record["model"],
            Profile::Thorough.model(Provider::OpenRouter)
        );
        assert_eq!(record["section"], "discussion");
        assert_eq!(record["title"], "[REDACTED]");
        assert!(
            chrono::DateTime::parse_from_rfc3339(record["timestamp"].as_str().unwrap()).is_ok()
        );
        assert!(!text.contains("Authorization") && !text.contains("sk-or-test"));
        let raw = record["raw_response"].as_str().unwrap();
        if index == 0 {
            assert_eq!(
                raw, body,
                "Preserve the original body, not parsed/reformatted content"
            );
            assert_eq!(record["generation_id"], "gen-placeholder");
        } else {
            assert!(raw.contains("[REDACTED]"));
            assert!(!raw.contains(key));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&entries[0]).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
    }
    // A file in place of the directory must not lose an otherwise successful response.
    let blocked = directory.join("not-a-directory");
    std::fs::write(&blocked, "keep").unwrap();
    let (mut network, request) = server(200, success).await;
    network.response_log_dir = Some(blocked.clone());
    let output = network
        .complete(
            Provider::OpenRouter,
            key,
            Profile::Fast,
            "Summarize",
            json!({}),
            json!({}),
        )
        .await
        .unwrap();
    assert_eq!(output["summary"], "placeholder");
    request.await.unwrap();
    assert_eq!(std::fs::read_to_string(blocked).unwrap(), "keep");
    std::fs::remove_dir_all(directory).unwrap();
}
