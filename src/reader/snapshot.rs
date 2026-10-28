use super::*;

impl Reader {
    pub fn load_snapshot(&mut self, snapshot: Arc<hn_lib::snapshot::Snapshot>) {
        // A snapshot session has no inference configuration, even on a configured laptop.
        self.config = Config {
            automatic: false,
            ..Config::default()
        };
        self.config_path = None;
        self.snapshot = Some(snapshot);
    }

    pub(super) fn load_captured_summary(&mut self) {
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let Some(reading) = self.active.and_then(|id| self.readings.get_mut(&id)) else {
            return;
        };
        let Some(article) = snapshot.articles.get(&reading.story.id) else {
            return;
        };
        let Some(summary) = &article.summary else {
            return;
        };
        if reading.article.result.is_some() {
            return;
        }
        let capture = Arc::new(Capture {
            captured: snapshot.generated_at.clone(),
            input: ArticleCapture {
                title: reading.story.title.clone(),
                text: article.text.clone(),
                partial: article.partial,
            },
        });
        reading.article.capture = Some(capture.clone());
        reading.article.result = Some(Completed {
            value: Article {
                text: summary.clone(),
            },
            profile: Profile::Fast,
            capture,
        });
    }
}

/// Reuse the native provider and prompt; visitor sessions never call this path.
pub(crate) async fn prepare(snapshot: &mut hn_lib::snapshot::Snapshot) -> anyhow::Result<()> {
    let config = Config::load(&Config::path()?)?;
    anyhow::ensure!(
        !config.key().is_empty(),
        "Publisher needs a configured summary API key"
    );
    let network = network::Network {
        response_log_dir: None,
        ..network::Network::default()
    };
    for feed in ["top", "new", "best"] {
        let selected = snapshot.feeds[feed]
            .iter()
            .copied()
            .find(|id| {
                let item = &snapshot.items[id];
                let Ok(url) = reqwest::Url::parse(item.url.as_deref().unwrap_or_default()) else {
                    return false;
                };
                let host = url.host_str().unwrap_or_default().trim_end_matches('.');
                url.scheme() == "https"
                    && !["x.com", "twitter.com"]
                        .iter()
                        .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
                    && snapshot.articles.get(id).is_some_and(|article| {
                        article.error.is_none() && article.text.chars().count() >= 400
                    })
            })
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "No useful featured article in {feed}; keeping previous published snapshot"
                )
            })?;
        let article = snapshot
            .articles
            .get_mut(&selected)
            .expect("selected captured article");
        if article
            .summary
            .as_ref()
            .is_some_and(|s| !s.trim().is_empty())
        {
            continue;
        }
        let captured = ArticleCapture {
            title: snapshot.items[&selected].title.clone(),
            text: article.text.clone(),
            partial: article.partial,
        };
        let summary = network
            .article(config.provider, config.key(), Profile::Fast, &captured)
            .await
            .map_err(|error| anyhow::anyhow!("Summary for {selected} failed: {}", error.message))?;
        article.summary = Some(summary.text);
        eprintln!("Prepared featured summary {selected} for {feed}");
    }
    Ok(())
}
