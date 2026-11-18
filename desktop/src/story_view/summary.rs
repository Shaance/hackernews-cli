//! Summary card, citations, and generation states.

use rune::tok::{self, radius, type_};
use rune::{Color, El, Link, LinkPaint, ScrollSpec, Span, Spring, Transition, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Kit, Spinner};

use crate::app::App;
use crate::app::chrome::quiet_button;
use crate::story::{ArticleSummaryState, Story, Summary, SummaryState};

use super::reading_chrome::{reading_transition, skeleton_line};

/// The summary card holds two independently generated sections above the discussion.
pub(super) fn summary_card(s: &Story, k: &Kit, t: tok::Theme, accent: Color, reduced: bool, max_height: f32,
    on_scroll: impl Fn(rune::ScrollPos) + 'static) -> El {
    let reflow = if reduced { Spring::NONE } else { Spring::new(0.18, 0.0) };
    let content = if s.summary_open {
        expanded_summary(s, k, t, accent, reduced, max_height, on_scroll)
    } else {
        folded_summary(k, t)
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
        body = body.child(unconfigured(k, t));
    } else {
        let article_waiting = matches!(&s.article_summary, ArticleSummaryState::Generating { previous: None });
        let article_motion = if reduced || article_waiting { Spring::NONE } else { Spring::new(0.20, 0.0) };
        let mut article = col().key("article-summary-content").w_full().gap(7.0)
            .transition(Transition::none()).instant_changes();
        if let Some(value) = s.article_summary.readable() {
            article = article.child(text(value.to_string()).key("article-summary").font(type_::BODY).color(t.text)
                .selectable(true).selection_color(k.selection()).wrap());
        }
        match &s.article_summary {
            ArticleSummaryState::Generating { previous } if previous.is_none() => {},
            ArticleSummaryState::Generating { .. } => article = article.child(text("Updating article summary…").role("status").font(type_::LABEL).color(t.text_secondary)),
            ArticleSummaryState::Failed { message, .. } => article = article.child(text(format!("Article summary unavailable: {message}")).role("alert").font(type_::LABEL).color(t.text_secondary).wrap())
                .child(quiet_button("Retry summaries").on_press(|s: &mut App| s.retry_summary()).build(k).cursor(rune::Cursor::Pointer).self_start()),
            _ => {}
        }
        let discussion_waiting = matches!(&s.summary, SummaryState::Generating { previous: None });
        let discussion_motion = if reduced || discussion_waiting { Spring::NONE } else { Spring::new(0.20, 0.0) };
        let mut discussion = col().key("discussion-summary-content").w_full().gap(7.0)
            .transition(Transition::none()).instant_changes();
        if let Some(summary) = s.summary.readable() {
            discussion = discussion.child(bullets(s, summary, k, t, accent));
        }
        match &s.summary {
            SummaryState::Generating { previous } if previous.is_none() => {},
            SummaryState::Generating { .. } => discussion = discussion.child(text("Updating discussion summary…").role("status").font(type_::LABEL).color(t.text_secondary)),
            SummaryState::Failed { message, auth, .. } => discussion = discussion.child(failure(message, *auth, k, t)),
            _ => {}
        }
        body = body
            .children((!matches!(s.article_summary, ArticleSummaryState::Unconfigured)).then(|| col().w_full().gap(6.0)
            .child(text("Article").key("article-summary-heading").role("heading").level(3).font(type_::SECTION).color(t.text_secondary))
            .child(col().key("article-summary-body").w_full().child_exit(Transition::none())
                .child(col().key("article-summary-result").w_full().transition(Transition::none()).animated_changes()
                    .motion(Spring::NONE).opacity(if article_waiting && !reduced { 0.4 } else { 1.0 })
                    .offset(0.0, if article_waiting && !reduced { 8.0 } else { 0.0 })
                    .opacity_motion(article_motion).offset_motion(article_motion).child(article))
                .children(article_waiting.then(|| generating(k, t, "Reading the article…"))))))
            .children((!matches!(s.summary, SummaryState::Unconfigured)).then(|| col().w_full().gap(6.0)
            .child(text("Discussion").key("discussion-summary-heading").role("heading").level(3).font(type_::SECTION).color(t.text_secondary))
            .child(col().key("discussion-summary-body").w_full().child_exit(Transition::none())
                .child(col().key("discussion-summary-result").w_full().transition(Transition::none()).animated_changes()
                    .motion(Spring::NONE).opacity(if discussion_waiting && !reduced { 0.4 } else { 1.0 })
                    .offset(0.0, if discussion_waiting && !reduced { 8.0 } else { 0.0 })
                    .opacity_motion(discussion_motion).offset_motion(discussion_motion).child(discussion))
                .children(discussion_waiting.then(|| generating(k, t, "Reading the discussion…"))))));
    }
    let more = rune::Live::new(0.0_f32);
    let on_more = more.clone();
    let scroll = col().key("summary-scroll").role("region").label("Summary content").focusable(true);
    // Inline citations share the viewport's keyboard focus; the selected chip shows it.
    let scroll = if s.focused_citation().is_some() { scroll.no_focus_ring() } else { scroll };
    col().key("summary-open").w_full().gap(10.0).px(14.0).pt(14.0).pb(20.0)
        .child(summary_header(k, t))
        .child(scroll
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

fn summary_header(k: &Kit, t: tok::Theme) -> El {
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
        .child(quiet_button("Hide").control_size(ControlSize::Mini)
            .label_with(|_, ink| text("Hide").key("label").font(type_::LABEL).color(ink))
            .on_press(|s: &mut App| { if let Some(story) = s.story_mut() { story.toggle_summary(); } })
            .build(k).cursor(rune::Cursor::Pointer).label("Hide summary"))
}

/// Each source reference is a pressable inline range in the point it supports.
fn bullets(s: &Story, summary: &Summary, k: &Kit, t: tok::Theme, accent: Color) -> El {
    let mut list = col().key("summary-points").w_full().gap(10.0);
    let focused = s.focused_citation().filter(|_| s.source().is_none());
    for (index, bullet) in summary.bullets.iter().enumerate() {
        let mut content = bullet.text.clone();
        let mut spans = Vec::new();
        let mut links = Vec::new();
        let mut point_selected = false;
        for citation in &bullet.citations {
            let position = citation.position;
            content.push(' ');
            let start = content.len();
            content.push_str(&format!("[{}]", position + 1));
            let end = content.len();
            let selected = focused.is_some_and(|selected| selected.position == position);
            point_selected |= selected;
            let span = Span::new(start, end);
            let link = Link::new(start..end, format!("cite-{position}")).role("button");
            let label = format!("Read comment by {}", citation.source.author);
            if selected {
                let ink = tok::theme(false).text;
                spans.push(span.color(ink).bg(accent, 2.0));
                links.push(link.label(format!("Selected. {label}"))
                    .hover(LinkPaint::new().ring(ink, 1.0)));
            } else {
                spans.push(span.color(t.text_secondary).underline(accent));
                links.push(link.label(label).hover(LinkPaint::new().color(t.text).underline(accent)));
            }
        }
        let mut paragraph = text(content).key(format!("summary-text-{index}"))
            .grow(1.0).min_w(0.0).font(type_::BODY).color(t.text)
            .selectable(true).selection_color(k.selection()).wrap().spans(spans);
        for link in links { paragraph = paragraph.link(link); }
        paragraph = paragraph.on_link(|s: &mut App, id, _| {
            if let Some(index) = id.strip_prefix("cite-").and_then(|value| value.parse().ok()) {
                s.open_citation(index);
            }
        });
        let mut point = row().key(format!("summary-point-{index}")).w_full().gap(8.0);
        if point_selected {
            point = point.child(
                el().key("citation-focus").a11y_hidden().transition(Transition::none())
                    .pin(Some(0.0), Some(-3.0), Some(0.0), Some(-3.0))
                    .fill(accent.alpha(0.10))
                    .border_sides([0.0, 0.0, 0.0, 2.0], accent),
            );
        }
        let marker = text("•").font(type_::BODY)
            .color(if point_selected { accent.alpha(0.0) } else { accent });
        list = list.child(point.child(marker).child(paragraph));
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

fn failure(message: &str, auth: bool, k: &Kit, t: tok::Theme) -> El {
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
                    Button::new("Retry").variant(ButtonVariant::Submit)
                        .on_press(|s: &mut App| s.retry_summary())
                        .build(k).key("summary-retry").cursor(rune::Cursor::Pointer),
                )
                .child(
                    quiet_button(if auth { "Add API key" } else { "Change provider" })
                        .on_press(|s: &mut App| s.open_summary_settings())
                        .build(k).key("summary-settings").cursor(rune::Cursor::Pointer),
                ),
        )
}

fn unconfigured(k: &Kit, t: tok::Theme) -> El {
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
            quiet_button("Open settings")
                .on_press(|s: &mut App| s.open_summary_settings())
                .build(k).key("summary-settings").cursor(rune::Cursor::Pointer).self_start(),
        )
}

/// A compact, clickable control returns the space to the summary.
fn folded_summary(k: &Kit, t: tok::Theme) -> El {
    row()
        .key("summary-folded")
        .role("button").label("Show summary").focusable(true)
        .on_press(|s: &mut App| { if let Some(story) = s.story_mut() { story.toggle_summary(); } })
        .hover(k.c.hover).press(k.c.pressed).cursor(rune::Cursor::Pointer)
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

