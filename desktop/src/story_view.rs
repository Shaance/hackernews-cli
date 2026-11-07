//! The story split's view: the article pane, the pinned summary card, the flat discussion.
//!
//! Every function here is a pure function of [`Story`] plus the theme, so a scene, a scripted
//! key and a landed request all draw the same tree. The card's four states are
//! [`SummaryState`]'s; this module only draws them, and the discussion is a flat list of
//! top-level comments — threading, the ancestor trail and focus mode are WS-D's.
//!
//! The article is `rune_kit::Markdown`, so headings, lists, quotes and fenced code are the
//! kit's, including its syntax colouring. Nothing here parses Markdown.

use hn_lib::reader::Source as CapturedSource;
use rune::tok::{self, radius, type_};
use rune::{Color, El, ScrollSpec, TextStyle, col, el, row, text};
use rune_kit::{Keybind, Kit, Markdown, Spinner};

use crate::story::{ArticleState, CommentRow, Story, Summary, SummaryState};

/// The article's measure: 66 characters at [`type_::BODY`] (13 pt), 6.8 pt per character.
/// DESIGN.md's reading measure, and the reason the article column does not fill the pane.
pub const MEASURE: f32 = 448.8;

/// The article's opacity while a citation source is open (DESIGN.md screen 04).
const DIMMED: f32 = 0.55;

/// The article's headline. `type_::TITLE` is a row title; a headline is a step above it.
const HEADLINE: TextStyle = TextStyle::new(21.0, 600.0);

/// The article pane: a sticky title over the Markdown body, at the reading measure.
pub fn article_pane(s: &Story, k: &Kit, t: tok::Theme) -> El {
    let mut pane = col().key("article-pane").w_full().h_full();
    if s.source.is_some() {
        // The source viewer takes the inspector; the article stays as context, dimmed.
        pane = pane.opacity(DIMMED);
    }
    pane.child(
        col()
            .key("article-scroll")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .scroll(ScrollSpec { stick_bottom: false })
            .scroll_edge_fade(20.0)
            .child(
                col()
                    .key("article-content")
                    .w_full()
                    .px(32.0)
                    .py(20.0)
                    .gap(14.0)
                    // The title holds to the top of the pane while the body scrolls under it,
                    // so the story you are reading is always named.
                    .child(article_header(s, k, t).sticky_top(0.0))
                    .child(article_body(s, k, t)),
            ),
    )
}

fn article_header(s: &Story, k: &Kit, t: tok::Theme) -> El {
    let story = &s.story;
    row()
        .key("article-header")
        .w_full()
        .gap(12.0)
        .pb(10.0)
        // Opaque, so the body scrolls under the title rather than through it.
        .fill(k.c.bg_base)
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(6.0)
                .child(
                    text(format!("Article \u{00b7} {}", host_of(&story.url)))
                        .key("article-eyebrow")
                        .font(type_::SECTION)
                        .color(t.text_secondary),
                )
                .child(
                    text(story.title.clone())
                        .key("article-title")
                        .font(HEADLINE)
                        .color(t.text)
                        .wrap(),
                )
                .child(
                    text(format!(
                        "{}  \u{00b7}  {} points  \u{00b7}  {}",
                        story.author,
                        story.score,
                        comments_label(story.comments)
                    ))
                    .key("article-meta")
                    .font(type_::LABEL)
                    .color(t.text_secondary),
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
pub fn inspector_pane(s: &Story, k: &Kit, t: tok::Theme, accent: Color) -> El {
    if let Some(source) = s.shown_source() {
        return source_viewer(s, source, k, t, accent);
    }
    col()
        .key("inspector")
        .w_full()
        .h_full()
        .scroll(ScrollSpec { stick_bottom: false })
        .scroll_edge_fade(20.0)
        .child(
            col()
                .key("inspector-content")
                .w_full()
                .px(14.0)
                .py(12.0)
                .gap(10.0)
                .child(summary_card(s, k, t, accent))
                .child(discussion(s, t)),
        )
}

/// The summary card. Its header and provenance are the card's identity; the body is whichever
/// of the four states the machine is in.
fn summary_card(s: &Story, k: &Kit, t: tok::Theme, accent: Color) -> El {
    if !s.summary_open {
        return folded_summary(s, k, t);
    }
    let mut card = col()
        .key("summary-card")
        .role("region")
        .label("Summary")
        .w_full()
        .gap(10.0)
        .px(14.0).py(14.0)
        .radius(radius::MD)
        .fill(t.card)
        .border(1.0, t.card_border)
        // Pinned: the discussion scrolls under it, and it never moves while tokens arrive.
        .sticky_top(12.0)
        .child(summary_header(s, t, accent));
    card = match &s.summary {
        SummaryState::Unconfigured => card.child(unconfigured(t, accent)),
        SummaryState::Generating { .. } => card.child(generating(k, t)),
        SummaryState::Done(summary) => card
            .child(bullets(s, summary, t, accent))
            .child(hint_row(k, t)),
        SummaryState::Failed {
            message,
            auth,
            previous,
        } => {
            let mut failed = card.child(failure(message, *auth, t, accent));
            if let Some(previous) = previous {
                // The old summary stays readable underneath, as DESIGN.md asks.
                failed = failed
                    .child(el().w_full().h(1.0).fill(t.separator))
                    .child(bullets(s, previous, t, accent));
            }
            failed
        }
    };
    card
}

fn summary_header(s: &Story, t: tok::Theme, accent: Color) -> El {
    let provenance = match &s.summary {
        SummaryState::Unconfigured => "no API key".to_string(),
        SummaryState::Generating { captured, of } => format!("{captured} of {of} sources captured"),
        SummaryState::Done(summary) => format!("{} of {} sources", summary.captured, summary.of),
        SummaryState::Failed { previous, .. } => match previous {
            Some(summary) => format!("{} of {} sources", summary.captured, summary.of),
            None => "no sources captured".to_string(),
        },
    };
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
        .child(
            text(provenance)
                .key("summary-provenance")
                .font(type_::LABEL)
                .color(if s.summary.is_failed() {
                    accent
                } else {
                    t.text_secondary
                }),
        )
}

/// The numbered bullets, each with the citation chips it rests on.
fn bullets(s: &Story, summary: &Summary, t: tok::Theme, accent: Color) -> El {
    let citations = s.citations();
    let mut list = col().key("summary-points").w_full().gap(10.0);
    for (index, bullet) in summary.bullets.iter().enumerate() {
        let mut chips = row().key(format!("cites-{index}")).gap(4.0);
        for id in &bullet.sources {
            // A chip only exists for a source the capture holds, so it can always be read.
            let Some(position) = citations.iter().position(|cited| cited == id) else {
                continue;
            };
            let selected = s.citation_focus && s.citation == position;
            chips = chips.child(citation_chip(*id, position + 1, selected, t, accent));
        }
        list = list.child(
            row()
                .key(format!("summary-point-{index}"))
                .w_full()
                .gap(8.0)
                .child(
                    row()
                        .w(16.0)
                        .h(16.0)
                        .center()
                        .radius(8.0)
                        .fill(t.control_off)
                        .child(
                            text(format!("{}", index + 1))
                                .font(type_::LABEL)
                                .color(t.text_secondary),
                        ),
                )
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(5.0)
                        .child(
                            text(bullet.text.clone())
                                .key(format!("summary-text-{index}"))
                                .font(type_::BODY)
                                .color(t.text)
                                .wrap(),
                        )
                        .child(chips),
                ),
        );
    }
    list
}

fn citation_chip(id: i32, number: usize, selected: bool, t: tok::Theme, accent: Color) -> El {
    row()
        .key(format!("cite-{id}"))
        .role("button")
        .label(format!("Citation {number}"))
        .size(18.0, 18.0)
        .center()
        .radius(4.0)
        .fill(if selected { accent } else { t.control_off })
        .focusable(true)
        .focus_selected(selected)
        .child(
            text(format!("{number}"))
                .font(type_::LABEL)
                .color(if selected { t.bg } else { t.text_secondary }),
        )
}

fn hint_row(k: &Kit, t: tok::Theme) -> El {
    col()
        .key("summary-hint")
        .w_full()
        .gap(6.0)
        .child(el().w_full().h(1.0).fill(t.separator))
        .child(
            row()
                .gap(6.0)
                .center_y()
                .child(Keybind::new(&["Tab"]).build(k))
                .child(
                    text("citation")
                        .font(type_::LABEL)
                        .color(t.text_secondary),
                )
                .child(Keybind::new(&["Enter"]).build(k))
                .child(
                    text("source")
                        .font(type_::LABEL)
                        .color(t.text_secondary),
                )
                .child(Keybind::new(&["S"]).build(k))
                .child(
                    text("hide")
                        .font(type_::LABEL)
                        .color(t.text_secondary),
                ),
        )
}

/// Generating: a spinner, skeleton rows, and the count of capture stages that have landed.
fn generating(k: &Kit, t: tok::Theme) -> El {
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
                    text("Reading the discussion\u{2026}")
                        .key("summary-status")
                        .font(type_::LABEL)
                        .color(t.text_secondary),
                ),
        )
        .children((0..5).map(|n| skeleton_line(n, t)))
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
            text("No summary API key")
                .font(type_::TITLE)
                .color(t.text),
        )
        .child(
            text("Add one in settings and the summary appears here. The article and the discussion do not need it.")
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
                .child(text("Open settings").font(type_::LABEL).color(t.bg)),
        )
}

/// `S` folded the card: one line, so the discussion gets the height.
fn folded_summary(s: &Story, k: &Kit, t: tok::Theme) -> El {
    let count = match &s.summary {
        SummaryState::Done(summary) => format!("{} points", summary.bullets.len()),
        SummaryState::Generating { .. } => "generating".to_string(),
        SummaryState::Failed { .. } => "failed".to_string(),
        SummaryState::Unconfigured => "no key".to_string(),
    };
    row()
        .key("summary-card")
        .role("region")
        .label("Summary")
        .w_full()
        .gap(8.0)
        .px(14.0)
        .py(10.0)
        .center_y()
        .radius(radius::MD)
        .fill(t.card)
        .border(1.0, t.card_border)
        .sticky_top(12.0)
        .child(
            text("Summary")
                .key("summary-title")
                .font(type_::TITLE)
                .color(t.text),
        )
        .child(
            text(count)
                .key("summary-provenance")
                .font(type_::LABEL)
                .color(t.text_secondary),
        )
        .child(el().grow(1.0))
        .child(Keybind::new(&["S"]).build(k))
        .child(
            text("show")
                .font(type_::LABEL)
                .color(t.text_secondary),
        )
}

/// The discussion: a flat list of top-level comments. Threading is WS-D's.
fn discussion(s: &Story, t: tok::Theme) -> El {
    let mut list = col()
        .key("discussion")
        .w_full()
        .gap(2.0)
        .child(
            row()
                .key("discussion-header")
                .w_full()
                .px(2.0)
                .py(6.0)
                .gap(8.0)
                .center_y()
                .child(
                    text("Discussion")
                        .font(type_::SECTION)
                        .color(t.text_secondary),
                )
                .child(
                    text(format!(
                        "{} top-level {}",
                        s.discussion.len(),
                        if s.discussion.len() == 1 {
                            "comment"
                        } else {
                            "comments"
                        }
                    ))
                    .key("discussion-count")
                    .font(type_::LABEL)
                    .color(t.text_secondary),
                ),
        );
    if s.discussion.is_empty() {
        list = list.child(
            text("No comments yet.")
                .key("discussion-empty")
                .font(type_::LABEL)
                .color(t.text_secondary),
        );
    }
    for comment in &s.discussion {
        list = list.child(comment_row(comment, t));
    }
    list
}

fn comment_row(c: &CommentRow, t: tok::Theme) -> El {
    let mut who = row()
        .key(format!("comment-who-{}", c.id))
        .w_full()
        .gap(8.0)
        .center_y()
        .child(
            text(c.author.clone())
                .key(format!("comment-author-{}", c.id))
                .font(type_::TITLE)
                .color(t.text),
        )
        .child(
            text(c.time_ago.clone())
                .key(format!("comment-age-{}", c.id))
                .font(type_::LABEL)
                .color(t.text_secondary),
        )
        .child(el().grow(1.0));
    if c.replies > 0 {
        // A count, not a fold target: opening a thread is WS-D's.
        who = who.child(
            text(if c.replies == 1 {
                "1 reply".to_string()
            } else {
                format!("{} replies", c.replies)
            })
                .key(format!("comment-replies-{}", c.id))
                .font(type_::LABEL)
                .color(t.text_secondary),
        );
    }
    col()
        .key(format!("comment-{}", c.id))
        .role("article")
        .label(format!("Comment by {}", c.author))
        .w_full()
        .gap(4.0)
        .px(10.0)
        .py(8.0)
        .radius(radius::SM)
        .child(who)
        .child(
            text(c.excerpt.clone())
                .key(format!("comment-text-{}", c.id))
                .font(type_::BODY)
                .color(t.text_secondary)
                .wrap(),
        )
}

/// The citation source viewer: the excerpt the model saw, then whatever the capture holds
/// beyond it, so a citation can be checked against the comment it came from.
fn source_viewer(
    s: &Story,
    source: &CapturedSource,
    k: &Kit,
    t: tok::Theme,
    accent: Color,
) -> El {
    let citations = s.citations();
    let position = citations
        .iter()
        .position(|id| *id == source.id)
        .map(|index| index + 1)
        .unwrap_or(0);
    let mut viewer = col()
        .key("source-viewer")
        .role("region")
        .label(format!("Citation {position}"))
        .w_full()
        .h_full()
        .scroll(ScrollSpec { stick_bottom: false })
        .scroll_edge_fade(20.0)
        .child(
            col()
                .key("source-content")
                .w_full()
                .px(14.0)
                .py(12.0)
                .gap(10.0)
                .child(
                    row()
                        .key("source-header")
                        .w_full()
                        .gap(8.0)
                        .center_y()
                        .child(
                            text(format!("Source {position} of {}", citations.len()))
                                .key("source-title")
                                .font(type_::TITLE)
                                .color(t.text),
                        )
                        .child(el().grow(1.0))
                        .child(
                            row()
                                .key("source-badge")
                                .h(18.0)
                                .px(7.0)
                                .center()
                                .radius(4.0)
                                .fill(accent)
                                .child(
                                    text(format!("citation {position}"))
                                        .font(type_::LABEL)
                                        .color(t.bg),
                                ),
                        ),
                )
                .child(
                    text(format!("{}, {}", source.author, age_of(s, source.id)))
                        .key("source-who")
                        .font(type_::LABEL)
                        .color(t.text_secondary),
                )
                .child(
                    col()
                        .key("source-excerpt")
                        .w_full()
                        .gap(6.0)
                        .px(10.0).py(10.0)
                        .radius(radius::SM)
                        .fill(t.control_off)
                        .child(
                            text("Sent to the model")
                                .font(type_::SECTION)
                                .color(t.text_secondary),
                        )
                        .child(
                            text(source.excerpt.clone())
                                .key("source-excerpt-text")
                                .font(type_::BODY)
                                .color(t.text)
                                .wrap(),
                        ),
                ),
        );
    if let Some(extra) = extra_text(source) {
        viewer = viewer.child(
            col()
                .key("source-extra")
                .w_full()
                .gap(6.0)
                .child(
                    text("Also captured")
                        .font(type_::SECTION)
                        .color(t.text_secondary),
                )
                .child(
                    text(extra.to_string())
                        .key("source-extra-text")
                        .font(type_::BODY)
                        .color(t.text_secondary)
                        .wrap(),
                ),
        );
    }
    viewer.child(
        row()
            .key("source-hint")
            .w_full()
            .gap(6.0)
            .center_y()
            .child(Keybind::new(&["Esc"]).build(k))
            .child(
                text("back to the discussion")
                    .font(type_::LABEL)
                    .color(t.text_secondary),
            ),
    )
}

/// What the capture holds beyond the excerpt the model saw, when it holds more.
///
/// The reader bounds an excerpt at 1800 characters (`src/reader/sources.rs:143`), so a long
/// comment has text the summary never saw. The viewer shows it rather than implying the
/// excerpt is the whole comment.
pub fn extra_text(source: &CapturedSource) -> Option<&str> {
    let excerpt = source.excerpt.trim_end();
    let text = source.text.trim();
    if text.len() > excerpt.len() && text.starts_with(excerpt) {
        let rest = text[excerpt.len()..].trim_start();
        return (!rest.is_empty()).then_some(rest);
    }
    None
}

/// The story's footer: the keys this view binds, and no page stepper — a story is not a page.
pub fn story_footer(s: &Story, k: &Kit, t: tok::Theme) -> El {
    let mut f = row()
        .key("story-footer")
        .w_full()
        .px(24.0)
        .py(12.0)
        .gap(18.0)
        .center_y()
        .child(hint(k, t, &["Esc"], "feed"))
        .child(hint(k, t, &["Tab"], "citation"))
        .child(hint(k, t, &["S"], "summary"))
        .child(hint(k, t, &["r"], "refresh"))
        .child(el().grow(1.0));
    if s.source.is_some() {
        f = f.child(
            text("Reading a citation")
                .key("story-footer-note")
                .font(type_::LABEL)
                .color(t.text_secondary),
        );
    }
    f
}

fn hint(k: &Kit, t: tok::Theme, keys: &[&str], label: &str) -> El {
    row()
        .gap(6.0)
        .center_y()
        .child(Keybind::new(keys).build(k))
        .child(text(label).font(type_::LABEL).color(t.text_secondary))
}

/// The story's toolbar: back, the title, its meta, and the summary pill.
pub fn story_toolbar(
    s: &Story,
    k: &Kit,
    t: tok::Theme,
    accent: Color,
    profile: &str,
    saved: bool,
) -> El {
    let story = &s.story;
    let profile = profile.to_lowercase();
    let mut bar = row()
        .key("story-toolbar")
        .w_full()
        .gap(10.0)
        .center_y()
        .child(Keybind::new(&["Esc"]).build(k))
        .child(
            text(story.title.clone())
                .key("story-toolbar-title")
                .font(type_::TITLE)
                .color(t.text)
                .truncate(),
        )
        .child(
            text(format!(
                "{}  \u{00b7}  {} points  \u{00b7}  {}",
                story.author,
                story.score,
                comments_label(story.comments)
            ))
            .key("story-toolbar-meta")
            .font(type_::LABEL)
            .color(t.text_secondary),
        );
    if saved {
        // The same 6 pt dot the feed row carries: the only persistent mark of a saved story.
        bar = bar.child(
            el()
                .key("story-saved-dot")
                .size(6.0, 6.0)
                .radius(3.0)
                .fill(accent)
                .role("status")
                .label("saved"),
        );
    }
    bar = bar.child(el().grow(1.0));
    // The pill is the one place the profile is named: the card header carries provenance, not
    // the profile, so the same fact is never on screen twice (DESIGN.md).
    bar = bar.child(
        row()
            .key("summary-pill")
            .role("status")
            .label(format!("summary \u{00b7} {profile}"))
            .h(20.0)
            .px(8.0)
            .gap(6.0)
            .center_y()
            .radius(10.0)
            .fill(t.control_off)
            .child(el().size(6.0, 6.0).radius(3.0).fill(accent))
            .child(
                text(format!("summary \u{00b7} {profile}"))
                    .font(type_::LABEL)
                    .color(t.text_secondary),
            ),
    );
    bar
}

fn comments_label(comments: Option<i32>) -> String {
    match comments {
        Some(0) => "discuss".to_string(),
        Some(n) => format!("{n} comments"),
        None => "no comments".to_string(),
    }
}

/// The host a story links to, for the article's eyebrow. The URL is the story's, not the
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

/// The age of the comment a citation points at, from the discussion rows.
fn age_of(s: &Story, id: i32) -> String {
    s.discussion
        .iter()
        .find(|comment| comment.id == id)
        .map(|comment| comment.time_ago.clone())
        .unwrap_or_else(|| "captured".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(excerpt: &str, text: &str) -> CapturedSource {
        CapturedSource {
            id: 901,
            parent: None,
            author: "tptacek".to_string(),
            excerpt: excerpt.to_string(),
            text: text.to_string(),
        }
    }

    #[test]
    fn a_capture_that_holds_more_than_the_excerpt_shows_the_rest() {
        let long = source(
            "The interesting part is what they left out.",
            "The interesting part is what they left out. No windowing, no font stack.",
        );
        assert_eq!(
            extra_text(&long),
            Some("No windowing, no font stack."),
            "the viewer shows what the model did not see"
        );
    }

    #[test]
    fn an_excerpt_that_is_the_whole_comment_has_nothing_extra() {
        let whole = source("A narrow ABI.", "A narrow ABI.");
        assert_eq!(extra_text(&whole), None);
        // Trailing whitespace is not extra text.
        let padded = source("A narrow ABI.", "A narrow ABI.\n\n");
        assert_eq!(extra_text(&padded), None);
    }

    #[test]
    fn the_eyebrow_names_the_host_without_www() {
        assert_eq!(host_of("https://ghostty.org/blog/libghostty"), "ghostty.org");
        assert_eq!(host_of("https://www.example.com/a"), "example.com");
        assert_eq!(host_of(""), "the article");
    }
}
