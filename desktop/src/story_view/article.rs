//! Article pane and extracted article content.

use rune::tok::{self, type_};
use rune::{El, ScrollSpec, col, el, row, text};
use rune_kit::{Kit, Markdown, Spinner};

use crate::app::{App, Pane};
use crate::story::{ArticleState, Story};

use super::reading_chrome::skeleton_line;
use super::{FALLBACK_BANNER, HEADLINE, MEASURE};

/// The article pane: a sticky title over the body, at the reading measure.
///
/// In [`Pane::Live`] the body is the child view's region, so the pane is the header plus an
/// empty element that reports its rect through `on_layout`. The browser action stays in the
/// footer outside that native child's bounds.
pub fn article_pane<'a>(s: &Story, k: &Kit, t: tok::Theme, pane: Pane<'a>) -> El {
    let outer = col().key("article-pane").w_full().h_full().child(
        col()
            .key("article-content")
            .w_full()
            .px(28.0)
            .pt(14.0)
            .child(article_header(s, k, t)),
    );
    match pane {
        Pane::Live { loading } => outer
            .child(
                el()
                    .key(format!("article-live-{}", s.id()))
                    .w_full()
                    .grow(1.0)
                    .min_h(0.0)
                    .min_w(0.0)
                    .fill(t.bg)
                    .center()
                    .on_layout(|s: &mut App, rect| s.live_rect(rect))
                    .children(loading.then(|| text("Loading article…").key("article-loading")
                        .role("status").busy(true).font(type_::LABEL).color(t.text_secondary))),
            ),
        Pane::Extracted { reason } => outer
            .child(fallback_banner(reason, t))
            .child(
                col()
                    .key("article-scroll")
                    .w_full()
                    .grow(1.0)
                    .min_h(0.0)
                    .scroll(ScrollSpec { stick_bottom: false })
                    .on_wheel(App::pointer_wheel)
                    .scroll_edge_fade(20.0)
                    .child(
                        col()
                            .key("article-body-content")
                            .w_full()
                            .px(28.0)
                            .py(14.0)
                            .child(article_body(s, k, t)),
                    ),
            ),
    }
}

/// Why the pane is the extracted text and not the page. It is a state, not an error: headless
/// runs and non-macOS builds have no child view to host, and the reader is told which they got.
fn fallback_banner(reason: Option<&str>, t: tok::Theme) -> El {
    let mut banner = row()
        .key("article-fallback")
        .role("status")
        .w_full()
        .px(28.0)
        .py(8.0)
        .gap(10.0)
        .center_y()
        .fill(t.card)
        .child(
            text(FALLBACK_BANNER)
                .font(type_::LABEL)
                .color(t.text_secondary)
                .truncate(),
        );
    if let Some(reason) = reason {
        banner = banner.child(
            text(reason.to_string())
                .key("article-fallback-reason")
                .font(type_::LABEL)
                .color(t.text_secondary)
                .truncate_middle(),
        );
    }
    banner
}

fn article_header(s: &Story, k: &Kit, t: tok::Theme) -> El {
    let story = &s.story;
    col()
        .key("article-header")
        .w_full()
        .gap(8.0)
        .pb(10.0)
        .fill(k.c.bg_base)
        .child(
            text(story.title.clone())
                .key("article-title")
                .w_full()
                .font(HEADLINE)
                .color(t.text)
                .wrap_anywhere(),
        )
        .child(
            row()
                .w_full()
                .gap(12.0)
                .items_start()
                .flex_wrap()
                .child(
                    text(format!(
                        "{}  \u{00b7}  {} points  \u{00b7}  {}  \u{00b7}  {}",
                        story.author,
                        story.score,
                        comments_label(story.comments),
                        host_of(&story.url)
                    ))
                    .key("article-meta")
                    .grow(1.0)
                    .shrink()
                    .min_w(220.0)
                    .font(type_::LABEL)
                    .color(t.text_secondary)
                    .wrap_anywhere(),
                ),
        )
}

fn article_body(s: &Story, k: &Kit, t: tok::Theme) -> El {
    match &s.article {
        ArticleState::Loading => col()
            .key("article-loading")
            .w_full()
            .gap(10.0)
            .child(
                row()
                    .gap(8.0)
                    .center_y()
                    .child(Spinner::new().size(12.0).color(t.text_secondary).build(k))
                    .child(
                        text("Extracting the article\u{2026}")
                            .font(type_::LABEL)
                            .color(t.text_secondary),
                    ),
            )
            .children((0..6).map(|n| skeleton_line(n, t))),
        ArticleState::Failed(message) => col()
            .key("article-failed")
            .w_full()
            .gap(6.0)
            .child(
                text("Could not read this article")
                    .font(type_::TITLE)
                    .color(t.text),
            )
            .child(
                text(message.clone())
                    .key("article-failed-message")
                    .font(type_::LABEL)
                    .color(t.text_secondary)
                    .wrap(),
            )
            .child(
                text("The discussion is still readable below.")
                    .font(type_::LABEL)
                    .color(t.text_secondary),
            ),
        ArticleState::Ready {
            text: markdown,
            partial,
        } => {
            let mut body = col()
                .key("article-body")
                .w_full()
                .gap(10.0)
                .child(Markdown::new(markdown.clone()).selectable(true).build(k).max_w(MEASURE));
            if *partial {
                body = body.child(
                    text("Truncated \u{00b7} the article is longer than the reader keeps.")
                        .key("article-partial")
                        .font(type_::LABEL)
                        .color(t.text_secondary),
                );
            }
            body
        }
    }
}

fn comments_label(comments: Option<i32>) -> String {
    // The terminal app's wording, singular included.
    match comments {
        Some(1) => "1 comment".to_string(),
        Some(n) => format!("{n} comments"),
        None => "discuss".to_string(),
    }
}

/// The host a story links to, for the article metadata. The URL is the story's, not the
/// article's: the reader follows redirects and does not report where it landed.
pub(super) fn host_of(url: &str) -> String {
    let rest = url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(url);
    let host = rest.split('/').next().unwrap_or(rest);
    let host = host.strip_prefix("www.").unwrap_or(host);
    if host.is_empty() {
        "the article".to_string()
    } else {
        host.to_string()
    }
}

