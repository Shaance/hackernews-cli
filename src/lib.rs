use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::{header::USER_AGENT, Client};
use serde::{Deserialize, Serialize};

const HN_API_URL: &str = "https://hacker-news.firebaseio.com/";
const YC_URL: &str = "https://news.ycombinator.com/";

#[derive(Debug, Serialize, Deserialize)]
pub struct HackerNewsItem {
    pub by: String,
    pub score: i32,
    pub time: u64,
    pub title: String,
    pub url: Option<String>,
    pub descendants: Option<i32>,
    id: i32,
    kids: Option<Vec<i32>>,
    r#type: String,
}

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
pub trait HackerNewsClient {
    fn new() -> Self;
    async fn get_stories(&self, story_type: &str) -> Result<Vec<i32>>;
    async fn get_items(&self, ids: &[i32]) -> Result<Vec<HNCLIItem>, Box<dyn std::error::Error>>;
}

pub struct HackerNewsClientImpl {
    client: Client,
}

#[async_trait]
impl HackerNewsClient for HackerNewsClientImpl {
    fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }

    async fn get_stories(&self, story_type: &str) -> Result<Vec<i32>> {
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

    async fn get_items(&self, ids: &[i32]) -> Result<Vec<HNCLIItem>, Box<dyn std::error::Error>> {
        let mut items = Vec::new();
        let pb = indicatif::ProgressBar::new(ids.len() as u64);
        for (idx, id) in ids.iter().enumerate() {
            let item = self.get_item(id).await?;
            let item = to_hn_cli_item(item);
            items.push(item);
            pb.println(format!("[+] fetched #{} | ETA {:?}", idx + 1, pb.eta()));
            pb.inc(1);
        }
        Ok(items)
    }
}

impl HackerNewsClientImpl {
    async fn get_item(&self, id: &i32) -> Result<HackerNewsItem, Box<dyn std::error::Error>> {
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

fn get_item_url(item: &HackerNewsItem) -> String {
    match &item.url {
        Some(url) => url.to_string(),
        None => format!("{}item?id={}", YC_URL, item.id),
    }
}

fn to_hn_cli_item(item: HackerNewsItem) -> HNCLIItem {
    HNCLIItem {
        title: (&item.title).to_string(),
        url: get_item_url(&item),
        author: item.by,
        time: unix_epoch_to_datetime(item.time),
        time_ago: time_ago(item.time),
        score: item.score,
        comments: item.descendants,
    }
}

fn unix_epoch_to_datetime(unixepoch: u64) -> String {
    let dt = chrono::DateTime::<chrono::Utc>::from_utc(
        chrono::NaiveDateTime::from_timestamp(unixepoch as i64, 0),
        chrono::Utc,
    );
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

fn time_ago(epoch_time: u64) -> String {
    let diff = now() - epoch_time as u64;
    match diff {
        0..=59 => format!("{} seconds ago", diff),
        60..=3599 => format!("{} minutes ago", diff / 60),
        3600..=86399 => format!("{} hours ago", diff / 3600),
        86400..=604799 => format!("{} days ago", diff / 86400),
        _ => format!("{} weeks ago", diff / 604800),
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("Could not retrieve current time")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_unix_epoch_to_datetime() {
        let dt = chrono::DateTime::<chrono::Utc>::from_utc(
            chrono::NaiveDateTime::from_timestamp(1588888888, 0),
            chrono::Utc,
        );
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
        assert_eq!(get_item_url(&item), "https://rust-lang.org");

        let item = HackerNewsItem { url: None, ..item };

        assert_eq!(
            get_item_url(&item),
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
        let item = to_hn_cli_item(item);
        assert_eq!(item.title, "Rust is awesome");
        assert_eq!(item.url, "https://rust-lang.org");
        assert_eq!(item.author, "me");
        assert_eq!(item.time, unix_epoch_to_datetime(now));
        assert_eq!(item.time_ago, "0 seconds ago");
        assert_eq!(item.score, 9);
        assert_eq!(item.comments, Some(1));
    }
}
