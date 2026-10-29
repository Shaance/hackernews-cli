//! Versioned offline input shared by the CLI and its browser companion.
use crate::hn_client::HackerNewsItem;
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    pub generated_at: String,
    pub feeds: BTreeMap<String, Vec<i32>>,
    pub items: BTreeMap<i32, HackerNewsItem>,
    pub articles: BTreeMap<i32, CapturedArticle>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{hn_client::HackerNewsClientImpl, HackerNewsCliService, HackerNewsCliServiceImpl};
    use std::sync::Arc;

    #[tokio::test]
    async fn snapshot_feed_accepts_real_job_and_preserves_ranking() {
        let mut snapshot: Snapshot =
            serde_json::from_str(include_str!("../preview/fixtures/snapshot.json")).unwrap();
        // Actual HN item that stopped the scheduled capture on 2026-10-29.
        let job: HackerNewsItem = serde_json::from_str(
            r#"{"by":"bastienbeurier","id":50280824,"score":1,"time":1793221206,
                "title":"Nango (YC W23) Is Hiring Across Eng, FDE, Marketing, DevRel",
                "type":"job","url":"https://nango.dev/careers"}"#,
        )
        .unwrap();
        snapshot.feeds.get_mut("top").unwrap().insert(1, job.id);
        snapshot.items.insert(job.id, job.clone());
        snapshot.validate().unwrap();
        let expected_ids = snapshot.feeds["top"].clone();
        let client = HackerNewsClientImpl::with_snapshot(Arc::new(snapshot.clone()));
        let page = HackerNewsCliServiceImpl::new_with_client(client)
            .fetch_stories_page("top", 20, 1)
            .await
            .unwrap();
        assert_eq!(
            page.iter().map(|item| item.id).collect::<Vec<_>>(),
            expected_ids
        );
        assert_eq!(page[1].title, job.title);
        assert_eq!(page[1].url, job.url.unwrap());

        snapshot.items.get_mut(&job.id).unwrap().r#type = "comment".into();
        assert!(
            snapshot.validate().is_err(),
            "comments cannot be feed entries"
        );
        snapshot.items.remove(&job.id);
        assert!(
            snapshot.validate().is_err(),
            "missing entries still fail validation"
        );
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapturedArticle {
    pub text: String,
    pub error: Option<String>,
    pub partial: bool,
    pub summary: Option<String>,
}

impl Snapshot {
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path).context("Read snapshot")?;
        ensure!(bytes.len() <= 16 * 1024 * 1024, "Snapshot exceeds 16 MiB");
        let value: Self = serde_json::from_slice(&bytes).context("Parse snapshot")?;
        value.validate()?;
        Ok(value)
    }

    pub fn write_atomic(&self, path: &Path) -> Result<()> {
        self.validate()?;
        let bytes = serde_json::to_vec(self)?;
        ensure!(bytes.len() <= 16 * 1024 * 1024, "Snapshot exceeds 16 MiB");
        let temporary = path.with_extension("pending");
        std::fs::write(&temporary, bytes)?;
        std::fs::rename(&temporary, path)?;
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "Unsupported snapshot version");
        chrono::DateTime::parse_from_rfc3339(&self.generated_at)?;
        for feed in ["top", "new", "best"] {
            let ids = self.feeds.get(feed).context("Missing feed")?;
            ensure!(!ids.is_empty() && ids.len() <= 20, "Invalid feed length");
            for id in ids {
                ensure!(
                    self.items
                        .get(id)
                        .is_some_and(|i| matches!(i.r#type.as_str(), "story" | "job")),
                    "Missing or invalid feed item {id}"
                );
            }
        }
        for (id, item) in &self.items {
            ensure!(*id == item.id, "Mismatched item ID");
            for child in item.kids.iter().flatten() {
                ensure!(self.items.contains_key(child), "Missing child {child}");
            }
        }
        Ok(())
    }
}
