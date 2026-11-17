//! The story split's view: the article pane, the pinned summary card, the comment tree.
//!
//! Every function here is a pure function of [`Story`] plus the theme, so a scene, a scripted
//! key and a landed request all draw the same tree. The card's four states are
//! [`SummaryState`]'s; this module only draws them.
//!
//! The discussion is DESIGN.md's "Comments: reading a thread you did not start", drawn from
//! [`crate::comments::Thread`]: a tree with one indent guide per ancestor, a sticky ancestor
//! trail above the focused comment, a depth cap that turns deeper replies into
//! `continue thread → N replies`, a focus rail on the selected comment, and a
//! folded comment's derived `N replies hidden`. The state is the thread's; this module only
//! draws it and hands presses back to it.
//!
//! The article is `rune_kit::Markdown`, so headings, lists, quotes and fenced code are the
//! kit's, including its syntax colouring. Nothing here parses Markdown.

use std::f32::consts::FRAC_PI_2;
use std::rc::Rc;

use hn_lib::reader::Source as CapturedSource;
use rune::tok::{self, radius, type_};
use rune::{Color, El, GrowthSpec, Link, LinkPaint, ScrollSpec, Span, Spring, SpringPair, TextStyle, Transition, Tx, col, el, icon, row, text};
use rune_kit::{Keybind, Kit, Markdown, Placement, Popover, Spinner};

use crate::app::App;
use crate::comments::{INDENT, MAX_DEPTH};
use crate::story::{ArticleState, ArticleSummaryState, Story, Summary, SummaryState};

/// The article's measure: 66 characters at [`type_::BODY`] (13 pt), 6.8 pt per character.
/// DESIGN.md's reading measure, and the reason the article column does not fill the pane.
pub const MEASURE: f32 = 448.8;

/// The article's headline. `type_::TITLE` is a row title; a headline is a step above it.
const HEADLINE: TextStyle = TextStyle::new(21.0, 600.0);

/// The fallback banner's text. The pane is the live page; this is what the reader sees when a
/// child view cannot exist (headless, non-macOS) or could not be built.
pub const FALLBACK_BANNER: &str = "live page unavailable \u{2014} showing extracted text";

/// What the article pane draws.
///
/// The live page is the default: the pane is a placeholder whose rect the child view is pinned
/// to, and the page itself is drawn by the web engine, not by Rune. The extracted Markdown is
/// the fallback, and it says so — a silent swap would leave the reader unsure whether they are
/// looking at the page or at a capture of it.
pub enum Pane<'a> {
    /// The story's real page, in a child view over the pane's rect.
    Live { loading: bool },
    /// The extracted text, with a banner saying why the live page is not there. `reason` is the
    /// child view's own failure, when there was one.
    Extracted { reason: Option<&'a str> },
}

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
                .color(t.text_secondary),
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
                .child(Markdown::new(markdown.clone()).build(k).max_w(MEASURE));
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

/// The inspector: the summary card pinned at the top, the discussion below it, or the source
/// viewer in their place.
///
/// The card and the discussion are siblings in a column that does not scroll, so the card is
/// pinned by layout rather than by `sticky_top`: the discussion has its own viewport and moves
/// under the card, and the card never moves at all. `overlays` takes the link chooser's surface,
/// which is anchored to a chip in the discussion header.
pub fn inspector_pane(
    s: &Story,
    k: &Kit,
    t: tok::Theme,
    accent: Color,
    overlays: &mut Vec<El>,
    reduced: bool,
    summary_height: f32,
    on_scroll: [impl Fn(rune::ScrollPos) + 'static; 3],
) -> El {
    let transition = reading_transition(reduced);
    let reflow = if reduced { Spring::NONE } else { Spring::new(0.18, 0.0) };
    let [discussion_scroll, summary_scroll, source_scroll] = on_scroll;
    let source = s.shown_source();
    let hidden = source.is_some();
    let back = if hidden || reduced { Spring::NONE } else { Spring::new(0.22, 0.0) };
    let mut inspector = col()
        .key("inspector")
        .transition(Transition::none())
        .opacity(if hidden { 0.0 } else { 1.0 })
        .offset(if hidden && !reduced { -16.0 } else { 0.0 }, 0.0)
        .opacity_motion(back)
        .offset_motion(back)
        .w_full()
        .h_full()
        .child(
            col()
                .key("inspector-content")
                .transition(Transition::none())
                .w_full()
                .h_full()
                .px(14.0)
                .pb(12.0)
                .gap(10.0)
                .child(summary_card(s, k, t, accent, reduced, summary_height, summary_scroll))
                .child(
                    discussion_pane(s, k, t, accent, overlays, reduced, discussion_scroll)
                        .transition(transition)
                        .motion(reflow)
                        .grow(1.0)
                        .min_h(0.0),
                ),
        );
    if hidden {
        // Keep the scroll viewport mounted, but exclude it from input and accessibility.
        inspector = inspector.inert().a11y_hidden();
    }
    let mut stage = col().key("inspector-stage").w_full().h_full().child(inspector);
    if let Some(source) = source {
        let enter = if reduced { Transition::none() } else {
            let mut enter = Transition::asymmetric(
                Transition::offset(16.0, 0.0).combined(Transition::opacity()), Transition::none(),
            ).springs(SpringPair::new(Spring::new(0.22, 0.0), Spring::NONE));
            enter.insertion.opacity = 0.4;
            enter
        };
        stage = stage.child(source_viewer(s, source, k, t, accent, reduced, source_scroll)
            .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).transition(enter));
    }
    stage
}

/// A new reading surface arrives together; the old text must leave before it can overlap it.
fn reading_transition(reduced: bool) -> Transition {
    if reduced {
        return Transition::none();
    }
    let mut transition = Transition::asymmetric(Transition::opacity(), Transition::none())
        .springs(SpringPair::new(Spring::new(0.15, 0.0), Spring::NONE));
    transition.insertion.opacity = 0.65;
    transition
}

/// The summary card holds two independently generated sections above the discussion.
fn summary_card(s: &Story, k: &Kit, t: tok::Theme, accent: Color, reduced: bool, max_height: f32,
    on_scroll: impl Fn(rune::ScrollPos) + 'static) -> El {
    let reflow = if reduced { Spring::NONE } else { Spring::new(0.18, 0.0) };
    let content = if s.summary_open {
        expanded_summary(s, k, t, accent, reduced, max_height, on_scroll)
    } else {
        folded_summary(t)
    };
    col()
        .key("summary-card")
        .transition(Transition::none())
        .role("region")
        .label("Summary")
        .w_full()
        .radius(radius::MD)
        .fill(t.card)
        .border(1.0, t.card_border)
        .clip_rounded(radius::MD)
        .motion(reflow)
        .child_exit(Transition::none())
        // The card stays pinned while the discussion has its own viewport.
        .sticky_top(12.0)
        .child(content.transition(reading_transition(reduced)).instant_changes())
}

fn expanded_summary(s: &Story, k: &Kit, t: tok::Theme, accent: Color, reduced: bool, max_height: f32,
    on_scroll: impl Fn(rune::ScrollPos) + 'static) -> El {
    let mut body = col().w_full().gap(14.0);
    if matches!(s.article_summary, ArticleSummaryState::Unconfigured)
        && matches!(s.summary, SummaryState::Unconfigured)
    {
        body = body.child(unconfigured(t, accent));
    } else {
        let article_waiting = matches!(&s.article_summary, ArticleSummaryState::Generating { previous: None });
        let article_motion = if reduced || article_waiting { Spring::NONE } else { Spring::new(0.20, 0.0) };
        let mut article = col().key("article-summary-content").w_full().gap(7.0)
            .transition(Transition::none()).instant_changes();
        if let Some(value) = s.article_summary.readable() {
            article = article.child(text(value.to_string()).key("article-summary").font(type_::BODY).color(t.text).wrap());
        }
        match &s.article_summary {
            ArticleSummaryState::Generating { previous } if previous.is_none() => {},
            ArticleSummaryState::Generating { .. } => article = article.child(text("Updating article summary…").role("status").font(type_::LABEL).color(t.text_secondary)),
            ArticleSummaryState::Failed { message, .. } => article = article.child(text(format!("Article summary unavailable: {message}")).role("alert").font(type_::LABEL).color(t.text_secondary).wrap())
                .child(row().role("button").label("Retry summaries").focusable(true).on_press(|s: &mut App| s.retry_summary())
                    .child(text("Retry summaries").font(type_::LABEL).color(t.text))),
            _ => {}
        }
        let discussion_waiting = matches!(&s.summary, SummaryState::Generating { previous: None });
        let discussion_motion = if reduced || discussion_waiting { Spring::NONE } else { Spring::new(0.20, 0.0) };
        let mut discussion = col().key("discussion-summary-content").w_full().gap(7.0)
            .transition(Transition::none()).instant_changes();
        if let Some(summary) = s.summary.readable() {
            discussion = discussion.child(bullets(s, summary, t, accent));
        }
        match &s.summary {
            SummaryState::Generating { previous } if previous.is_none() => {},
            SummaryState::Generating { .. } => discussion = discussion.child(text("Updating discussion summary…").role("status").font(type_::LABEL).color(t.text_secondary)),
            SummaryState::Failed { message, auth, .. } => discussion = discussion.child(failure(message, *auth, t, accent)),
            _ => {}
        }
        body = body
            .child(col().w_full().gap(6.0)
            .child(text("Article").key("article-summary-heading").role("heading").level(3).font(type_::SECTION).color(t.text_secondary))
            .child(col().key("article-summary-body").w_full().child_exit(Transition::none())
                .child(col().key("article-summary-result").w_full().transition(Transition::none()).animated_changes()
                    .motion(Spring::NONE).opacity(if article_waiting && !reduced { 0.4 } else { 1.0 })
                    .offset(0.0, if article_waiting && !reduced { 8.0 } else { 0.0 })
                    .opacity_motion(article_motion).offset_motion(article_motion).child(article))
                .children(article_waiting.then(|| generating(k, t, "Reading the article…")))))
            .child(col().w_full().gap(6.0)
            .child(text("Discussion").key("discussion-summary-heading").role("heading").level(3).font(type_::SECTION).color(t.text_secondary))
            .child(col().key("discussion-summary-body").w_full().child_exit(Transition::none())
                .child(col().key("discussion-summary-result").w_full().transition(Transition::none()).animated_changes()
                    .motion(Spring::NONE).opacity(if discussion_waiting && !reduced { 0.4 } else { 1.0 })
                    .offset(0.0, if discussion_waiting && !reduced { 8.0 } else { 0.0 })
                    .opacity_motion(discussion_motion).offset_motion(discussion_motion).child(discussion))
                .children(discussion_waiting.then(|| generating(k, t, "Reading the discussion…")))));
    }
    let more = rune::Live::new(0.0_f32);
    let on_more = more.clone();
    col().key("summary-open").w_full().gap(10.0).px(14.0).pt(14.0).pb(20.0)
        .child(summary_header(t))
        .child(col().key("summary-scroll").role("region").label("Summary content").focusable(true)
            .on_key(|s: &mut App, ev: &rune::KeyEvent, tx: &mut Tx| match ev.key {
                rune::Key::PageDown => { s.keyboard_navigation(ev); tx.scroll_page("summary-scroll", 1); true }
                rune::Key::PageUp => { s.keyboard_navigation(ev); tx.scroll_page("summary-scroll", -1); true }
                _ => false,
            }).w_full().min_h(0.0).shrink()
            .max_h((max_height - 62.0).max(80.0)).scroll(ScrollSpec { stick_bottom: false }).on_scroll_offset(on_scroll)
            .on_wheel(App::pointer_wheel)
            .chain_scroll_offset(move |at| { on_more.set(if at.max[1] > at.offset[1] + 1.0 { 1.0 } else { 0.0 }); })
            .scroll_edge_fade(12.0).child(body))
        .child(row().key("summary-more").pin(None, None, Some(14.0), Some(2.0))
            .live_opacity(&more).a11y_hidden()
            .child(text("More below ↓").font(type_::LABEL).color(t.text_secondary)))
}

fn summary_header(t: tok::Theme) -> El {
    row()
        .key("summary-header")
        .w_full()
        .gap(8.0)
        .center_y()
        .child(
            text("Summary")
                .key("summary-title")
                .font(type_::TITLE)
                .color(t.text),
        )
        .child(el().grow(1.0))
        .child(row().role("button").label("Hide summary").focusable(true)
            .on_press(|s: &mut App| { if let Some(story) = s.story_mut() { story.toggle_summary(); } })
            .child(text("Hide").font(type_::LABEL).color(t.text_secondary)))
}

/// Each source reference is a pressable inline range in the point it supports.
fn bullets(s: &Story, summary: &Summary, t: tok::Theme, accent: Color) -> El {
    let citations = s.citations();
    let mut list = col().key("summary-points").w_full().gap(10.0);
    for (index, bullet) in summary.bullets.iter().enumerate() {
        let mut content = bullet.text.clone();
        let mut spans = Vec::new();
        let mut links = Vec::new();
        for id in &bullet.sources {
            let Some(position) = citations.iter().position(|cited| cited == id) else { continue; };
            let Some(source) = s.sources.iter().find(|source| source.id == *id) else { continue; };
            content.push(' ');
            let start = content.len();
            content.push_str(&format!("[{}]", position + 1));
            let end = content.len();
            let selected = s.citation_focus && s.citation == position;
            spans.push(Span::new(start, end).color(if selected { t.text } else { t.text_secondary }).underline(accent));
            links.push(Link::new(start..end, format!("cite-{position}"))
                .role("button").label(format!("Read comment by {}", source.author))
                .hover(LinkPaint::new().color(t.text).underline(accent)));
        }
        let mut paragraph = text(content).key(format!("summary-text-{index}"))
            .grow(1.0).min_w(0.0).font(type_::BODY).color(t.text).wrap().spans(spans);
        for link in links { paragraph = paragraph.link(link); }
        paragraph = paragraph.on_link(|s: &mut App, id, _| {
            if let Some(index) = id.strip_prefix("cite-").and_then(|value| value.parse().ok()) {
                s.open_citation(index);
            }
        });
        list = list.child(
            row()
                .key(format!("summary-point-{index}"))
                .w_full()
                .gap(8.0)
                .child(text("•").font(type_::BODY).color(accent))
                .child(paragraph),
        );
    }
    list
}

fn generating(k: &Kit, t: tok::Theme, label: &str) -> El {
    col()
        .key("summary-generating")
        .w_full()
        .gap(10.0)
        .child(
            row()
                .gap(8.0)
                .center_y()
                .child(Spinner::new().size(12.0).color(t.text_secondary).build(k))
                .child(
                    text(label.to_string())
                        .key("summary-status")
                        .font(type_::LABEL)
                        .color(t.text_secondary),
                ),
        )
        .children((0..2).map(|n| skeleton_line(n, t)))
}

/// A skeleton row: a bar whose width varies so the block reads as text, not as a table.
fn skeleton_line(n: usize, t: tok::Theme) -> El {
    let width = [1.0, 0.86, 0.62, 0.94, 0.48][n % 5];
    el()
        .key(format!("skeleton-{n}"))
        .w_full()
        .h(9.0)
        .radius(4.0)
        .fill(t.control_off)
        .opacity(0.35 + 0.5 * width)
}

fn failure(message: &str, auth: bool, t: tok::Theme, accent: Color) -> El {
    col()
        .key("summary-failed")
        .role("alert")
        .w_full()
        .gap(8.0)
        .child(
            text("Discussion summary failed")
                .font(type_::TITLE)
                .color(t.text),
        )
        .child(
            text(message.to_string())
                .key("summary-failed-message")
                .font(type_::LABEL)
                .color(t.text_secondary)
                .wrap(),
        )
        .child(
            row()
                .gap(8.0)
                .child(
                    row()
                        .key("summary-retry")
                        .role("button")
                        .label("Retry")
                        .h(22.0)
                        .px(10.0)
                        .center()
                        .radius(4.0)
                        .fill(accent)
                        .focusable(true)
                        .on_press(|s: &mut App| s.retry_summary())
                        .child(text("Retry").font(type_::LABEL).color(t.bg)),
                )
                .child(
                    row()
                        .key("summary-settings")
                        .role("button")
                        .label(if auth { "Add API key" } else { "Change provider" })
                        .h(22.0)
                        .px(10.0)
                        .center()
                        .radius(4.0)
                        .fill(t.control_off)
                        .focusable(true)
                        .on_press(|s: &mut App| s.open_summary_settings())
                        .child(
                            text(if auth { "Add API key" } else { "Change provider" })
                                .font(type_::LABEL)
                                .color(t.text_secondary),
                        ),
                ),
        )
}

fn unconfigured(t: tok::Theme, accent: Color) -> El {
    col()
        .key("summary-unconfigured")
        .w_full()
        .gap(6.0)
        .child(
            text("No summary yet")
                .font(type_::TITLE)
                .color(t.text),
        )
        .child(
            text("Open summary settings to add an API key or generate a summary.")
                .key("summary-unconfigured-hint")
                .font(type_::LABEL)
                .color(t.text_secondary)
                .wrap(),
        )
        .child(
            row()
                .key("summary-settings")
                .role("button")
                .label("Open settings")
                .h(22.0)
                .px(10.0)
                .center()
                .radius(4.0)
                .fill(accent)
                .focusable(true)
                .on_press(|s: &mut App| s.open_summary_settings())
                .child(text("Open settings").font(type_::LABEL).color(t.bg)),
        )
}

/// A compact, clickable control returns the space to the summary.
fn folded_summary(t: tok::Theme) -> El {
    row()
        .key("summary-folded")
        .role("button").label("Show summary").focusable(true)
        .on_press(|s: &mut App| { if let Some(story) = s.story_mut() { story.toggle_summary(); } })
        .w_full()
        .gap(8.0)
        .px(14.0)
        .py(10.0)
        .center_y()
        .child(
            text("Summary")
                .key("summary-title")
                .font(type_::TITLE)
                .color(t.text),
        )
        .child(el().grow(1.0))
        .child(
            text("Show")
                .font(type_::LABEL)
                .color(t.text_secondary),
        )
}

/// The discussion's viewport: the tree scrolls here, under the pinned card, and nowhere else.
fn discussion_pane(
    s: &Story,
    k: &Kit,
    t: tok::Theme,
    accent: Color,
    overlays: &mut Vec<El>,
    reduced: bool,
    on_scroll: impl Fn(rune::ScrollPos) + 'static,
) -> El {
    col()
        .key("discussion-scroll")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec { stick_bottom: false })
        .on_wheel(App::pointer_wheel)
        .scroll_edge_fade(20.0)
        .scroll_margin(20.0)
        .on_scroll_offset(on_scroll)
        .child(discussion(s, k, t, accent, overlays, reduced).motion(Spring::NONE))
}

/// The discussion: one sticky header holding its ancestor trail, then the visible rows.
fn discussion(s: &Story, k: &Kit, t: tok::Theme, accent: Color, overlays: &mut Vec<El>, reduced: bool) -> El {
    let thread = &s.thread;
    let mut list = col()
        .key("discussion")
        .w_full()
        .gap(2.0)
        .child(discussion_header(s, k, t, accent, overlays));
    if s.refreshing_comments {
        list = list.child(text("Refreshing comments…").key("discussion-refreshing").role("status").font(type_::LABEL).color(t.text_secondary));
    }
    if let Some(error) = &s.refresh_error {
        list = list.child(text(format!("Refresh incomplete: {error}")).role("alert").font(type_::LABEL).color(t.text_secondary).wrap());
    }
    if thread.is_empty() {
        let message = match &s.discussion {
            None => "Loading discussion…",
            Some(Ok(())) => "No readable comments yet.",
            Some(Err(message)) => message,
        };
        list = list.child(text(message).key("discussion-empty").font(type_::LABEL).color(t.text_secondary));
        if matches!(s.discussion, Some(Err(_))) {
            list = list.child(row().key("discussion-retry").role("button").label("Retry discussion")
                .focusable(true).on_press(|s: &mut App| s.refresh_comments())
                .child(text("Retry discussion").font(type_::LABEL).color(accent)));
        }
        return list;
    }
    if let Some(Err(message)) = &s.discussion {
        list = list.child(text(message.clone()).role("alert").font(type_::LABEL).color(t.text_secondary))
            .child(row().role("button").label("Retry discussion").focusable(true)
                .on_press(|s: &mut App| s.refresh_comments())
                .child(text("Retry discussion").font(type_::LABEL).color(accent)));
    }
    let rows = Rc::new(thread.rows());
    let keys = Rc::clone(&rows);
    let kit = (*k).clone();
    list.child(col().key("discussion-rows").w_full().gap(2.0).instant_changes()
        .growth(GrowthSpec { spring: Spring::NONE, ..GrowthSpec::default() }).virtual_rows(rows.len(), 110.0,
            move |_s: &App, i| format!("comment-{}", keys[i].id).into(),
            move |app: &App, i| app.story().map(|s| comment_row(s, rows[i].id, rows[i].depth, &kit, t, accent, false, reduced)).unwrap_or_else(el)))
}

/// The discussion's header: what the capture holds, whether focus mode is on, and `L`'s chip.
fn discussion_header(
    s: &Story,
    k: &Kit,
    t: tok::Theme,
    accent: Color,
    overlays: &mut Vec<El>,
) -> El {
    let thread = &s.thread;
    let trail = thread.trail();
    let leading = if trail.len() > 1 {
        trail_strip(s, &trail, t)
    } else {
        row().child(text("Discussion").font(type_::SECTION).color(t.text_secondary))
    };
    let mut header = row()
        .key("discussion-header")
        .w_full()
        .h(27.0)
        .px(2.0)
        .gap(8.0)
        .center_y()
        .fill(t.bg)
        .sticky_top(0.0)
        .instant_changes()
        .child(leading.grow(1.0).shrink().min_w(0.0));
    header = header.child(row().key("discussion-focus").role("button")
        .label(if thread.focus() { "Turn focus mode off" } else { "Turn focus mode on" })
        .focusable(true).on_press(|s: &mut App| { if let Some(story) = s.story_mut() { story.thread.toggle_focus(); } })
        .child(text(if thread.focus() { "Focus on" } else { "Focus off" }).font(type_::LABEL).color(t.text_secondary)));
    let has_links = !s.story.links.is_empty() || thread.cursor().and_then(|id| thread.node(id)).is_some_and(|node| !node.links.is_empty());
    if has_links { header = header.child(link_chooser(s, k, t, accent, overlays)); }
    header
}

/// The ancestor trail: `tptacek › kstrauser › jmillikin`, held to the top of the discussion's
/// viewport so "where am I" never scrolls away. Every segment but the last re-roots there.
fn trail_strip(s: &Story, trail: &[i32], t: tok::Theme) -> El {
    let last = trail.len() - 1;
    let mut strip = row()
        .key("trail")
        .role("region")
        .label("Thread trail")
        .w_full()
        .gap(6.0)
        .center_y()
        .motion(Spring::NONE);
    for (at, id) in trail.iter().enumerate() {
        if at > 0 {
            strip = strip.child(
                text("\u{203a}")
                    .font(type_::LABEL)
                    .color(t.text_secondary),
            );
        }
        let author = s.thread.node(*id).map(|node| {
            if node.readable { node.author.clone() } else { "Replies".to_string() }
        }).unwrap_or_default();
        if at == last {
            // The comment you are reading: named, not a target.
            strip = strip.child(
                text(author)
                    .key(format!("trail-{id}"))
                    .shrink().min_w(0.0).truncate()
                    .font(type_::LABEL)
                    .color(t.text),
            );
        } else {
            let target = *id;
            strip = strip.child(
                row()
                    .key(format!("trail-{id}"))
                    .role("button")
                    .label(format!("Back to {author}"))
                    .shrink().min_w(0.0)
                    .radius(3.0)
                    .focusable(true)
                    .on_press(move |s: &mut App| {
                        if let Some(story) = s.story_mut() {
                            story.thread.reroot(target);
                        }
                    })
                    .child(text(author).shrink().min_w(0.0).truncate().font(type_::LABEL).color(t.text_secondary)),
            );
        }
    }
    strip
}

/// One comment: its indent guides, its header, its text, and what it is hiding.
///
/// The row is the fold target and the cursor target at once. Folding keeps the comment readable;
/// a comment at the depth cap offers `continue thread` instead of drawing deeper replies.
fn comment_row(s: &Story, id: i32, depth: u32, k: &Kit, t: tok::Theme, accent: Color, context: bool, reduced: bool) -> El {
    let thread = &s.thread;
    let captured = context.then(|| s.shown_source()).flatten().filter(|source| source.id == id);
    let node = thread.node(id);
    if node.is_none() && captured.is_none() { return el(); }
    let nav = context.then(|| &s.source.as_ref().unwrap().nav);
    let cursor = nav.map_or_else(|| thread.cursor(), |nav| nav.cursor);
    let focus = nav.map_or_else(|| thread.focus(), |nav| nav.focus);
    let selected = cursor == Some(id);
    let folded = nav.map_or_else(|| thread.is_folded(id), |nav| nav.folded.contains(&id));
    let capped = if context { depth + 1 >= MAX_DEPTH && thread.has_children(id) } else { thread.capped(id) };
    let readable = captured.is_some() || node.is_some_and(|node| node.readable);
    let author = captured.map(|source| source.author.as_str()).or_else(|| node.map(|node| node.author.as_str())).unwrap_or("");
    let body = captured.map(|source| source.text.as_str()).or_else(|| node.map(|node| node.text.as_str())).unwrap_or("");
    let replies = thread.child_count(id);
    let loaded = node.is_some_and(|node| !node.children.is_empty());
    let loading = s.pending_reply.as_ref().is_some_and(|r| r.parent_id == id);
    let expanded = (loaded || loading) && !folded && !capped;
    let mut r = col()
        .key(format!("{}comment-{id}", if context { "source-" } else { "" }))
        .role(if readable { "article" } else { "group" })
        .label(if readable { format!("Comment by {author}") } else { "Replies".to_string() })
        .level(depth + 1)
        .w_full()
        .gap(4.0)
        .pl(8.0 + depth as f32 * INDENT)
        .pr(10.0)
        .py(8.0)
        .radius(radius::SM)
        .focusable(true)
        .focus_selected(selected)
        .focus_ring(1.5, 0.0, accent)
        .animated_changes()
        .motion(Spring::NONE)
        .transition(if depth == 0 || reduced { Transition::none() } else {
            Transition::asymmetric(Transition::opacity(), Transition::none())
                .springs(SpringPair::new(Spring::new(0.15, 0.0), Spring::NONE))
        });
    r = r.on_press(move |s: &mut App| {
        if let Some(story) = s.story_mut() {
            if !context { story.citation_focus = false; }
            story.with_thread(|thread| { thread.select(id); });
        }
    }).on_pointer_move_local(move |s: &mut App, _, tx| {
        if let Some(story) = s.story_mut() {
            let changed = story.with_thread(|thread| thread.focus() && thread.cursor() != Some(id) && thread.select(id));
            if changed && !context { story.citation_focus = false; }
            if changed { tx.instant(); } else { tx.live_only(); }
        }
    });
    if selected && depth == 0 {
        r = r.child(el().key(format!("focus-rail-{id}"))
            .pin(Some(5.0), Some(7.0), None, Some(7.0)).w(2.0).fill(accent));
    }
    if replies > 0 { r = r.expanded(expanded); }
    // Selection colors the nearest guide instead of adding a second parallel bar.
    for level in 1..=depth {
        let active = selected && level == depth;
        r = r.child(
            el()
                .key(if active { format!("focus-rail-{id}") } else { format!("guide-{id}-{level}") })
                .pin(
                    Some(14.0 + (level as f32 - 1.0) * INDENT),
                    Some(0.0),
                    None,
                    Some(0.0),
                )
                .w(if active { 2.0 } else { 1.0 })
                .fill(if active { accent } else { t.text_secondary.alpha(0.5) }),
        );
    }
    let retry_primary = !readable && !loaded && !folded && !capped && !loading
        && thread.reply_batch(id, false).is_empty() && !thread.reply_batch(id, true).is_empty();
    let action = if capped { "Continue thread" } else if folded { "Show replies" }
        else if retry_primary { "Retry replies" } else if !loaded && !loading { "Load replies" }
        else { "Hide replies" };
    if !readable {
        let noun = if replies == 1 { "reply" } else { "replies" };
        r = r.child(
            row()
                .key(format!("comment-group-{id}"))
                .role("button")
                .label(format!("{action}: {replies} {noun}"))
                .h(28.0)
                .gap(6.0)
                .center_y()
                .self_start()
                .focusable(true)
                .disabled(s.pending_reply.is_some() && !loading && !loaded && !folded && !capped)
                .on_press(move |s: &mut App| s.activate_comment(id))
                .child(icon(k.glyph.chevron_down, 20.0)
                    .icon_rotation(if expanded { 0.0 } else { -FRAC_PI_2 })
                    .motion(Spring::NONE)
                    .color(t.text_secondary))
                .child(text(format!("{action} ({replies})")).font(type_::LABEL).color(t.text_secondary)),
        );
    } else {
        let mut who = row()
            .key(format!("comment-who-{id}"))
            .w_full()
            .gap(6.0)
            .center_y();
        if replies > 0 || capped {
            who = who.child(
                row().size(20.0, 20.0)
                    .child(icon(k.glyph.chevron_down, 20.0).key(format!("comment-chevron-{id}"))
                        .icon_rotation(if expanded { 0.0 } else { -FRAC_PI_2 })
                        .motion(if reduced { Spring::NONE } else { Spring::new(0.12, 0.0) })
                        .color(t.text_secondary).a11y_hidden())
                    .child(el().key(format!("comment-disclosure-{id}"))
                        .absolute(-4.0, -4.0).size(28.0, 28.0)
                        .role("button").label(action).focusable(true)
                        .on_press(move |s: &mut App| s.activate_comment(id))),
            );
        } else {
            // A leaf keeps the chevron's lane, so authors line up down the thread.
            who = who.child(el().size(20.0, 20.0));
        }
        who = who
            .child(
                text(author.to_string())
                    .key(format!("comment-author-{id}"))
                    .shrink()
                    .min_w(0.0)
                    .font(type_::TITLE)
                    .color(if focus && !thread.in_chain_from(id, cursor) { t.text_secondary } else { t.text })
                    .truncate(),
            )
            .child(
                text(node.map_or("", |node| node.time_ago.as_str()).to_string())
                    .key(format!("comment-age-{id}"))
                    .font(type_::LABEL)
                    .color(t.text_secondary),
            );
        if folded && !capped {
            let hidden = if context { thread.hidden_count_at(id, depth) } else { thread.hidden_count(id) }.max(replies);
            let noun = if hidden == 1 { "reply" } else { "replies" };
            who = who.child(
                row()
                    .key(format!("comment-hidden-{id}"))
                    .role("button")
                    .label(format!("Show {hidden} hidden {noun}"))
                    .h(18.0)
                    .px(6.0)
                    .center()
                    .radius(4.0)
                    .fill(t.control_off)
                    .focusable(true)
                    .on_press(move |s: &mut App| s.activate_comment(id))
                    .child(text(format!("{hidden} hidden {noun}")).font(type_::LABEL).color(t.text_secondary)),
            );
        }
        r = r.child(who.child(el().grow(1.0)))
            .child(
                text(body.to_string())
                    .key(if captured.is_some() { "source-comment-text".to_string() } else { format!("comment-text-{id}") })
                    .ml(26.0)
                    .font(type_::BODY)
                    .color(if !focus || selected { t.text } else { t.text_secondary })
                    .wrap_anywhere(),
            );
    }
    if capped && readable {
        let behind = thread.reply_count(id).max(replies);
        r = r.child(
            row()
                .key(format!("comment-continue-{id}"))
                .role("button")
                .label(format!("Continue thread, {behind} replies"))
                .h(20.0)
                .ml(26.0)
                .px(8.0)
                .center_y()
                .self_start()
                .radius(4.0)
                .fill(t.control_off)
                .focusable(true)
                .on_press(move |s: &mut App| {
                    s.activate_comment(id);
                })
                .child(
                    text(format!("continue thread \u{2192} {behind} replies"))
                        .font(type_::LABEL)
                        .color(accent),
                ),
        );
    }
    if !folded && !capped && replies > 0 {
        let indent = if readable { 26.0 } else { 0.0 };
        if loading {
            r = r.child(text("Loading replies…").ml(indent).role("status").busy(true)
                .transition(Transition::none()).font(type_::LABEL).color(t.text_secondary));
        } else {
            for (retry, label) in [(false, "Load replies"), (true, "Retry replies")] {
                if !readable && !loaded && (!retry || retry_primary) { continue; }
                let count = thread.reply_batch(id, retry).len();
                if count == 0 { continue; }
                let label = if !retry && !thread.children_of(id).is_empty() { "Load more replies" } else { label };
                let label = format!("{label} ({count})");
                r = r.child(row().key(format!("comment-load-{id}-{retry}"))
                    .ml(indent).self_start().role("button").label(label.clone()).focusable(true).disabled(s.pending_reply.is_some())
                    .on_press(move |s: &mut App| s.load_replies(id, retry))
                    .child(text(label).font(type_::LABEL).color(accent)));
            }
            if let Some(crate::comments::BranchStatus::Failed { message, .. }) = node.map(|node| &node.branch) {
                r = r.child(text(message.clone()).ml(indent).role("status").font(type_::LABEL).color(t.text_secondary));
            }
        }
    }
    r
}

/// `L`'s chooser: the links the post and the focused comment carry, anchored to a chip in the
/// discussion header. The surface goes into the overlay layer; the chip is what it hangs from.
fn link_chooser(
    s: &Story,
    k: &Kit,
    t: tok::Theme,
    accent: Color,
    overlays: &mut Vec<El>,
) -> El {
    let mut pop = Popover::new("links")
        .label("Links")
        .placement(Placement::BottomEnd)
        .open(s.links.is_some())
        .on_close(|s: &mut App| {
            if let Some(story) = s.story_mut() {
                story.close_links();
            }
        });
    if let Some(chooser) = &s.links {
        for (index, item) in chooser.items.iter().enumerate() {
            let selected = index == chooser.selected;
            pop = pop.child(
                row()
                    .key(format!("link-{index}"))
                    .role("button")
                    .label(format!("{}: {}", item.source, item.url))
                    .w_full()
                    .gap(8.0)
                    .px(8.0)
                    .py(6.0)
                    .radius(4.0)
                    .fill(if selected { accent } else { t.control_off })
                    .focusable(true)
                    .focus_selected(selected)
                    .on_press_tx(move |s: &mut App, tx: &mut Tx| {
                        let Some(story) = s.story_mut() else {
                            return;
                        };
                        if let Some(chooser) = &mut story.links {
                            chooser.selected = index;
                        }
                        if let Some(url) = story.take_link() {
                            let _ = tx.open_url(&url);
                        }
                    })
                    .child(
                        col()
                            .grow(1.0)
                            .min_w(0.0)
                            .gap(2.0)
                            .child(
                                text(item.source.clone())
                                    .key(format!("link-source-{index}"))
                                    .font(type_::LABEL)
                                    .color(if selected { t.bg } else { t.text_secondary }),
                            )
                            .child(
                                text(item.url.clone())
                                    .key(format!("link-url-{index}"))
                                    .font(type_::LABEL)
                                    .color(if selected { t.bg } else { t.text })
                                    .truncate_middle(),
                            ),
                    ),
            );
        }
    }
    let trigger = row()
        .key("links-chip")
        .role("button")
        .label("Links")
        .h(18.0)
        .px(6.0)
        .gap(4.0)
        .center_y()
        .radius(4.0)
        .fill(t.control_off)
        .focusable(true)
        .on_press(|s: &mut App| s.choose_links())
        .child(Keybind::new(&["⇧", "L"]).build(k))
        .child(text("links").font(type_::LABEL).color(t.text_secondary));
    pop.trigger(trigger).build(k, overlays)
}

/// The captured comment and its current reply context share the discussion's thread data.
fn source_viewer(
    s: &Story,
    source: &CapturedSource,
    k: &Kit,
    t: tok::Theme,
    accent: Color,
    reduced: bool,
    on_scroll: impl Fn(rune::ScrollPos) + 'static,
) -> El {
    let thread = &s.thread;
    let mut content = col().key("source-content").w_full().px(28.0).py(14.0).gap(10.0)
        .child(text("Discussion summary").key("source-summary-heading").font(type_::LABEL).color(t.text_secondary));
    if let Some(summary) = s.summary.readable() {
        for (index, bullet) in summary.bullets.iter().enumerate().filter(|(_, bullet)| bullet.sources.contains(&source.id)) {
            content = content.child(row().key(format!("source-summary-{index}")).w_full().gap(8.0)
                .child(text("•").font(type_::BODY).color(accent))
                .child(text(bullet.text.clone()).grow(1.0).min_w(0.0).font(type_::BODY).color(t.text).wrap()));
        }
    }
    let rows = Rc::new(thread.source_rows(&s.source.as_ref().unwrap().nav));
    let keys = Rc::clone(&rows);
    let kit = (*k).clone();
    content = content.child(el().w_full().h(1.0).my(4.0).fill(t.separator))
        .child(col().key("source-replies").w_full().gap(2.0).instant_changes()
        .growth(GrowthSpec { spring: Spring::NONE, ..GrowthSpec::default() }).virtual_rows(rows.len(), 110.0,
            move |_app: &App, i| format!("source-comment-{}", keys[i].id).into(),
            move |app: &App, i| app.story().map(|story| comment_row(story, rows[i].id, rows[i].depth, &kit, t, accent, true, reduced)).unwrap_or_else(el)));
    col().key("source-viewer").role("region").label(format!("Comment by {}", source.author))
        .focusable(true).w_full().h_full().scroll(ScrollSpec { stick_bottom: false }).scroll_edge_fade(20.0)
        .on_wheel(App::pointer_wheel)
        .on_scroll_offset(on_scroll).child(content)
}

/// One footer stays in the same place when the reader enters or leaves a source.
pub fn story_footer(s: &Story, k: &Kit, t: tok::Theme, hints: &rune::Live<f32>, reduced: bool, contrast: bool) -> El {
    let in_source = s.source.is_some();
    let back = if in_source { "Back to discussion" } else { "Back to feed" };
    let h = |keys: &[&str], label: &str| crate::app::passive_hint(hint(k, t, keys, label), hints, reduced, contrast);
    let mut footer = row().key("story-footer").instant_changes().w_full().px(24.0).py(2.0).gap(18.0).flex_wrap().center_y()
        .child(row().key("story-back").role("button").label(back).focusable(true)
            .h(36.0).px(8.0).gap(6.0).center_y().radius(4.0)
            .hover(k.c.hover).press(k.c.pressed).cursor(rune::Cursor::Pointer)
            .on_press_tx(|s: &mut App, tx: &mut Tx| { s.story_back(tx); })
            .child(Keybind::new(&["Esc"]).build(k))
            .child(text(back).font(type_::LABEL).color(t.text)))
        .child(h(&["j", "k"], "move"))
        .child(h(&["Tab"], if in_source { "next source" } else { "citation" }));
    if !in_source {
        footer = footer.child(h(&["⇧", "S"], "summary"))
            .child(h(&["r"], "refresh comments"));
    }
    footer.child(row().key("article-open-original").role("button").label("Open in browser")
        .focusable(true).on_press(|s: &mut App| s.pending_external = true)
        .gap(6.0).center_y().child(Keybind::new(&["⇧", "O"]).build(k))
        .child(text("browser ↗").font(type_::LABEL).color(t.text_secondary)))
}

fn hint(k: &Kit, t: tok::Theme, keys: &[&str], label: &str) -> El {
    row().gap(6.0).center_y().child(Keybind::new(keys).build(k))
        .child(text(label).font(type_::LABEL).color(t.text_secondary))
}

/// The titlebar remains a drag region and may carry the saved indicator.
pub fn story_toolbar(t: tok::Theme, accent: Color, saved: bool) -> El {
    let mut bar = row().key("story-toolbar").w_full().center_y();
    if saved {
        bar = bar.child(row().key("story-saved").role("status").label("Saved").gap(6.0).center_y()
            .child(el().key("story-saved-dot").size(6.0, 6.0).radius(3.0).fill(accent))
            .child(text("Saved").font(type_::LABEL).color(t.text_secondary).a11y_hidden()));
    }
    bar
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
fn host_of(url: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    #[test]
    fn browser_button_keeps_its_compact_accessible_target() {
        fn footer(story: &Story, cx: &mut rune::Cx<Story>) -> El {
            let dark = cx.system_dark();
            story_footer(story, &Kit::neutral(dark), tok::theme(dark), &rune::Live::new(1.0), false, false)
        }
        let story = Story::opening(fixtures::item(fixtures::STORY_INDEX, fixtures::STORY_ID), false);
        let mut rt = rune::Runtime::new(story, footer, (760.0, 100.0), rune::Env::new(true, 1.0));
        rt.frame(0.0);
        let elements = rt.elements();
        let button = elements.iter().find(|e| e.key.as_deref() == Some("article-open-original")).unwrap();
        assert_eq!(button.label.as_deref(), Some("Open in browser"));
        assert!(button.focusable);
        assert!(button.rect[2] < 130.0);
    }

    fn nested_comment(story: &Story, cx: &mut rune::Cx<Story>) -> El {
        let dark = cx.system_dark();
        comment_row(story, 9010, 1, &Kit::neutral(dark), tok::theme(dark), tok::accent(3, dark), false, false)
    }

    #[test]
    fn folded_comment_keeps_body_and_places_guide_outside_text() {
        let mut story = Story::opening(fixtures::item(fixtures::STORY_INDEX, fixtures::STORY_ID), true);
        story.thread = crate::comments::Thread::from_comments(&fixtures::thread());
        let mut rt = rune::Runtime::new(story, nested_comment, (420.0, 250.0), rune::Env::new(true, 1.0));
        rt.frame(0.0);
        let guide_x = rt.elements().iter().find(|e| e.key.as_deref() == Some("guide-9010-1")).unwrap().rect[0];
        rt.update(|story, tx| {
            assert!(story.thread.select(9010));
            assert!(story.thread.fold());
            tx.instant();
        });
        rt.frame(0.0);
        let elements = rt.elements();
        let find = |key: &str| elements.iter().find(|e| e.key.as_deref() == Some(key) && !e.exiting).unwrap();
        let author = find("comment-author-9010");
        let body = find("comment-text-9010");
        let guide = find("focus-rail-9010");
        let control = find("comment-hidden-9010");
        assert_eq!(guide.rect[0], guide_x, "selection shares the existing nesting guide");
        assert!(!elements.iter().any(|e| e.key.as_deref() == Some("guide-9010-1") && !e.exiting));
        assert!((author.rect[0] - body.rect[0]).abs() < 0.5);
        assert!(guide.rect[0] + guide.rect[2] < body.rect[0]);
        assert!(control.focusable && control.rect[2] < 200.0);
        assert!(control.label.as_deref().is_some_and(|label| label.starts_with("Show ")));
    }

    #[test]
    fn unreadable_parent_keeps_replies_without_author_or_body() {
        let mut comments = fixtures::thread();
        comments.iter_mut().find(|comment| comment.id == 9010).unwrap().deleted = true;
        let mut story = Story::opening(fixtures::item(fixtures::STORY_INDEX, fixtures::STORY_ID), true);
        story.thread = crate::comments::Thread::from_comments(&comments);
        let mut rt = rune::Runtime::new(story, nested_comment, (420.0, 250.0), rune::Env::new(true, 1.0));
        rt.frame(0.0);
        let elements = rt.elements();
        assert!(elements.iter().any(|e| e.key.as_deref() == Some("comment-group-9010") && e.focusable));
        for key in ["comment-author-9010", "comment-age-9010", "comment-text-9010"] {
            assert!(!elements.iter().any(|e| e.key.as_deref() == Some(key)));
        }
    }

    #[test]
    fn pending_reply_disclosure_matches_visible_loading_content() {
        for readable in [true, false] {
            let mut parent = fixtures::thread().into_iter().find(|comment| comment.id == 9010).unwrap();
            parent.deleted = !readable;
            let mut story = Story::opening(fixtures::item(fixtures::STORY_INDEX, fixtures::STORY_ID), true);
            story.thread = crate::comments::Thread::from_comments(&[parent]);
            assert!(story.request_replies(9010, false).is_some());
            let mut rt = rune::Runtime::new(story, nested_comment, (420.0, 250.0), rune::Env::new(true, 1.0));
            rt.frame(0.0);
            let elements = rt.elements();
            let row = elements.iter().find(|e| e.key.as_deref() == Some("comment-9010")).unwrap();
            assert_eq!(row.expanded, Some(true), "the pending disclosure has visible loading content");
            assert!(elements.iter().any(|e| e.text.as_deref() == Some("Loading replies…")));
            rt.update(|story, tx| { assert!(story.thread.fold()); tx.instant(); });
            rt.frame(0.0);
            let elements = rt.elements();
            let row = elements.iter().find(|e| e.key.as_deref() == Some("comment-9010")).unwrap();
            assert_eq!(row.expanded, Some(false));
            assert!(!elements.iter().any(|e| e.text.as_deref() == Some("Loading replies…") && !e.exiting));
        }
    }

    #[test]
    fn article_metadata_names_the_host_without_www() {
        assert_eq!(host_of("https://ghostty.org/blog/libghostty"), "ghostty.org");
        assert_eq!(host_of("https://www.example.com/a"), "example.com");
        assert_eq!(host_of(""), "the article");
    }
}
