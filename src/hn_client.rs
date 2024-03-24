use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::future::join_all;
use mockall::automock;
use reqwest::header::USER_AGENT;
use reqwest::Client;
use serde::{Deserialize, Serialize};

const HN_API_URL: &str = "https://hacker-news.firebaseio.com/";
const YC_URL: &str = "https://news.ycombinator.com/";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HackerNewsItem {
    pub by: String,
    pub score: i32,
    pub time: u64,
    pub title: String,
    pub url: Option<String>,
    pub descendants: Option<i32>,
    pub(crate) id: i32,
    pub(crate) kids: Option<Vec<i32>>,
    pub(crate) r#type: String,
}

#[automock]
#[async_trait]
pub trait HackerNewsClient {
    async fn get_story_ids(&self, story_type: &str) -> Result<Vec<i32>>;
    async fn get_items(&self, ids: &[i32]) -> Vec<Result<HackerNewsItem>>;
    fn get_y_combinator_url(&self) -> &str;
}

#[derive(Default)]
pub struct HackerNewsClientImpl {
    client: Client,
}

#[async_trait]
impl HackerNewsClient for HackerNewsClientImpl {
    async fn get_story_ids(&self, story_type: &str) -> Result<Vec<i32>> {
        let url = format!("{}/v0/{}stories.json", HN_API_URL, story_type);
        let resp = self
            .client
            .get(&url)
            .header(USER_AGENT, "reqwest")
            .send()
            .await
            .with_context(|| format!("Could not retrieve data from `{}`", url))?
            .json::<Vec<i32>>()
            .await?;
        Ok(resp)
    }

    async fn get_items(&self, ids: &[i32]) -> Vec<Result<HackerNewsItem>> {
        let future_items = ids.iter().map(|id| self.get_item(id));
        return join_all(future_items).await;
    }

    fn get_y_combinator_url(&self) -> &str {
        YC_URL
    }
}

impl HackerNewsClientImpl {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }
    async fn get_item(&self, id: &i32) -> Result<HackerNewsItem> {
        let url = format!("{}/v0/item/{}.json", HN_API_URL, id);
        let resp = self
            .client
            .get(&url)
            .header(USER_AGENT, "reqwest")
            .send()
            .await
            .with_context(|| format!("Could not retrieve data from `{}`", url))?
            .json::<HackerNewsItem>()
            .await?;
        Ok(resp)
    }
}
