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
                    self.items.get(id).is_some_and(|i| i.r#type == "story"),
                    "Missing story {id}"
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
