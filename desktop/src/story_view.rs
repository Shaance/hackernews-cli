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
use std::rc::Rc;

use hn_lib::reader::Source as CapturedSource;
use rune::tok::{self, motion, radius, type_};
use rune::{Color, El, GrowthSpec, ScrollSpec, Spring, SpringPair, TextStyle, Transition, Tx, col, el, icon, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Keybind, Kit, Markdown, Placement, Popover, Spinner};

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
/// empty element that reports its rect through `on_layout`. The header stays Rune's, which is
/// what keeps "Open in browser ↗" clickable: the child draws above everything Rune paints, so
/// anything the reader must still be able to press has to sit outside its rect.
pub fn article_pane<'a>(s: &Story, k: &Kit, t: tok::Theme, pane: Pane<'a>) -> El {
    let mut outer = col().key("article-pane").w_full().h_full().child(
        col()
            .key("article-content")
            .w_full()
            .px(28.0)
            .pt(14.0)
            .child(article_header(s, k, t)),
    );
    if s.source.is_some() {
        // The source viewer takes the inspector; the article stays as context, dimmed.
        outer = outer.opacity(ARTICLE_DIMMED);
    }
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
                )
                // The extracted article is a reading surface; `O` also opens the real page.
                .child(
                    Button::new("Open in browser")
                        .control_size(ControlSize::Regular)
                        .label_with(|font, ink| text("Open in browser \u{2197}").font(font).color(ink))
                        .on_press(|s: &mut App| s.pending_external = true)
                        .build(k)
                        .key("article-open-original")
                        .self_start(),
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
) -> El {
    let transition = reading_transition(reduced);
    let reflow = if reduced { Spring::NONE } else { Spring::new(0.18, 0.0) };
    if let Some(source) = s.shown_source() {
        let viewer = source_viewer(s, source, k, t, accent).transition(transition);
        return if reduced { viewer.instant_changes() } else { viewer };
    }
    let inspector = col()
        .key("inspector")
        .transition(Transition::none())
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
                .child(summary_card(s, k, t, accent, reduced, reflow))
                .child(
                    discussion_pane(s, k, t, accent, overlays)
                        .transition(transition)
                        .motion(reflow)
                        .grow(1.0)
                        .min_h(0.0),
                ),
        );
    if reduced { inspector.instant_changes() } else { inspector }
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

/// The summary card. Its header and provenance are the card's identity; the body is whichever
/// of the four states the machine is in.
fn summary_card(s: &Story, k: &Kit, t: tok::Theme, accent: Color, reduced: bool, reflow: Spring) -> El {
    let content = if s.summary_open {
        expanded_summary(s, k, t, accent)
    } else {
        folded_summary(s, k, t)
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
        // Pinned: the discussion scrolls under it, and it never moves while tokens arrive.
        .sticky_top(12.0)
        .child(content.transition(reading_transition(reduced)).instant_changes())
}

fn expanded_summary(s: &Story, k: &Kit, t: tok::Theme, accent: Color) -> El {
    let mut card = col()
        .key("summary-open")
        .w_full()
        .gap(10.0)
        .px(14.0).py(14.0)
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
        .key("summary-folded")
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
        .scroll_margin(20.0)
        .child(discussion(s, k, t, accent, overlays).motion(motion::DEFAULT))
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
        let message = match &s.discussion {
            None => "Loading discussion…",
            Some(Ok(())) => "No readable comments yet.",
            Some(Err(message)) => message,
        };
        list = list.child(text(message).key("discussion-empty").font(type_::LABEL).color(t.text_secondary));
        if matches!(s.discussion, Some(Err(_))) {
            list = list.child(row().key("discussion-retry").role("button").label("Retry discussion")
                .focusable(true).on_press(|s: &mut App| s.retry_summary())
                .child(text("Retry discussion").font(type_::LABEL).color(accent)));
        }
        return list;
    }
    if let Some(Err(message)) = &s.discussion {
        list = list.child(text(message.clone()).role("alert").font(type_::LABEL).color(t.text_secondary))
            .child(row().role("button").label("Retry discussion").focusable(true)
                .on_press(|s: &mut App| s.retry_summary())
                .child(text("Retry discussion").font(type_::LABEL).color(accent)));
    }
    // The trail is the cursor's ancestor chain, so it only has something to say once the cursor
    // is below the top level: one segment is the comment itself, and the row already says that.
    let trail = thread.trail();
    if trail.len() > 1 {
        list = list.child(trail_strip(s, &trail, t));
    }
    let rows = Rc::new(thread.rows());
    let keys = Rc::clone(&rows);
    let kit = (*k).clone();
    list.child(col().key("discussion-rows").w_full().gap(2.0)
        .growth(GrowthSpec::default()).virtual_rows(rows.len(), 110.0,
            move |_s: &App, i| format!("comment-{}", keys[i].id).into(),
            move |app: &App, i| app.story().map(|s| comment_row(s, rows[i].id, rows[i].depth, &kit, t, accent)).unwrap_or_else(el)))
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
    let count = thread.len();
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
            text(format!("{count} loaded"))
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
        let author = s.thread.node(*id).map(|node| {
            if node.readable { node.author.clone() } else { "Replies".to_string() }
        }).unwrap_or_default();
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
/// The row is the fold target and the cursor target at once. Folding keeps the comment readable;
/// a comment at the depth cap offers `continue thread` instead of drawing deeper replies.
fn comment_row(s: &Story, id: i32, depth: u32, k: &Kit, t: tok::Theme, accent: Color) -> El {
    let thread = &s.thread;
    let Some(node) = thread.node(id) else {
        return el();
    };
    let selected = thread.cursor() == Some(id);
    let folded = thread.is_folded(id);
    let capped = thread.capped(id);
    let replies = thread.child_count(id);
    let loaded = !node.children.is_empty();
    let loading = s.pending_reply.as_ref().is_some_and(|r| r.parent_id == id);
    let expanded = (loaded || loading) && !folded && !capped;
    let mut r = col()
        .key(format!("comment-{id}"))
        .role(if node.readable { "article" } else { "group" })
        .label(if node.readable { format!("Comment by {}", node.author) } else { "Replies".to_string() })
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
        // Focus mode dims everything off the chain on REFLOW; the chain does not move.
        .motion(motion::REFLOW)
        .opacity(if thread.dimmed(id) { DIMMED } else { 1.0 })
        .on_press(move |s: &mut App| {
            if let Some(story) = s.story_mut() {
                story.thread.select(id);
            }
        });
    if replies > 0 { r = r.expanded(expanded); }
    // One hairline per ancestor level: the threadline that says how deep this row is.
    for level in 1..=depth {
        r = r.child(
            el()
                .key(format!("guide-{id}-{level}"))
                .pin(
                    Some(8.0 + (level as f32 - 0.5) * INDENT),
                    Some(0.0),
                    None,
                    Some(0.0),
                )
                .w(1.0)
                .fill(t.text_secondary.alpha(0.5)),
        );
    }
    let retry_primary = !node.readable && !loaded && !folded && !capped && !loading
        && thread.reply_batch(id, false).is_empty() && !thread.reply_batch(id, true).is_empty();
    let action = if capped { "Continue thread" } else if folded { "Show replies" }
        else if retry_primary { "Retry replies" } else if !loaded && !loading { "Load replies" }
        else { "Hide replies" };
    if !node.readable {
        let noun = if replies == 1 { "reply" } else { "replies" };
        r = r.child(
            row()
                .key(format!("comment-group-{id}"))
                .role("button")
                .label(format!("{action}: {replies} {noun}"))
                .h(20.0)
                .gap(6.0)
                .center_y()
                .self_start()
                .focusable(true)
                .disabled(s.pending_reply.is_some() && !loading && !loaded && !folded && !capped)
                .on_press(move |s: &mut App| s.activate_comment(id))
                .child(icon(k.glyph.chevron_down, 12.0)
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
                icon(k.glyph.chevron_down, 12.0)
                    .icon_rotation(if expanded { 0.0 } else { -FRAC_PI_2 })
                    .motion(motion::SNAP)
                    .color(t.text_secondary)
                    .role("button").label(action).focusable(true)
                    .on_press(move |s: &mut App| s.activate_comment(id)),
            );
        } else {
            // A leaf keeps the chevron's lane, so authors line up down the thread.
            who = who.child(el().size(12.0, 12.0));
        }
        who = who
            .child(
                text(node.author.clone())
                    .key(format!("comment-author-{id}"))
                    .shrink()
                    .min_w(0.0)
                    .font(type_::TITLE)
                    .color(t.text)
                    .truncate(),
            )
            .child(
                text(node.time_ago.clone())
                    .key(format!("comment-age-{id}"))
                    .font(type_::LABEL)
                    .color(t.text_secondary),
            );
        if folded && !capped {
            let hidden = thread.hidden_count(id).max(replies);
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
                text(node.text.clone())
                    .key(format!("comment-text-{id}"))
                    .ml(18.0)
                    .font(type_::BODY)
                    .color(t.text_secondary)
                    .wrap_anywhere(),
            );
    }
    if capped && node.readable {
        let behind = thread.reply_count(id).max(replies);
        r = r.child(
            row()
                .key(format!("comment-continue-{id}"))
                .role("button")
                .label(format!("Continue thread, {behind} replies"))
                .h(20.0)
                .ml(18.0)
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
        let indent = if node.readable { 18.0 } else { 0.0 };
        if loading {
            r = r.child(text("Loading replies…").ml(indent).role("status").busy(true).font(type_::LABEL).color(t.text_secondary));
        } else {
            for (retry, label) in [(false, "Load replies"), (true, "Retry replies")] {
                if !node.readable && !loaded && (!retry || retry_primary) { continue; }
                let count = thread.reply_batch(id, retry).len();
                if count == 0 { continue; }
                let label = if !retry && !thread.children_of(id).is_empty() { "Load more replies" } else { label };
                let label = format!("{label} ({count})");
                r = r.child(row().key(format!("comment-load-{id}-{retry}"))
                    .ml(indent).self_start().role("button").label(label.clone()).focusable(true).disabled(s.pending_reply.is_some())
                    .on_press(move |s: &mut App| s.load_replies(id, retry))
                    .child(text(label).font(type_::LABEL).color(accent)));
            }
            if let crate::comments::BranchStatus::Failed { message, .. } = &node.branch {
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
                .px(28.0)
                .pt(14.0)
                .pb(12.0)
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
/// The discussion's keys are here rather than in a second footer. The toolbar owns the feed
/// action; the source viewer says "back to the discussion" in its own header.
pub fn story_footer(k: &Kit, t: tok::Theme) -> El {
    row()
        .key("story-footer")
        .w_full()
        .px(24.0)
        .py(12.0)
        .gap(18.0)
        .flex_wrap()
        .center_y()
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

/// The story's toolbar: return to the feed and the story's saved state.
pub fn story_toolbar(k: &Kit, t: tok::Theme, accent: Color, saved: bool) -> El {
    let mut bar = row()
        .key("story-toolbar")
        .w_full()
        .gap(6.0)
        .center_y()
        .child(
            Button::new("Back to feed")
                .variant(ButtonVariant::Ghost)
                .on_press(|s: &mut App| s.close_story())
                .build(k)
                .key("story-back"),
        )
        .child(Keybind::new(&["Esc"]).build(k));
    if saved {
        bar = bar.child(
            row()
                .key("story-saved")
                .role("status")
                .label("Saved")
                .gap(6.0)
                .ml(6.0)
                .center_y()
                .child(el().key("story-saved-dot").size(6.0, 6.0).radius(3.0).fill(accent))
                .child(text("Saved").font(type_::LABEL).color(t.text_secondary).a11y_hidden()),
        );
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
    use crate::fixtures;

    #[test]
    fn browser_button_keeps_its_compact_accessible_target() {
        fn header(story: &Story, cx: &mut rune::Cx<Story>) -> El {
            let dark = cx.system_dark();
            article_header(story, &Kit::neutral(dark), tok::theme(dark))
        }
        let mut story = Story::opening(fixtures::item(fixtures::STORY_INDEX, fixtures::STORY_ID), false);
        story.story.title = "A long story title that wraps across several lines ".repeat(3);
        let mut rt = rune::Runtime::new(story, header, (360.0, 220.0), rune::Env::new(true, 1.0));
        rt.frame(0.0);
        let elements = rt.elements();
        let button = elements.iter().find(|e| e.key.as_deref() == Some("article-open-original")).unwrap();
        let title = elements.iter().find(|e| e.key.as_deref() == Some("article-title")).unwrap();
        assert_eq!(button.label.as_deref(), Some("Open in browser"));
        assert!(button.focusable);
        assert!((button.rect[3] - 24.0).abs() < 0.1);
        assert!(title.rect[3] > button.rect[3]);
        assert!(title.rect[2] > button.rect[2] + 100.0, "the title keeps the full header width");
    }

    fn nested_comment(story: &Story, cx: &mut rune::Cx<Story>) -> El {
        let dark = cx.system_dark();
        comment_row(story, 9010, 1, &Kit::neutral(dark), tok::theme(dark), tok::accent(3, dark))
    }

    #[test]
    fn folded_comment_keeps_body_and_places_guide_outside_text() {
        let mut story = Story::opening(fixtures::item(fixtures::STORY_INDEX, fixtures::STORY_ID), true);
        story.thread = crate::comments::Thread::from_comments(&fixtures::thread());
        assert!(story.thread.select(9010));
        let mut rt = rune::Runtime::new(story, nested_comment, (420.0, 250.0), rune::Env::new(true, 1.0));
        rt.frame(0.0);
        rt.update(|story, tx| { assert!(story.thread.fold()); tx.instant(); });
        rt.frame(0.0);
        let elements = rt.elements();
        let find = |key: &str| elements.iter().find(|e| e.key.as_deref() == Some(key) && !e.exiting).unwrap();
        let author = find("comment-author-9010");
        let body = find("comment-text-9010");
        let guide = find("guide-9010-1");
        let control = find("comment-hidden-9010");
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
    fn article_metadata_names_the_host_without_www() {
        assert_eq!(host_of("https://ghostty.org/blog/libghostty"), "ghostty.org");
        assert_eq!(host_of("https://www.example.com/a"), "example.com");
        assert_eq!(host_of(""), "the article");
    }
}
