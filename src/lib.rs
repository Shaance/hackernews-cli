use anyhow::{Context, Result};
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

pub async fn get_stories(
    client: &Client,
    story_type: &str,
) -> Result<Vec<i32>, Box<dyn std::error::Error>> {
    let url = format!("{}/v0/{}stories.json", HN_API_URL, story_type);
    let resp = client
        .get(&url)
        .header(USER_AGENT, "reqwest")
        .send()
        .await
        .with_context(|| format!("Could not retrieve data from `{}`", url))?
        .json::<Vec<i32>>()
        .await?;
    Ok(resp)
}

async fn get_item(client: &Client, id: &i32) -> Result<HackerNewsItem, Box<dyn std::error::Error>> {
    let url = format!("{}/v0/item/{}.json", HN_API_URL, id);
    let resp = client
        .get(&url)
        .header(USER_AGENT, "reqwest")
        .send()
        .await
        .with_context(|| format!("Could not retrieve data from `{}`", url))?
        .json::<HackerNewsItem>()
        .await?;
    Ok(resp)
}

pub async fn get_items(
    client: &Client,
    ids: &[i32],
) -> Result<Vec<HNCLIItem>, Box<dyn std::error::Error>> {
    let mut items = Vec::new();
    let pb = indicatif::ProgressBar::new(ids.len() as u64);
    for (idx, id) in ids.iter().enumerate() {
        let item = get_item(client, id).await?;
        let item = HNCLIItem {
            title: item.title,
            url: item.url.unwrap_or(format!("{}item?id={}", YC_URL, item.id)),
            author: item.by,
            time: unix_epoch_to_datetime(item.time),
            time_ago: time_ago(item.time),
            score: item.score,
            comments: item.descendants,
        };
        items.push(item);
        pb.println(format!("[+] fetched #{} | ETA {:?}", idx + 1, pb.eta()));
        pb.inc(1);
    }
    Ok(items)
}

fn unix_epoch_to_datetime(unixepoch: u64) -> String {
    let dt = chrono::DateTime::<chrono::Utc>::from_utc(
        chrono::NaiveDateTime::from_timestamp(unixepoch as i64, 0),
        chrono::Utc,
    );
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

fn time_ago(epoch_time: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let diff = now - epoch_time as u64;
    match diff {
        0..=59 => format!("{} seconds ago", diff),
        60..=3599 => format!("{} minutes ago", diff / 60),
        3600..=86399 => format!("{} hours ago", diff / 3600),
        86400..=604799 => format!("{} days ago", diff / 86400),
        _ => format!("{} weeks ago", diff / 604800),
    }
}

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
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
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
