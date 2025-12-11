use crate::hn_client::{
    HackerNewsClient, HackerNewsClientImpl, HackerNewsItem, MockHackerNewsClient,
};
use crate::time_utils::{time_ago, unix_epoch_to_datetime};
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::collections::HashSet;

pub mod app;
pub mod event;
pub mod hn_client;
mod time_utils;
pub mod ui;

/// HackerNews CLI Item representation for display
///
/// This struct represents a HackerNews story item formatted for CLI display,
/// containing all the relevant information about a story.

#[derive(Debug, Clone)]
/// Struct representing a HackerNews story item for CLI display
pub struct HNCLIItem {
    /// Story ID
    pub id: i32,
    /// Story title
    pub title: String,
    /// URL to the story
    pub url: String,
    /// Author/username of the poster
    pub author: String,
    /// Formatted timestamp when the story was posted
    pub time: String,
    /// Human-readable time ago string (e.g., "2 hours ago")
    pub time_ago: String,
    /// Story score (upvotes)
    pub score: i32,
    /// Number of comments, if available
    pub comments: Option<i32>,
}

impl std::fmt::Display for HNCLIItem {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let comment_str = match self.comments {
            Some(comments) => format!("{} comments", comments),
            None => String::new(),
        };
        let first_line = format!("{} by {}", self.title, self.author);
        let second_line = format!(
            "[{} points] - {} - {}",
            self.score, comment_str, self.time_ago
        );
        let last_line = format!("-> {}", self.url);
        write!(f, "{}\n{}\n{}", first_line, second_line, last_line)
    }
}

#[async_trait]
/// Main service trait for HackerNews CLI functionality
pub trait HackerNewsCliService {
    /// Fetch a page of stories from HackerNews
    ///
    /// # Arguments
    ///
    /// * `story_type` - Type of stories to fetch (e.g., "top", "new", "best")
    /// * `page_size` - Number of stories per page (1-50)
    /// * `page` - Page number to fetch (1-based)
    ///
    /// # Returns
    ///
    /// Vector of HNCLIItem structs representing the stories
    async fn fetch_stories_page(
        &self,
        story_type: &str,
        page_size: u8,
        page: u32,
    ) -> Result<Vec<HNCLIItem>>;

    /// Fetch top-level comments for a story
    ///
    /// # Arguments
    ///
    /// * `story_id` - The ID of the story
    ///
    /// # Returns
    ///
    /// Vector of Comment structs representing top-level comments
    async fn fetch_top_level_comments(&self, story_id: i32) -> Result<Vec<app::Comment>>;

    /// Fetch children for a comment
    ///
    /// # Arguments
    ///
    /// * `comment_ids` - Vec of comment IDs to fetch
    /// * `depth` - Depth of nesting for these comments
    ///
    /// # Returns
    ///
    /// Vector of Comment structs
    async fn fetch_comment_children(
        &self,
        comment_ids: &[i32],
        depth: usize,
    ) -> Result<Vec<app::Comment>>;

    /// Get the valid story types supported by the service
    ///
    /// # Returns
    ///
    /// HashSet of valid story type strings
    fn get_valid_story_types() -> HashSet<&'static str>;
}

/// Implementation of the HackerNews CLI service
///
/// This struct provides the concrete implementation of the HackerNewsCliService trait,
/// handling the business logic for fetching and formatting HackerNews stories.
pub struct HackerNewsCliServiceImpl<C: HackerNewsClient = HackerNewsClientImpl> {
    hn_client: C,
}

#[async_trait]
impl<C: HackerNewsClient + Sync> HackerNewsCliService for HackerNewsCliServiceImpl<C> {
    async fn fetch_stories_page(
        &self,
        story_type: &str,
        page_size: u8,
        page: u32,
    ) -> Result<Vec<HNCLIItem>> {
        let ids = self
            .hn_client
            .get_story_ids(story_type)
            .await
            .context(format!("Failed to get story IDs for type: {}", story_type))?;

        // Check if we have any stories at all
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        // Calculate pagination offsets
        let start = ((page - 1) as usize) * (page_size as usize);
        let end = start + (page_size as usize);

        // Check if we're trying to access beyond available stories
        if start >= ids.len() {
            return Ok(Vec::new());
        }

        // Get the slice for the current page
        let end = end.min(ids.len());
        let page_ids = &ids[start..end];

        let items = self.hn_client.get_items(page_ids).await;

        let mut result = Vec::new();
        for item_result in items {
            match item_result {
                Ok(item) => result.push(self.api_item_to_hn_cli_item(item)),
                Err(_e) => {
                    // Silently skip failed items - they may be deleted or unavailable
                }
            }
        }

        Ok(result)
    }

    async fn fetch_top_level_comments(&self, story_id: i32) -> Result<Vec<app::Comment>> {
        // First, fetch the story to get top-level comment IDs
        let story = self.hn_client.get_item(story_id).await?;

        let comment_ids = match story.kids {
            Some(ids) => ids,
            None => return Ok(Vec::new()),
        };

        // Fetch top-level comments
        self.fetch_comment_children(&comment_ids, 0).await
    }

    async fn fetch_comment_children(
        &self,
        comment_ids: &[i32],
        depth: usize,
    ) -> Result<Vec<app::Comment>> {
        let items = self.hn_client.get_items(comment_ids).await;

        let mut comments = Vec::new();
        for item_result in items {
            match item_result {
                Ok(item) => {
                    comments.push(self.api_item_to_comment(item, depth));
                }
                Err(_e) => {
                    // Silently skip failed comments - they may be deleted or unavailable
                }
            }
        }

        Ok(comments)
    }

    fn get_valid_story_types() -> HashSet<&'static str> {
        HashSet::from(["best", "new", "top"])
    }
}

impl<C: HackerNewsClient> HackerNewsCliServiceImpl<C> {
    /// Create a new HackerNews CLI service with a custom client
    ///
    /// # Arguments
    ///
    /// * `client` - Custom HackerNews client implementation
    ///
    /// # Returns
    ///
    /// A new HackerNewsCliServiceImpl instance
    pub fn new_with_client(client: C) -> Self {
        HackerNewsCliServiceImpl { hn_client: client }
    }
}

impl Default for HackerNewsCliServiceImpl<HackerNewsClientImpl> {
    fn default() -> Self {
        Self::new()
    }
}

impl HackerNewsCliServiceImpl<HackerNewsClientImpl> {
    /// Create a new HackerNews CLI service with the default client
    ///
    /// # Returns
    ///
    /// A new HackerNewsCliServiceImpl instance with default client
    pub fn new() -> Self {
        HackerNewsCliServiceImpl {
            hn_client: HackerNewsClientImpl::new(),
        }
    }
}

impl HackerNewsCliServiceImpl<MockHackerNewsClient> {
    /// Create a new HackerNews CLI service with a mock client for testing
    ///
    /// # Returns
    ///
    /// A new HackerNewsCliServiceImpl instance with mock client
    pub fn new_with_mock() -> Self {
        HackerNewsCliServiceImpl {
            hn_client: MockHackerNewsClient::new(),
        }
    }
}

impl<C: HackerNewsClient> HackerNewsCliServiceImpl<C> {
    fn get_item_url(&self, item: &HackerNewsItem) -> String {
        match &item.url {
            Some(url) => url.to_string(),
            None => format!(
                "{}item?id={}",
                self.hn_client.get_y_combinator_url(),
                item.id
            ),
        }
    }

    fn api_item_to_hn_cli_item(&self, item: HackerNewsItem) -> HNCLIItem {
        HNCLIItem {
            id: item.id,
            title: item.title.to_string(),
            url: self.get_item_url(&item),
            author: item.by,
            time: unix_epoch_to_datetime(item.time),
            time_ago: time_ago(item.time),
            score: item.score,
            comments: item.descendants,
        }
    }

    fn api_item_to_comment(&self, item: HackerNewsItem, depth: usize) -> app::Comment {
        let text = item.text.map(|t| decode_html(&t)).unwrap_or_default();

        let child_ids = item.kids.unwrap_or_default();

        app::Comment {
            id: item.id,
            author: item.by,
            text,
            time_ago: time_ago(item.time),
            state: app::CommentState::Collapsed,
            depth,
            deleted: item.deleted || item.dead,
            child_ids,
        }
    }
}

/// Decode HTML entities and strip basic HTML tags from comment text
fn decode_html(text: &str) -> String {
    // First decode HTML entities
    let decoded = html_escape::decode_html_entities(text);

    // Convert <p> tags to double newlines
    let result = decoded.replace("<p>", "\n\n");

    // Simple HTML tag stripping (iterate and remove everything between < and >)
    let mut clean = String::new();
    let mut in_tag = false;

    for ch in result.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => clean.push(ch),
            _ => {}
        }
    }

    clean.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hn_client::MockHackerNewsClient;
    use crate::time_utils::now;
    use mockall::predicate;

    #[test]
    fn test_unix_epoch_to_datetime() {
        let dt = chrono::DateTime::from_timestamp(1588888888, 0).unwrap();
        assert_eq!(
            dt.format("%Y-%m-%d %H:%M:%S").to_string(),
            "2020-05-07 22:01:28"
        );
    }

    #[test]
    fn test_time_ago() {
        let now = now();
        assert_eq!(time_ago(now), "0 seconds ago");
        assert_eq!(time_ago(now - 60), "1 minutes ago");
        assert_eq!(time_ago(now - 3600), "1 hours ago");
        assert_eq!(time_ago(now - 86400), "1 days ago");
        assert_eq!(time_ago(now - 604800), "1 weeks ago");
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
}
