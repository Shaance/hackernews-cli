//! Prepare bounded source material without a private service or browser crawler.
use super::{network::Network, ArticleCapture, Capture, DiscussionCapture, Failure, Source};
use hn_lib::{HNCLIItem, HackerNewsCliService, HackerNewsCliServiceImpl};
use reqwest::Client;
use std::collections::VecDeque;

pub fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect()
}
pub async fn article(client: &Client, story: &HNCLIItem) -> Result<(String, bool), Failure> {
    let text =
        if story.url.is_empty() || story.url.starts_with("https://news.ycombinator.com/item?") {
            story.text.clone()
        } else {
            let url = reqwest::Url::parse(&story.url)
                .map_err(|_| Failure::new("Invalid article URL. o opens the original."))?;
            if !matches!(url.scheme(), "http" | "https") {
                return Err(Failure::new(
                    "This article format is not supported. o opens the original.",
                ));
            }
            let mut response =
                client.get(url.clone()).send().await.map_err(|_| {
                    Failure::new("Could not fetch this article. r Retry · o Original")
                })?;
            if !response.status().is_success() {
                return Err(Failure::new(format!(
                    "Article returned HTTP {}. r Retry · o Original",
                    response.status().as_u16()
                )));
            }
            let final_url = response.url().clone();
            let mut html = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| Failure::new("Article download interrupted. r Retry"))?
            {
                if html.len() + chunk.len() > 5 * 1024 * 1024 {
                    return Err(Failure::new(
                        "Article is too large to read here. o Original",
                    ));
                }
                html.extend_from_slice(&chunk);
            }
            tokio::task::spawn_blocking(move || extract(&html, &final_url))
                .await
                .map_err(|_| Failure::new("Article reader failed. r Retry"))??
        };
    let text = clean(&text);
    let partial = text.chars().count() > 24_000;
    Ok((text.chars().take(24_000).collect(), partial))
}
pub fn extract(html: &[u8], url: &reqwest::Url) -> Result<String, Failure> {
    let product = readability::extractor::extract(&mut std::io::Cursor::new(html), url)
        .map_err(|_| Failure::new("Could not extract this article. o Original"))?;
    if product.text.trim().is_empty() {
        return Err(Failure::new("No readable article text. o Original"));
    }
    Ok(product.text)
}
pub async fn article_capture(
    network: &Network,
    mut story: HNCLIItem,
    refresh: bool,
) -> Result<Capture<ArticleCapture>, Failure> {
    if refresh {
        story = network
            .service
            .story_with_comment_ids(story.id)
            .await
            .map_err(|_| Failure::new("Could not refresh story. r Retry"))?
            .0;
    }
    let (text, partial) = article(&network.client, &story).await?;
    Ok(Capture::new(ArticleCapture {
        title: clean(&story.title),
        text,
        partial,
    }))
}

pub async fn discussion(
    service: &HackerNewsCliServiceImpl,
    mut story: HNCLIItem,
    refresh: bool,
) -> Result<Capture<DiscussionCapture>, Failure> {
    let (current, roots) = service
        .story_with_comment_ids(story.id)
        .await
        .map_err(|_| Failure::new("Could not load discussion. r Retry"))?;
    if refresh {
        story = current;
    }
    let mut roots: VecDeque<_> = roots.into_iter().enumerate().collect();
    let mut threads: Vec<VecDeque<(Option<i32>, i32)>> = vec![VecDeque::new(); roots.len()];
    let mut next_thread = 0;
    let mut sources = Vec::new();
    let mut attempted = 0;
    let mut root_attempts = 0;
    let mut partial = false;
    while attempted < 60 {
        let mut batch = Vec::new();
        let replies = threads.iter().any(|q| !q.is_empty());
        if !roots.is_empty() && (root_attempts < 40 || !replies) {
            let quota = if root_attempts < 40 {
                40 - root_attempts
            } else {
                60 - attempted
            };
            for (thread, id) in roots.drain(..roots.len().min(quota).min(60 - attempted).min(32)) {
                batch.push((thread, None, id));
                root_attempts += 1;
            }
        } else if replies {
            // Round-robin across roots; each root's queue preserves parent-first HN order.
            while batch.len() < 32.min(60 - attempted) && threads.iter().any(|q| !q.is_empty()) {
                let thread = next_thread % threads.len();
                next_thread = (thread + 1) % threads.len();
                if let Some((parent, id)) = threads[thread].pop_front() {
                    batch.push((thread, parent, id));
                }
            }
        } else {
            break;
        }
        let ids: Vec<_> = batch.iter().map(|(_, _, id)| *id).collect();
        attempted += ids.len();
        let comments = service
            .fetch_comment_children(&ids, 0)
            .await
            .map_err(|_| Failure::new("Could not load discussion. r Retry"))?;
        partial |= comments.len() != ids.len();
        for comment in comments {
            let &(thread, parent, _) = batch.iter().find(|(_, _, id)| *id == comment.id).unwrap();
            threads[thread].extend(comment.child_ids.iter().map(|id| (Some(comment.id), *id)));
            let text = if comment.deleted {
                String::new()
            } else {
                clean(&comment.text)
            };
            let excerpt: String = text.chars().take(1800).collect();
            partial |= excerpt.is_empty() || excerpt.len() < text.len();
            sources.push(Source {
                id: comment.id,
                parent,
                author: clean(&comment.author),
                text,
                excerpt,
            });
        }
    }
    let count = sources.iter().filter(|s| !s.excerpt.is_empty()).count();
    if sources.is_empty() && partial {
        return Err(Failure::new("Comments could not be loaded. r Retry"));
    }
    partial |= !roots.is_empty() || threads.iter().any(|q| !q.is_empty());
    let coverage = format!(
        "{count} comment excerpts{}",
        if partial { " (partial)" } else { "" }
    );
    Ok(Capture::new(DiscussionCapture {
        title: clean(&story.title),
        post: clean(&story.text).chars().take(6000).collect(),
        sources,
        coverage,
    }))
}
