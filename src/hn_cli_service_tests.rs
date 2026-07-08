use super::*;
use crate::hn_client::{HackerNewsItem, MockHackerNewsClient};
use crate::time_utils::{now, unix_epoch_to_datetime};
use mockall::predicate;

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

#[tokio::test]
async fn test_fetch_stories_page() {
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
