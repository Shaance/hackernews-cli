//! Bounded daily publisher. Uses the same article extractor as the native reader.
use anyhow::{ensure, Context, Result};
use futures::{stream, StreamExt};
use hn_lib::{
    hn_client::{HackerNewsClient, HackerNewsClientImpl},
    snapshot::{CapturedArticle, Snapshot},
    HackerNewsCliServiceImpl,
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::Path,
    time::Duration,
};

pub async fn capture(path: &Path) -> Result<()> {
    let client = HackerNewsClientImpl::new();
    let mut feeds = BTreeMap::new();
    for category in ["top", "new", "best"] {
        let ids = client.get_story_ids(category).await?;
        feeds.insert(
            category.to_string(),
            ids.into_iter().take(20).collect::<Vec<_>>(),
        );
    }
    let unique: BTreeSet<_> = feeds.values().flatten().copied().collect();
    let mut items = BTreeMap::new();
    let mut articles = BTreeMap::new();
    let mut captures = stream::iter(unique.into_iter().map(|id| {
        let client = client.clone();
        async move {
            let story = client.get_item(id).await?;
            let service = HackerNewsCliServiceImpl::new_with_client(client.clone());
            let cli = service.story_with_comment_ids(id).await?.0;
            let article = match public_article_client(&cli.url).await {
                Ok(http) => match crate::reader::sources::article(&http, &cli).await {
                    Ok((text, partial)) => CapturedArticle {
                        text,
                        partial,
                        error: None,
                        summary: None,
                    },
                    Err(e) => CapturedArticle {
                        text: String::new(),
                        partial: false,
                        error: Some(e.message),
                        summary: None,
                    },
                },
                Err(e) => CapturedArticle {
                    text: String::new(),
                    partial: false,
                    error: Some(e.to_string()),
                    summary: None,
                },
            };
            let mut queue: VecDeque<_> = story.kids.clone().unwrap_or_default().into();
            let mut captured = vec![story];
            let mut seen = BTreeSet::from([id]);
            for _ in 0..20 {
                let Some(id) = queue.pop_front() else { break };
                if !seen.insert(id) {
                    continue;
                }
                if let Ok(comment) = client.get_item(id).await {
                    // Parent-first depth-first traversal preserves some real reply threads.
                    for child in comment.kids.iter().flatten().rev() {
                        queue.push_front(*child);
                    }
                    captured.push(comment);
                }
            }
            eprintln!(
                "Captured {id}: {} comments, {} article characters{}",
                captured.len() - 1,
                article.text.chars().count(),
                if article.error.is_some() {
                    " (unavailable)"
                } else {
                    ""
                }
            );
            Ok::<_, anyhow::Error>((id, captured, article))
        }
    }))
    .buffer_unordered(6);
    while let Some(result) = captures.next().await {
        let (id, captured, article) = result?;
        for mut item in captured {
            item.title = crate::reader::sources::clean(&item.title);
            item.text = item.text.map(|s| crate::reader::sources::clean(&s));
            items.insert(item.id, item);
        }
        articles.insert(id, article);
    }
    let captured_ids: BTreeSet<_> = items.keys().copied().collect();
    for item in items.values_mut() {
        if let Some(kids) = &mut item.kids {
            kids.retain(|id| captured_ids.contains(id));
        }
    }
    let snapshot = Snapshot {
        version: 1,
        generated_at: chrono::Utc::now().to_rfc3339(),
        feeds,
        items,
        articles,
    };
    snapshot.write_atomic(path)?;
    eprintln!(
        "Published {} stories to {}",
        snapshot.articles.len(),
        path.display()
    );
    Ok(())
}

async fn public_article_client(raw: &str) -> Result<reqwest::Client> {
    let url = reqwest::Url::parse(raw)?;
    ensure!(
        url.scheme() == "https" && url.port_or_known_default() == Some(443),
        "Preview captures HTTPS articles only"
    );
    ensure!(
        url.username().is_empty() && url.password().is_none(),
        "Credentials in URL rejected"
    );
    let host = url.host_str().context("Missing article host")?;
    let addresses: Vec<_> =
        tokio::time::timeout(Duration::from_secs(5), tokio::net::lookup_host((host, 443)))
            .await??
            .filter(|a| match a.ip() {
                std::net::IpAddr::V4(ip) => {
                    let [a, b, _, _] = ip.octets();
                    !(ip.is_private()
                        || ip.is_loopback()
                        || ip.is_link_local()
                        || ip.is_unspecified()
                        || ip.is_broadcast()
                        || ip.is_documentation()
                        || ip.is_multicast()
                        || a == 0
                        || a >= 240
                        || (a == 100 && (64..=127).contains(&b))
                        || (a == 198 && (b == 18 || b == 19)))
                }
                std::net::IpAddr::V6(_) => false,
            })
            .collect();
    ensure!(!addresses.is_empty(), "No public IPv4 article address");
    // Pin the checked addresses; disable proxies and redirects so they cannot bypass this check.
    Ok(reqwest::Client::builder()
        .no_proxy()
        .resolve_to_addrs(host, &addresses)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(12))
        .user_agent("hn-cli-preview/0.1")
        .build()?)
}

/// Enrich a validated capture without replacing it if any selected summary fails.
pub async fn prepare(path: &Path) -> Result<()> {
    let mut snapshot = Snapshot::load(path)?;
    crate::reader::snapshot::prepare(&mut snapshot).await?;
    snapshot.write_atomic(path)
}
