use crate::hn_client::{HackerNewsClient, HackerNewsClientImpl, HackerNewsItem};
use crate::time_utils::{time_ago, unix_epoch_to_datetime};
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashSet;

mod hn_client;
mod time_utils;

#[derive(Debug)]
pub struct HNCLIItem {
    pub title: String,
    pub url: String,
    pub author: String,
    pub time: String,
    pub time_ago: String,
    pub score: i32,
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
pub trait HackerNewsCliService {
    async fn fetch_stories_page(
        &self,
        story_type: &str,
        page_size: u8,
        page: u32,
    ) -> Result<Vec<HNCLIItem>>;

    fn get_valid_story_types() -> HashSet<&'static str>;
}

pub struct HackerNewsCliServiceImpl {
    hn_client: HackerNewsClientImpl,
}

#[async_trait]
impl HackerNewsCliService for HackerNewsCliServiceImpl {
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
            .unwrap_or_else(|_| panic!("Failed to get ids from story type {}", story_type));

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

        Ok(self
            .hn_client
            .get_items(page_ids)
            .await
            .into_iter()
            .map(|x| self.api_item_to_hn_cli_item(x.unwrap()))
            .collect())
    }

    fn get_valid_story_types() -> HashSet<&'static str> {
        HashSet::from(["best", "new", "top"])
    }
}

impl HackerNewsCliServiceImpl {
    pub fn new(client: Option<HackerNewsClientImpl>) -> Self {
        match client {
            None => HackerNewsCliServiceImpl {
                hn_client: HackerNewsClientImpl::new(),
            },
            Some(hn_client) => HackerNewsCliServiceImpl { hn_client },
        }
    }
}

impl HackerNewsCliServiceImpl {
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
            title: item.title.to_string(),
            url: self.get_item_url(&item),
            author: item.by,
            time: unix_epoch_to_datetime(item.time),
            time_ago: time_ago(item.time),
            score: item.score,
            comments: item.descendants,
        }
    }
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
        };

        let service = HackerNewsCliServiceImpl::new(None);
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
        };

        let service = HackerNewsCliServiceImpl::new(None);
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
    #[ignore]
    // broken for now as we can't use dynamic dispatch with async traits
    async fn test_fetch_top_n_stories() {
        let mut hn_client = MockHackerNewsClient::new();
        hn_client
            .expect_get_story_ids()
            .with(predicate::eq("best"))
            .times(1)
            .returning(|_| Ok(vec![1]));

        hn_client.expect_get_items().times(1).returning(|_| {
            vec![Ok(HackerNewsItem {
                by: "".to_string(),
                score: 0,
                time: 0,
                title: "".to_string(),
                url: None,
                descendants: None,
                id: 0,
                kids: None,
                r#type: "".to_string(),
            })]
        });

        // let service = HackerNewsCliServiceImpl::new(Some(hn_client));
        //
        // let items = service.fetch_top_n_stories("best", 1).await;
        //
        // assert!(items.is_ok());
        // assert_eq!(items.unwrap().len(), 1);
    }
}
