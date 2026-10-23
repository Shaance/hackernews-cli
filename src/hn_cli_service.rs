use crate::app;
use crate::hn_client::{HackerNewsClient, HackerNewsClientImpl, HackerNewsItem};
use crate::time_utils::{time_ago, unix_epoch_to_datetime};
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::collections::HashSet;

#[cfg(test)]
#[path = "hn_cli_service_tests.rs"]
mod tests;

/// HackerNews CLI Item representation for display
///
/// This struct represents a HackerNews story item formatted for CLI display,
/// containing all the relevant information about a story.

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
    /// Decoded text of a self post (for example, Ask HN).
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub links: Vec<String>,
}

impl std::fmt::Display for HNCLIItem {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let comment_str = match self.comments {
            Some(comments) => format!("{comments} comments"),
            None => String::new(),
        };
        let first_line = format!("{} by {}", self.title, self.author);
        let second_line = format!(
            "[{} points] - {} - {}",
            self.score, comment_str, self.time_ago
        );
        let last_line = format!("-> {}", self.url);
        write!(f, "{first_line}\n{second_line}\n{last_line}")
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
#[derive(Clone)]
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
            .context(format!("Failed to get story IDs for type: {story_type}"))?;

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
        let comment_ids = self.top_level_comment_ids(story_id).await?;

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
    /// Fetch IDs separately so the summary experiment can bound work before fetching.
    pub async fn top_level_comment_ids(&self, story_id: i32) -> Result<Vec<i32>> {
        Ok(self
            .hn_client
            .get_item(story_id)
            .await?
            .kids
            .unwrap_or_default())
    }

    /// Fetch current story metadata and root IDs from the same HN item.
    pub async fn story_with_comment_ids(&self, story_id: i32) -> Result<(HNCLIItem, Vec<i32>)> {
        let item = self.hn_client.get_item(story_id).await?;
        let roots = item.kids.clone().unwrap_or_default();
        Ok((self.api_item_to_hn_cli_item(item), roots))
    }

    /// Refresh only the branches already open, retaining the old tree on any fetch error.
    pub async fn refresh_comments(
        &self,
        story_id: i32,
        previous: &[app::Comment],
    ) -> Result<Vec<app::Comment>> {
        let ids = self.top_level_comment_ids(story_id).await?;
        self.refresh_comment_branch(&ids, previous, 0).await
    }

    async fn refresh_comment_branch(
        &self,
        ids: &[i32],
        previous: &[app::Comment],
        depth: usize,
    ) -> Result<Vec<app::Comment>> {
        let mut comments = Vec::new();
        for item in self.hn_client.get_items(ids).await {
            let mut comment = self.api_item_to_comment(item?, depth);
            if let Some(app::Comment {
                state: app::CommentState::Expanded { children },
                ..
            }) = previous.iter().find(|old| old.id == comment.id)
            {
                comment.state = app::CommentState::Expanded {
                    children: Box::pin(self.refresh_comment_branch(
                        &comment.child_ids,
                        children,
                        depth + 1,
                    ))
                    .await?,
                };
            }
            comments.push(comment);
        }
        Ok(comments)
    }

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
        let raw_text = item.text.as_deref().unwrap_or_default();
        let links = web_links(raw_text);
        let text = decode_html(raw_text);
        HNCLIItem {
            id: item.id,
            title: item.title.to_string(),
            url: self.get_item_url(&item),
            author: item.by,
            time: unix_epoch_to_datetime(item.time),
            time_ago: time_ago(item.time),
            score: item.score,
            comments: item.descendants,
            text,
            links,
        }
    }

    fn api_item_to_comment(&self, item: HackerNewsItem, depth: usize) -> app::Comment {
        let raw_text = item.text.as_deref().unwrap_or_default();
        let links = web_links(raw_text);
        let text = decode_html(raw_text);

        let child_ids = item.kids.unwrap_or_default();

        app::Comment {
            id: item.id,
            author: item.by,
            text,
            links,
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
    let without_tags = strip_html_tags(text);
    html_escape::decode_html_entities(&without_tags)
        .trim_matches(['\n', '\r'])
        .to_string()
}

fn strip_html_tags(text: &str) -> String {
    let mut clean = String::new();
    let mut in_tag = false;
    let mut tag = String::new();

    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if in_tag {
            if ch == '>' {
                match tag.trim().to_ascii_lowercase().as_str() {
                    "p" | "/p" if !clean.ends_with("\n\n") => {
                        clean.push_str("\n\n");
                    }
                    "br" | "br/" | "br /" | "pre" | "/pre" if !clean.ends_with('\n') => {
                        clean.push('\n');
                    }
                    _ => {}
                }
                tag.clear();
                in_tag = false;
            } else {
                tag.push(ch);
            }
            continue;
        }

        match ch {
            '<' if chars
                .peek()
                .is_some_and(|next| next.is_ascii_alphabetic() || *next == '/' || *next == '!') =>
            {
                in_tag = true;
            }
            _ => clean.push(ch),
        }
    }

    clean
}

/// HN emits quoted href attributes. Keep web destinations even when the label is abbreviated.
fn web_links(html: &str) -> Vec<String> {
    let mut links = Vec::new();
    for tag in html
        .split('<')
        .skip(1)
        .filter_map(|part| part.split_once('>'))
    {
        let tag = tag.0.trim();
        if !tag.starts_with("a ") {
            continue;
        }
        let Some((_, value)) = tag.split_once("href=") else {
            continue;
        };
        let Some(quote @ ('\'' | '"')) = value.chars().next() else {
            continue;
        };
        let Some((url, _)) = value[1..].split_once(quote) else {
            continue;
        };
        let url = html_escape::decode_html_entities(url).into_owned();
        if reqwest::Url::parse(&url).is_ok_and(|parsed| matches!(parsed.scheme(), "http" | "https"))
            && !links.contains(&url)
        {
            links.push(url);
        }
    }
    links
}
