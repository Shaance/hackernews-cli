//! Discussion, comment rows, links, and cited source views.

use std::f32::consts::FRAC_PI_2;
use std::rc::Rc;

use hn_lib::comments::BranchStatus;
use crate::story::VISIBLE_COMMENT_LEVELS;
use rune::tok::{self, radius, type_};
use rune::{Color, El, GrowthSpec, ScrollSpec, Spring, SpringPair, Transition, Tx, col, el, icon, row, text};
use rune_kit::{Keybind, Kit, Placement, Popover};

use crate::app::App;
use crate::app::chrome::quiet_button;
use crate::story::{SourceView, Story};

/// Points each indent level steps in (DESIGN.md: "every indent level costs 13pt").
const INDENT: f32 = 13.0;

/// How much of a comment the tree draws.
///
/// HN caps a comment at 10,000 characters, which is 2,000 points of text in a 500-point pane.
/// The tree keeps the first 2,000 and says it stopped; the citation viewer still shows the whole
/// captured comment, so nothing is lost, only shortened where it is drawn.
const MAX_TEXT: usize = 2000;

#[derive(Clone, Copy)]
pub(super) enum RenderContext<'a> {
    Discussion,
    Source(&'a SourceView),
}

/// The discussion's viewport: the tree scrolls here, under the pinned card, and nowhere else.
pub(super) fn discussion_pane(
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
            list = list.child(quiet_button("Retry discussion").on_press(|s: &mut App| s.refresh_comments())
                .build(k).key("discussion-retry").cursor(rune::Cursor::Pointer).self_start());
        }
        return list;
    }
    if let Some(Err(message)) = &s.discussion {
        list = list.child(text(message.clone()).role("alert").font(type_::LABEL).color(t.text_secondary))
            .child(quiet_button("Retry discussion")
                .on_press(|s: &mut App| s.refresh_comments())
                .build(k).cursor(rune::Cursor::Pointer).self_start());
    }
    let rows = Rc::new(thread.rows());
    let keys = Rc::clone(&rows);
    let kit = (*k).clone();
    let mut stack = col().key("discussion-rows").w_full().gap(2.0).motion(Spring::NONE);
    if reduced { stack = stack.instant_changes(); }
    list.child(stack
        .growth(GrowthSpec { spring: Spring::NONE, ..GrowthSpec::default() }).virtual_rows(rows.len(), 110.0,
            move |_s: &App, i| format!("comment-{}", keys[i].id).into(),
            move |app: &App, i| app.story().map(|s| comment_row(s, rows[i].id, rows[i].depth, &kit, t, accent, RenderContext::Discussion, reduced, app.keyboard_selection)).unwrap_or_else(el)))
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
        trail_strip(s, &trail, k, t)
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
    header = header.child(quiet_button(if thread.focus() { "Focus on" } else { "Focus off" })
        .on_press(|s: &mut App| { if let Some(story) = s.story_mut() { story.thread.toggle_focus(); } })
        .build(k).key("discussion-focus").cursor(rune::Cursor::Pointer)
        .label(if thread.focus() { "Turn focus mode off" } else { "Turn focus mode on" }));
    let has_links = !s.story.links.is_empty() || thread.cursor().and_then(|id| thread.node(id)).is_some_and(|node| !node.links.is_empty());
    if has_links { header = header.child(link_chooser(s, k, t, accent, overlays)); }
    header
}

/// The ancestor trail: `tptacek › kstrauser › jmillikin`, held to the top of the discussion's
/// viewport so "where am I" never scrolls away. Every segment but the last re-roots there.
fn trail_strip(s: &Story, trail: &[i32], k: &Kit, t: tok::Theme) -> El {
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
                    .hover(k.c.hover).press(k.c.pressed).cursor(rune::Cursor::Pointer)
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

/// A comment's text as the tree draws it: trimmed, and bounded at [`MAX_TEXT`] with an ellipsis
/// so a row cannot run to thousands of points.
fn drawn_text(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= MAX_TEXT {
        return text.to_string();
    }
    let kept: String = text.chars().take(MAX_TEXT).collect();
    format!("{kept}\u{2026}")
}

/// One comment: its indent guides, its header, its text, and what it is hiding.
///
/// The row is the fold target and the cursor target at once. Folding keeps the comment readable;
/// a comment at the depth cap offers `continue thread` instead of drawing deeper replies.
pub(super) fn comment_row(s: &Story, id: i32, depth: u32, k: &Kit, t: tok::Theme, accent: Color, context: RenderContext<'_>, reduced: bool, keyboard: bool) -> El {
    let thread = &s.thread;
    let captured = match context { RenderContext::Source(source) => Some(source.captured()).filter(|source| source.id == id), RenderContext::Discussion => None };
    let node = thread.node(id);
    let (author, body, readable) = match (captured, node) {
        (Some(source), _) => (source.author.as_str(), source.text.as_str(), true),
        (None, Some(node)) => (node.author.as_str(), node.text.as_str(), node.readable),
        (None, None) => return el(),
    };
    let nav = match context { RenderContext::Source(source) => Some(&source.nav), RenderContext::Discussion => None };
    let context = matches!(context, RenderContext::Source(_));
    let cursor = nav.map_or_else(|| thread.cursor(), |nav| nav.cursor);
    let focus = nav.map_or_else(|| thread.focus(), |nav| nav.focus);
    let selected = cursor == Some(id);
    let folded = nav.map_or_else(|| thread.is_folded(id), |nav| nav.folded.contains(&id));
    let capped = if context { depth + 1 >= VISIBLE_COMMENT_LEVELS && thread.has_children(id) } else { thread.capped(id) };
    let replies = thread.child_count(id);
    let loaded = node.is_some_and(|node| !node.children.is_empty());
    let loading = s.pending_reply.as_ref().is_some_and(|r| r.batch.parent_id() == id);
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
        .motion(if reduced || keyboard { Spring::NONE } else { Spring::new(0.12, 0.0) })
        // Outgoing virtual rows would otherwise paint over the live rows closing their space.
        .transition(if depth == 0 || reduced { Transition::none() } else {
            Transition::asymmetric(Transition::opacity(), Transition::none())
                .springs(SpringPair::new(Spring::new(0.15, 0.0), Spring::NONE))
        });
    if keyboard || reduced { r = r.child_exit(Transition::none()); }
    if depth > 0 && !reduced { r = r.fold_glide(Spring::new(0.12, 0.0)); }
    r = r.on_press_tx(move |s: &mut App, tx: &mut Tx| {
        if let Some(story) = s.story_mut() {
            if !context { story.clear_citation_focus(); }
            let changed = story.with_thread(|thread| thread.cursor() != Some(id) && thread.select(id));
            if changed { s.keyboard_selection = tx.by_keys(); }
        }
    }).on_pointer_move_local(move |s: &mut App, ev, tx| {
        if !s.pointer_can_select(ev) {
            tx.live_only();
            return;
        }
        if let Some(story) = s.story_mut() {
            let changed = story.with_thread(|thread| thread.focus() && thread.cursor() != Some(id) && thread.select(id));
            if changed && !context { story.clear_citation_focus(); }
            if changed { s.keyboard_selection = false; }
            if !changed { tx.live_only(); }
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
        && thread.reply_batch(id, false).is_none() && thread.reply_batch(id, true).is_some();
    let action = if capped { "Continue thread" } else if folded { "Show replies" }
        else if retry_primary { "Retry replies" } else if !loaded && !loading { "Load replies" }
        else { "Hide replies" };
    let activate = move |s: &mut App, tx: &mut Tx| {
        if !capped { tx.keep_on_screen(format!("{}comment-{id}", if context { "source-" } else { "" })); }
        s.activate_comment(id);
    };
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
                .radius(4.0).hover(k.c.hover).press(k.c.pressed).cursor(rune::Cursor::Pointer)
                .on_press_tx(activate)
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
                        .radius(4.0).hover(k.c.hover).press(k.c.pressed).cursor(rune::Cursor::Pointer)
                        .on_press_tx(activate)),
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
                quiet_button(format!("{hidden} hidden {noun}"))
                    .on_press_tx(activate)
                    .build(k).key(format!("comment-hidden-{id}")).cursor(rune::Cursor::Pointer)
                    .label(format!("Show {hidden} hidden {noun}"))
                    .transition(Transition::none()),
            );
        }
        r = r.child(who.child(el().grow(1.0)))
            .child(
                text(if captured.is_some() { body.to_string() } else { drawn_text(body) })
                    .key(if captured.is_some() { "source-comment-text".to_string() } else { format!("comment-text-{id}") })
                    .ml(26.0)
                    .font(type_::BODY)
                    .color(if !focus || selected { t.text } else { t.text_secondary })
                    .selectable(true)
                    .selection_color(k.selection())
                    .wrap_anywhere(),
            );
    }
    if capped && readable {
        let behind = thread.reply_count(id).max(replies);
        r = r.child(
            quiet_button(format!("continue thread \u{2192} {behind} {}", if behind == 1 { "reply" } else { "replies" }))
                .on_press(move |s: &mut App| {
                    s.activate_comment(id);
                })
                .build(k).key(format!("comment-continue-{id}")).cursor(rune::Cursor::Pointer)
                .label(format!("Continue thread, {behind} replies"))
                .ml(26.0).self_start(),
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
                let Some(batch) = thread.reply_batch(id, retry) else { continue; };
                let count = batch.ids().len();
                let label = if !retry && !thread.children_of(id).is_empty() { "Load more replies" } else { label };
                let label = format!("{label} ({count})");
                r = r.child(quiet_button(label).disabled(s.pending_reply.is_some())
                    .on_press(move |s: &mut App| s.load_replies(id, retry))
                    .build(k).key(format!("comment-load-{id}-{retry}")).cursor(rune::Cursor::Pointer).ml(indent).self_start());
            }
            if let Some(BranchStatus::Failed { message, .. }) = node.map(|node| &node.branch) {
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
        for (index, (item, selected)) in chooser.items().enumerate() {
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
                        if let Some(chooser) = &mut story.links && chooser.select(index) {
                            let _ = tx.open_url(chooser.selected_url());
                            story.close_links();
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
pub(super) fn source_viewer(
    s: &Story,
    source: &SourceView,
    k: &Kit,
    t: tok::Theme,
    accent: Color,
    reduced: bool,
    on_scroll: impl Fn(rune::ScrollPos) + 'static,
) -> El {
    let thread = &s.thread;
    let mut content = col().key("source-content").w_full().px(28.0).py(14.0).gap(10.0).motion(Spring::NONE)
        .child(text("Discussion summary").key("source-summary-heading").font(type_::LABEL).color(t.text_secondary));
    for (index, bullet) in source.summary().bullets.iter().enumerate()
        .filter(|(_, bullet)| bullet.citations.iter().any(|citation| citation.source.id == source.id())) {
        content = content.child(row().key(format!("source-summary-{index}")).w_full().gap(8.0)
            .child(text("•").font(type_::BODY).color(accent))
            .child(text(bullet.text.clone()).grow(1.0).min_w(0.0).font(type_::BODY).color(t.text).wrap()));
    }
    let rows = Rc::new(thread.source_rows(&source.nav));
    let keys = Rc::clone(&rows);
    let kit = (*k).clone();
    let mut stack = col().key("source-replies").w_full().gap(2.0).motion(Spring::NONE);
    if reduced { stack = stack.instant_changes(); }
    content = content.child(el().w_full().h(1.0).my(4.0).fill(t.separator))
        .child(stack
        .growth(GrowthSpec { spring: Spring::NONE, ..GrowthSpec::default() }).virtual_rows(rows.len(), 110.0,
            move |_app: &App, i| format!("source-comment-{}", keys[i].id).into(),
            move |app: &App, i| app.story().and_then(|story| story.source().map(|source|
                comment_row(story, rows[i].id, rows[i].depth, &kit, t, accent, RenderContext::Source(source), reduced, app.keyboard_selection))).unwrap_or_else(el)));
    col().key("source-viewer").role("region").label(format!("Comment by {}", source.captured().author))
        .focusable(true).w_full().h_full().scroll(ScrollSpec { stick_bottom: false }).scroll_edge_fade(20.0)
        .on_wheel(App::pointer_wheel)
        .on_scroll_offset(on_scroll).child(content)
}

#[cfg(test)]
mod tests {
    use super::{drawn_text, MAX_TEXT};

    #[test]
    fn discussion_text_is_trimmed_and_capped_at_a_character_boundary() {
        let long = "é".repeat(MAX_TEXT + 10);
        let text = drawn_text(&long);
        assert_eq!(text.chars().count(), MAX_TEXT + 1);
        assert!(text.ends_with('…'));
        assert_eq!(drawn_text("  A narrow ABI.  "), "A narrow ABI.");
    }
}
