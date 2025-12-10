use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::future::join_all;
use mockall::automock;
use reqwest::header::USER_AGENT;
use reqwest::Client;
use serde::{Deserialize, Serialize};

/// HackerNews API base URL
const HN_API_URL: &str = "https://hacker-news.firebaseio.com/";
/// YCombinator base URL for item links
const YC_URL: &str = "https://news.ycombinator.com/";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HackerNewsItem {
    #[serde(default)]
    pub by: String,
    #[serde(default)]
    pub score: i32,
    pub time: u64,
    #[serde(default)]
    pub title: String,
    pub url: Option<String>,
    pub descendants: Option<i32>,
    pub(crate) id: i32,
    pub(crate) kids: Option<Vec<i32>>,
    pub(crate) r#type: String,
    pub text: Option<String>,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub dead: bool,
}

#[automock]
#[async_trait]
pub trait HackerNewsClient {
    async fn get_story_ids(&self, story_type: &str) -> Result<Vec<i32>>;
    async fn get_items(&self, ids: &[i32]) -> Vec<Result<HackerNewsItem>>;
    async fn get_item(&self, id: i32) -> Result<HackerNewsItem>;
    fn get_y_combinator_url(&self) -> &str;
}

/// Configuration for HackerNews client
#[derive(Debug, Clone)]
pub struct HackerNewsClientConfig {
    /// API base URL
    pub api_url: String,
    /// YCombinator base URL
    pub yc_url: String,
    /// Request timeout in seconds
    pub timeout: u64,
    /// User agent string
    pub user_agent: String,
}

impl Default for HackerNewsClientConfig {
    fn default() -> Self {
        Self {
            api_url: HN_API_URL.to_string(),
            yc_url: YC_URL.to_string(),
            timeout: 10,
            user_agent: "hn-cli".to_string(),
        }
    }
}

#[derive(Debug)]
pub struct HackerNewsClientImpl {
    client: Client,
    config: HackerNewsClientConfig,
}

#[async_trait]
impl HackerNewsClient for HackerNewsClientImpl {
    async fn get_story_ids(&self, story_type: &str) -> Result<Vec<i32>> {
        let url = format!("{}v0/{}stories.json", self.config.api_url, story_type);
        let resp = self
            .client
            .get(&url)
            .header(USER_AGENT, &self.config.user_agent)
            .send()
            .await
            .with_context(|| format!("Could not retrieve data from `{}`", url))?
            .json::<Vec<i32>>()
            .await?;
        Ok(resp)
    }

    async fn get_items(&self, ids: &[i32]) -> Vec<Result<HackerNewsItem>> {
        let future_items = ids.iter().map(|id| self.get_item(*id));
        return join_all(future_items).await;
    }

    async fn get_item(&self, id: i32) -> Result<HackerNewsItem> {
        let url = format!("{}v0/item/{}.json", self.config.api_url, id);
        let resp = self
            .client
            .get(&url)
            .header(USER_AGENT, &self.config.user_agent)
            .send()
            .await
            .with_context(|| format!("Could not retrieve data from `{}`", url))?
            .json::<HackerNewsItem>()
            .await?;
        Ok(resp)
    }

    fn get_y_combinator_url(&self) -> &str {
        &self.config.yc_url
    }
}

impl Default for HackerNewsClientImpl {
    fn default() -> Self {
        Self::new()
    }
}

impl HackerNewsClientImpl {
    /// Create a new HackerNews client with default configuration
    pub fn new() -> Self {
        Self::with_config(HackerNewsClientConfig::default())
    }

    /// Create a new HackerNews client with custom configuration
    pub fn with_config(config: HackerNewsClientConfig) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout))
            .build()
            .expect("Failed to create HTTP client");

        Self { client, config }
    }
}
