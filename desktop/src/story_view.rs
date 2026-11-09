//! The story split's view: the article pane, the pinned summary card, the comment tree.
//!
//! Every function here is a pure function of [`Story`] plus the theme, so a scene, a scripted
//! key and a landed request all draw the same tree. The card's four states are
//! [`SummaryState`]'s; this module only draws them.
//!
//! The discussion is DESIGN.md's "Comments: reading a thread you did not start", drawn from
//! [`crate::comments::Thread`]: a tree with one indent guide per ancestor, a sticky ancestor
//! trail above the focused comment, a depth cap that turns deeper replies into
//! `continue thread → N replies`, focus mode's 0.35 dim on everything off the chain, and a
//! folded comment's derived `N replies hidden`. The state is the thread's; this module only
//! draws it and hands presses back to it.
//!
//! The article is `rune_kit::Markdown`, so headings, lists, quotes and fenced code are the
//! kit's, including its syntax colouring. Nothing here parses Markdown.

use std::f32::consts::FRAC_PI_2;

use hn_lib::reader::Source as CapturedSource;
use rune::tok::{self, motion, radius, type_};
use rune::{Color, El, ScrollSpec, TextStyle, Tx, col, el, icon, row, text};
use rune_kit::{Keybind, Kit, Markdown, Placement, Popover, Spinner};

use crate::app::App;
use crate::comments::{DIMMED, INDENT};
use crate::story::{ArticleState, Story, Summary, SummaryState};

/// The article's measure: 66 characters at [`type_::BODY`] (13 pt), 6.8 pt per character.
/// DESIGN.md's reading measure, and the reason the article column does not fill the pane.
pub const MEASURE: f32 = 448.8;

/// The article's opacity while a citation source is open (DESIGN.md screen 04).
const ARTICLE_DIMMED: f32 = 0.55;

/// The article's headline. `type_::TITLE` is a row title; a headline is a step above it.
const HEADLINE: TextStyle = TextStyle::new(21.0, 600.0);

/// The article pane: a sticky title over the Markdown body, at the reading measure.
pub fn article_pane(s: &Story, k: &Kit, t: tok::Theme) -> El {
    let mut pane = col().key("article-pane").w_full().h_full();
    if s.source.is_some() {
        // The source viewer takes the inspector; the article stays as context, dimmed.
        pane = pane.opacity(ARTICLE_DIMMED);
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
        // The extracted article is a reading surface; the real page is one press away.
        // `O` does the same from the keyboard.
        .child(
            row()
                .key("article-open-original")
                .px(10.0)
                .py(4.0)
                .radius(radius::SM)
                .fill(t.control_off)
                .on_press(|s: &mut App| s.pending_external = true)
                .child(
                    text("open original \u{2197}")
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
) -> El {
    if let Some(source) = s.shown_source() {
        return source_viewer(s, source, k, t, accent);
    }
    col()
        .key("inspector")
        .w_full()
        .h_full()
        .child(
            col()
                .key("inspector-content")
                .w_full()
                .h_full()
                .px(14.0)
                .py(12.0)
                .gap(10.0)
                .child(summary_card(s, k, t, accent))
                .child(
                    discussion_pane(s, k, t, accent, overlays)
                        .grow(1.0)
                        .min_h(0.0),
                ),
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
        .on_press(move |s: &mut App| {
            if let Some(story) = s.story_mut() {
                story.select_citation(number - 1);
            }
        })
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
                .on_press(|s: &mut App| s.open_summary_settings())
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

/// The discussion's viewport: the tree scrolls here, under the pinned card, and nowhere else.
fn discussion_pane(
    s: &Story,
    k: &Kit,
    t: tok::Theme,
    accent: Color,
    overlays: &mut Vec<El>,
) -> El {
    col()
        .key("discussion-scroll")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec { stick_bottom: false })
        .scroll_edge_fade(20.0)
        .child(discussion(s, k, t, accent, overlays))
}

/// The discussion: the header, the sticky ancestor trail, then the tree's visible rows.
fn discussion(s: &Story, k: &Kit, t: tok::Theme, accent: Color, overlays: &mut Vec<El>) -> El {
    let thread = &s.thread;
    let mut list = col()
        .key("discussion")
        .w_full()
        .gap(2.0)
        .child(discussion_header(s, k, t, accent, overlays));
    if thread.is_empty() {
        return list.child(
            text("No comments yet.")
                .key("discussion-empty")
                .font(type_::LABEL)
                .color(t.text_secondary),
        );
    }
    // The trail is the cursor's ancestor chain, so it only has something to say once the cursor
    // is below the top level: one segment is the comment itself, and the row already says that.
    let trail = thread.trail();
    if trail.len() > 1 {
        list = list.child(trail_strip(s, &trail, t));
    }
    for row in thread.rows() {
        list = list.child(comment_row(s, row.id, row.depth, k, t, accent));
    }
    list
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
    let mut header = row()
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
                "{} {} \u{00b7} {} top-level",
                thread.len(),
                if thread.len() == 1 { "comment" } else { "comments" },
                thread.roots()
            ))
            .key("discussion-count")
            .font(type_::LABEL)
            .color(t.text_secondary),
        );
    if thread.focus() {
        header = header.child(
            row()
                .key("discussion-focus")
                .role("status")
                .label("focus mode")
                .h(16.0)
                .px(6.0)
                .center()
                .radius(4.0)
                .fill(accent)
                .child(text("focus").font(type_::LABEL).color(t.bg)),
        );
    }
    header
        .child(el().grow(1.0))
        .child(link_chooser(s, k, t, accent, overlays))
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
        .px(2.0)
        .py(6.0)
        .center_y()
        // Opaque, so the rows scroll under the trail rather than through it.
        .fill(t.bg)
        // A segment added or removed is SNAP on the strip's width; the strip itself does not
        // move, it follows the scroll offset.
        .motion(motion::SNAP)
        .sticky_top(0.0);
    for (at, id) in trail.iter().enumerate() {
        if at > 0 {
            strip = strip.child(
                text("\u{203a}")
                    .font(type_::LABEL)
                    .color(t.text_secondary),
            );
        }
        let author = s
            .thread
            .node(*id)
            .map(|node| node.author.clone())
            .unwrap_or_default();
        if at == last {
            // The comment you are reading: named, not a target.
            strip = strip.child(
                text(author)
                    .key(format!("trail-{id}"))
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
                    .px(4.0)
                    .py(1.0)
                    .radius(3.0)
                    .focusable(true)
                    .on_press(move |s: &mut App| {
                        if let Some(story) = s.story_mut() {
                            story.thread.reroot(target);
                        }
                    })
                    .child(text(author).font(type_::LABEL).color(t.text_secondary)),
            );
        }
    }
    strip
}

/// One comment: its indent guides, its header, its text, and what it is hiding.
///
/// The row is the fold target and the cursor target at once. A folded comment keeps its header
/// and states the count it hides; a comment at the depth cap states the replies behind
/// `continue thread` instead of drawing them.
fn comment_row(s: &Story, id: i32, depth: u32, k: &Kit, t: tok::Theme, accent: Color) -> El {
    let thread = &s.thread;
    let Some(node) = thread.node(id) else {
        return el();
    };
    let selected = thread.cursor() == Some(id);
    let folded = thread.is_folded(id);
    let capped = thread.capped(id);
    let replies = thread.children_of(id).len();
    let mut r = col()
        .key(format!("comment-{id}"))
        .role("article")
        .label(format!("Comment by {}", node.author))
        .level(depth + 1)
        .expanded(!folded)
        .w_full()
        .gap(4.0)
        .pl(8.0 + depth as f32 * INDENT)
        .pr(10.0)
        .py(8.0)
        .radius(radius::SM)
        .focusable(true)
        .focus_selected(selected)
        .focus_ring(1.5, 0.0, accent)
        // Focus mode dims everything off the chain on REFLOW; the chain does not move.
        .motion(motion::REFLOW)
        .opacity(if thread.dimmed(id) { DIMMED } else { 1.0 })
        .on_press(move |s: &mut App| {
            if let Some(story) = s.story_mut() {
                story.thread.select(id);
            }
        });
    // One hairline per ancestor level: the threadline that says how deep this row is.
    for level in 1..=depth {
        r = r.child(
            el()
                .key(format!("guide-{id}-{level}"))
                .pin(
                    Some(8.0 + level as f32 * INDENT),
                    Some(0.0),
                    None,
                    Some(0.0),
                )
                .w(1.0)
                .fill(t.separator),
        );
    }
    let mut who = row()
        .key(format!("comment-who-{id}"))
        .w_full()
        .gap(6.0)
        .center_y();
    if replies > 0 || capped {
        who = who.child(
            icon(k.glyph.chevron_down, 12.0)
                .icon_rotation(if folded { -FRAC_PI_2 } else { 0.0 })
                .motion(motion::SNAP)
                .color(t.text_secondary),
        );
    } else {
        // A leaf keeps the chevron's lane, so authors line up down the thread.
        who = who.child(el().size(12.0, 12.0));
    }
    who = who
        .child(
            text(node.author.clone())
                .key(format!("comment-author-{id}"))
                .font(type_::TITLE)
                .color(t.text),
        )
        .child(
            text(node.time_ago.clone())
                .key(format!("comment-age-{id}"))
                .font(type_::LABEL)
                .color(t.text_secondary),
        )
        .child(el().grow(1.0));
    r = r.child(who);
    if !folded {
        r = r.child(
            text(node.text.clone())
                .key(format!("comment-text-{id}"))
                .font(type_::BODY)
                .color(t.text_secondary)
                .wrap(),
        );
    }
    if folded {
        let hidden = thread.hidden_count(id);
        r = r.child(
            row()
                .key(format!("comment-hidden-{id}"))
                .role("button")
                .label(format!("{hidden} replies hidden, Enter expands"))
                .h(18.0)
                .px(6.0)
                .center()
                .radius(4.0)
                .fill(t.control_off)
                .focusable(true)
                .on_press(move |s: &mut App| {
                    if let Some(story) = s.story_mut() {
                        story.thread.select(id);
                        story.thread.unfold();
                    }
                })
                .child(
                    text(format!("{hidden} replies hidden \u{00b7} Enter expands"))
                        .font(type_::LABEL)
                        .color(t.text_secondary),
                ),
        );
    }
    if capped {
        let behind = thread.reply_count(id);
        r = r.child(
            row()
                .key(format!("comment-continue-{id}"))
                .role("button")
                .label(format!("Continue thread, {behind} replies"))
                .h(20.0)
                .px(8.0)
                .center_y()
                .radius(4.0)
                .fill(t.control_off)
                .focusable(true)
                .on_press(move |s: &mut App| {
                    if let Some(story) = s.story_mut() {
                        story.thread.reroot(id);
                    }
                })
                .child(
                    text(format!("continue thread \u{2192} {behind} replies"))
                        .font(type_::LABEL)
                        .color(accent),
                ),
        );
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
        .on_press(|s: &mut App| {
            if let Some(story) = s.story_mut() {
                story.open_links();
            }
        })
        .child(Keybind::new(&["L"]).build(k))
        .child(text("links").font(type_::LABEL).color(t.text_secondary));
    pop.trigger(trigger).build(k, overlays)
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
///
/// The discussion's keys are here rather than in a second footer, so the seven hints are what the
/// row has room for at the narrowest split. The source viewer says "back to the discussion" in its
/// own header, so the footer does not repeat it.
pub fn story_footer(k: &Kit, t: tok::Theme) -> El {
    row()
        .key("story-footer")
        .w_full()
        .px(24.0)
        .py(12.0)
        .gap(18.0)
        .center_y()
        .child(hint(k, t, &["Esc"], "feed"))
        .child(hint(k, t, &["j", "k"], "move"))
        .child(hint(k, t, &["f"], "focus"))
        .child(hint(k, t, &["L"], "links"))
        .child(hint(k, t, &["Tab"], "citation"))
        .child(hint(k, t, &["S"], "summary"))
        .child(hint(k, t, &["r"], "refresh"))
        .child(el().grow(1.0))
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
    // The terminal app's wording, singular included.
    match comments {
        Some(1) => "1 comment".to_string(),
        Some(n) => format!("{n} comments"),
        None => "discuss".to_string(),
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

/// The age of the comment a citation points at, from the discussion tree.
fn age_of(s: &Story, id: i32) -> String {
    s.thread
        .node(id)
        .map(|node| node.time_ago.clone())
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
