use super::*;
use crate::hn_client::{HackerNewsItem, MockHackerNewsClient};
use crate::time_utils::{now, unix_epoch_to_datetime};
use anyhow::anyhow;
use mockall::predicate;

fn hn_item(
    id: i32,
    by: &str,
    text: Option<&str>,
    kids: Option<Vec<i32>>,
    deleted: bool,
    dead: bool,
) -> HackerNewsItem {
    HackerNewsItem {
        id,
        by: by.to_string(),
        time: now(),
        kids,
        url: Some("https://example.com".to_string()),
        score: 10,
        title: "Test Story".to_string(),
        descendants: Some(5),
        r#type: "comment".to_string(),
        text: text.map(ToString::to_string),
        deleted,
        dead,
    }
}

#[test]
fn test_display() {
    let item = HNCLIItem {
        id: 123,
        title: "Rust is awesome".to_string(),
        url: "https://rust-lang.org".to_string(),
        author: "me".to_string(),
        time: "2020-05-07 22:01:28".to_string(),
        time_ago: "0 seconds ago".to_string(),
        score: 9,
        comments: Some(1),
    };
    assert_eq!(
        item.to_string(),
        "Rust is awesome by me\n[9 points] - 1 comments - 0 seconds ago\n-> https://rust-lang.org"
    );
}

#[test]
fn test_get_item_url() {
    let item = HackerNewsItem {
        id: 1,
        by: "me".to_string(),
        time: 1588888888,
        kids: None,
        url: Some("https://rust-lang.org".to_string()),
        score: 9,
        title: "Rust is awesome".to_string(),
        descendants: Some(1),
        r#type: "story".to_string(),
        text: None,
        deleted: false,
        dead: false,
    };

    let mut client = MockHackerNewsClient::new();
    client
        .expect_get_y_combinator_url()
        .return_const("https://news.ycombinator.com/".to_string());
    let service = HackerNewsCliServiceImpl::new_with_client(client);
    assert_eq!(service.get_item_url(&item), "https://rust-lang.org");

    let item = HackerNewsItem { url: None, ..item };

    assert_eq!(
        service.get_item_url(&item),
        "https://news.ycombinator.com/item?id=1"
    );
}

#[test]
fn test_to_hn_cli_item() {
    let now = now();

    let item = HackerNewsItem {
        id: 1,
        by: "me".to_string(),
        time: now,
        kids: None,
        url: Some("https://rust-lang.org".to_string()),
        score: 9,
        title: "Rust is awesome".to_string(),
        descendants: Some(1),
        r#type: "story".to_string(),
        text: None,
        deleted: false,
        dead: false,
    };

    let mut client = MockHackerNewsClient::new();
    client
        .expect_get_y_combinator_url()
        .return_const("https://news.ycombinator.com/".to_string());
    let service = HackerNewsCliServiceImpl::new_with_client(client);
    let item = service.api_item_to_hn_cli_item(item);

    assert_eq!(item.title, "Rust is awesome");
    assert_eq!(item.url, "https://rust-lang.org");
    assert_eq!(item.author, "me");
    assert_eq!(item.time, unix_epoch_to_datetime(now));
    assert_eq!(item.time_ago, "0 seconds ago");
    assert_eq!(item.score, 9);
    assert_eq!(item.comments, Some(1));
}

#[test]
fn decode_html_preserves_escaped_comparisons() {
    assert_eq!(
        decode_html("x&lt;y &amp;&amp; y&gt;z<p><i>done</i>"),
        "x<y && y>z\n\ndone"
    );
}

#[tokio::test]
async fn fetch_stories_page_returns_requested_page_items() {
    // Create a mock client
    let mut hn_client = MockHackerNewsClient::new();

    // Set up expectations
    hn_client
        .expect_get_story_ids()
        .with(predicate::eq("best"))
        .times(1)
        .returning(|_| Ok(vec![1, 2, 3]));

    hn_client.expect_get_items().times(1).returning(|ids| {
        ids.iter()
            .map(|_| {
                Ok(HackerNewsItem {
                    by: "test_user".to_string(),
                    score: 10,
                    time: 1234567890,
                    title: "Test Story".to_string(),
                    url: Some("https://example.com".to_string()),
                    descendants: Some(5),
                    id: 1,
                    kids: None,
                    r#type: "story".to_string(),
                    text: None,
                    deleted: false,
                    dead: false,
                })
            })
            .collect()
    });

    // Create service with mock client
    let service = HackerNewsCliServiceImpl::new_with_client(hn_client);

    // Test fetching first page with 2 items
    let items = service.fetch_stories_page("best", 2, 1).await;

    assert!(items.is_ok());
    let items = items.unwrap();
    assert_eq!(items.len(), 2);

    // Verify the items are properly converted
    assert_eq!(items[0].title, "Test Story");
    assert_eq!(items[0].author, "test_user");
    assert_eq!(items[0].score, 10);
}

#[tokio::test]
async fn fetch_top_level_comments_converts_comment_fields() {
    let mut hn_client = MockHackerNewsClient::new();

    hn_client
        .expect_get_item()
        .with(predicate::eq(1))
        .times(1)
        .returning(|_| {
            Ok(hn_item(
                1,
                "story_author",
                None,
                Some(vec![10, 11]),
                false,
                false,
            ))
        });

    hn_client
        .expect_get_items()
        .times(1)
        .withf(|ids| ids == [10, 11])
        .returning(|_| {
            vec![
                Ok(hn_item(
                    10,
                    "commenter",
                    Some("Hello &amp;<p>world"),
                    Some(vec![100, 101]),
                    false,
                    false,
                )),
                Ok(hn_item(11, "flagged", None, None, false, true)),
            ]
        });

    let service = HackerNewsCliServiceImpl::new_with_client(hn_client);

    let comments = service
        .fetch_top_level_comments(1)
        .await
        .expect("top-level comments should load");

    assert_eq!(comments.len(), 2);
    assert_eq!(comments[0].id, 10);
    assert_eq!(comments[0].author, "commenter");
    assert_eq!(comments[0].text, "Hello &\n\nworld");
    assert_eq!(comments[0].child_ids, vec![100, 101]);
    assert_eq!(comments[0].depth, 0);
    assert!(matches!(comments[0].state, app::CommentState::Collapsed));
    assert!(!comments[0].deleted);
    assert_eq!(comments[1].id, 11);
    assert!(comments[1].deleted);
    assert!(comments[1].text.is_empty());
}

#[tokio::test]
async fn fetch_comment_children_skips_failed_items_and_sets_depth() {
    let mut hn_client = MockHackerNewsClient::new();

    hn_client
        .expect_get_items()
        .times(1)
        .withf(|ids| ids == [20, 21])
        .returning(|_| {
            vec![
                Err(anyhow!("comment unavailable")),
                Ok(hn_item(
                    21,
                    "child_author",
                    Some("<i>Child</i> &gt; parent"),
                    None,
                    true,
                    false,
                )),
            ]
        });

    let service = HackerNewsCliServiceImpl::new_with_client(hn_client);

    let comments = service
        .fetch_comment_children(&[20, 21], 2)
        .await
        .expect("available child comments should load");

    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].id, 21);
    assert_eq!(comments[0].author, "child_author");
    assert_eq!(comments[0].text, "Child > parent");
    assert_eq!(comments[0].depth, 2);
    assert!(comments[0].child_ids.is_empty());
    assert!(matches!(comments[0].state, app::CommentState::Collapsed));
    assert!(comments[0].deleted);
}
