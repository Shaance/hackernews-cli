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
    prepare_with_network(snapshot, &config, &network).await
}

async fn prepare_with_network(
    snapshot: &mut hn_lib::snapshot::Snapshot,
    config: &Config,
    network: &network::Network,
) -> anyhow::Result<()> {
    for feed in ["top", "new", "best"] {
        let candidates: Vec<_> = snapshot.feeds[feed]
            .iter()
            .copied()
            .filter(|id| {
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
            // One first choice and one alternate; at most six provider calls per run.
            .take(2)
            .collect();
        let mut prepared = false;
        for selected in candidates {
            let article = snapshot
                .articles
                .get_mut(&selected)
                .expect("selected captured article");
            // Match the publication guard before selecting this as the featured article.
            if article
                .summary
                .as_ref()
                .is_some_and(|s| s.split_whitespace().count() >= 40)
            {
                prepared = true;
                break;
            }
            article.summary = None;
            let captured = ArticleCapture {
                title: snapshot.items[&selected].title.clone(),
                text: article.text.clone(),
                partial: article.partial,
            };
            match network
                .article(config.provider, config.key(), Profile::Fast, &captured)
                .await
            {
                Ok(summary) if summary.text.split_whitespace().count() >= 40 => {
                    article.summary = Some(summary.text);
                    eprintln!("Prepared featured summary {selected} for {feed}");
                    prepared = true;
                    break;
                }
                Ok(_) => eprintln!("Skipping summary {selected} for {feed}: fewer than 40 words"),
                Err(error) => {
                    eprintln!("Skipping summary {selected} for {feed}: {}", error.message)
                }
            }
        }
        anyhow::ensure!(prepared, "No valid featured summary for {feed} after up to two eligible articles; keeping previous published snapshot");
    }
    Ok(())
}

#[cfg(test)]
#[path = "snapshot_tests.rs"]
mod tests;
