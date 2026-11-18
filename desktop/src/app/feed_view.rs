//! Feed tabs, story rows, banners and footer.

use super::{App, Arrival, Columns, Grid, Ranking, RequestLifecycle, DEFAULT_GRID, FEED_GUTTER, ROW_HEIGHT};
use hn_lib::app::StoryType;
use hn_lib::{feed_state, HNCLIItem};
use rune::tok::{self, radius, type_};
use rune::{col, el, row, text, Color, El, ScrollSpec, Spring, SpringPair, Transition, Tx};
use rune_kit::{Keybind, Kit};
use super::chrome::{passive_hint, quiet_button, saved_dot};
use crate::feed::tab_index;

pub(super) fn feed_tabs(s: &App, k: &Kit, t: tok::Theme, accent: Color, reduced: bool) -> El {
    let selected = tab_index(s.feed.tab);
    let indicator = col().key("indicator").h(36.0).grid_cell(selected).radius(7.0).fill(k.c.bg_base)
        .box_shadow(rune::Shadow { spread: 0.5, color: k.c.border_strong, ..Default::default() })
        .motion(if reduced { Spring::NONE } else { Spring::new(0.18, 0.0) }).a11y_hidden()
        .child(row().pin(Some(0.0), None, Some(0.0), Some(3.0)).h(2.0).center()
            .child(el().w(16.0).h(2.0).radius(1.0).fill(accent)));
    let mut tabs = row().key("feed-tabs").role("radiogroup").label("Feed").focus_group()
        .equal_columns(4).h(36.0).center_y().radius(7.0).fill(k.c.layer_01).child(indicator);
    for (i, (tab, label)) in [
        (StoryType::Top, "Top"), (StoryType::New, "New"),
        (StoryType::Best, "Best"), (StoryType::Saved, "Saved"),
    ].into_iter().enumerate() {
        tabs = tabs.child(row().key(format!("segment{i}")).grid_cell(i).role("radio").label(label)
            .position_in_set(i + 1, 4).focusable(true).focus_selected(i == selected)
            .focus_ring(2.0, 1.0, accent).h(36.0).px(16.0).center().radius(7.0)
            .hover(k.c.hover).press(k.c.pressed)
            .on_press_tx(move |s: &mut App, tx: &mut Tx| {
                // Pointer selection returns arrows to the feed; an accessibility-focused tab keeps them.
                if !tx.by_keys() && !tx.focus_visible() { tx.blur(); }
                if tab != s.feed.tab { s.apply(feed_state::switch_tab(&s.feed.state(), tab)); }
            })
            .child(text(label).font(type_::TITLE).color(if i == selected { t.text } else { t.text_secondary }).a11y_hidden()));
    }
    tabs
}

pub(super) fn feed_body(s: &App, k: &Kit, t: tok::Theme, accent: Color, cols: Columns, reduced: bool, contrast: bool) -> El {
    let scroll_y = s.feed_scroll_y.clone();
    let mut body = col().w_full().h_full();
    if let Some(error) = s.saved.error() {
        body = body.child(banner(s, error, k, t));
    }
    // A failed ranking or window is a state with a retry, never a blank list.
    if let Some(error) = s.feed.error.as_deref() {
        body = body.child(feed_error(error, k, t));
    }
    body.child(
        feed_list(s, t, accent, cols, reduced)
            .on_wheel(App::pointer_wheel)
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .on_scroll_offset(move |pos| scroll_y.set(pos.offset[1]))
            .scroll_edge_fade(20.0)
            .grow(1.0)
            .min_h(0.0),
    )
    .child(el().w_full().h(1.0).fill(t.separator))
    .child(footer(s, k, t, accent, reduced, contrast))
}

pub(super) fn feed_arrival(x: f32, y: f32) -> Transition {
    let enter = Transition::offset(x, y).combined(Transition::opacity());
    let mut transition = Transition::asymmetric(enter, Transition::none())
        .springs(SpringPair::new(Spring::new(0.22, 0.0), Spring::NONE));
    transition.insertion.opacity = 0.2;
    transition
}

/// Column-major: story `c * rows + r`, so the left column fills first and `j` walks straight
/// down it. One scroll viewport for both columns; every cell keeps its own key, so
/// `Tx::scroll_into_view` reaches a story whichever column it is in.
fn feed_list(s: &App, t: tok::Theme, accent: Color, cols: Columns, reduced: bool) -> El {
    // The measured grid is available during this view, before poll can settle its fill.
    // Show the screenful containing the selection immediately, using the held stories.
    let capacity = Grid { columns: cols, ..s.measured.get().unwrap_or(DEFAULT_GRID) }.capacity();
    let start = if s.feed.tab != StoryType::Saved {
        (s.feed.selected / capacity) * capacity.get()
    } else { 0 };
    let shown = if s.feed.tab == StoryType::Saved { s.feed.stories.len() }
        else { s.feed.stories.len().saturating_sub(start).min(capacity.get()) };
    let per_column = shown.div_ceil(cols.count().get());
    // Keep the scroll viewport stable; only a completed page turn remounts this group.
    if s.feed.request.is_loading() && s.feed.stories.is_empty() && s.feed.error.is_none() {
        return feed_skeleton(s, t, cols, reduced);
    }
    let cols = cols.count().get();
    let transition = match s.arrival.filter(|_| !reduced) {
        Some(Arrival::Initial) => feed_arrival(0.0, 12.0),
        Some(Arrival::Tab(direction) | Arrival::Page(direction)) => feed_arrival(direction as f32 * 24.0, 0.0),
        _ => Transition::none(),
    };
    let mut list = col()
        .key(format!("feed-page-{}", s.feed_revision))
        .transition(transition)
        .instant_changes()
        .w_full()
        .pt(FEED_GUTTER)
        .pb(FEED_GUTTER)
        .gap(if cols == 2 { 0.0 } else { 2.0 });
    if shown == 0 && s.feed.error.is_none()
        && (s.feed.tab != StoryType::Saved || s.saved.error().is_none()) {
        let label = if s.feed.tab == StoryType::Saved { "Press S on a story to save it" } else { "No stories to show" };
        list = list.px(24.0).role("status").label(label)
            .child(text(label).font(type_::BODY).color(t.text_secondary).a11y_hidden());
        return col().key("feed-list").w_full().child(list);
    }
    for r in 0..per_column {
        let mut line = row().key(format!("line-{r}")).w_full().h(ROW_HEIGHT);
        for c in 0..cols {
            let i = c * per_column + r;
            if i >= shown {
                break;
            }
            if c > 0 {
                // The hairline between columns. Rows sit at gap 0 in two-column mode, so the
                // per-row rules join into one continuous divider.
                line = line.child(el().w(1.0).h_full().fill(t.separator));
            }
            line = line.child(story_row(s, start + i, t, accent, reduced).grow(1.0));
        }
        list = list.child(line);
    }
    col().key("feed-list").w_full().child(list)
}

fn story_row(s: &App, i: usize, t: tok::Theme, accent: Color, reduced: bool) -> El {
    let story = &s.feed.stories[i];
    let id = story.id;
    let rank = s.feed.display_cursor + i + 1;
    let selected = i == s.feed.selected;
    let failed = s.feed.failed.contains(&id);
    let saved = s.saved.contains(id);
    let in_saved_view = s.feed.tab == StoryType::Saved;

    let mut r = row()
        .key(format!("story-{id}"))
        .motion(if reduced || s.keyboard_selection { Spring::NONE } else { Spring::new(0.12, 0.0) })
        .instant_changes()
        .transition(Transition::none())
        .role("button")
        .label(format!("{} \u{00b7} {}", story.title, meta(story, failed)))
        .h_full()
        .px(12.0)
        .gap(14.0)
        .center_y()
        .radius(radius::SM)
        .focusable(true)
        .focus_selected(selected)
        // DESIGN.md's focus affordance is amber, not the system blue. `focus_highlight` paints a
        // solid overlay, which would bury the selected row's card fill, so the ring carries it.
        .focus_ring(1.5, 0.0, accent)
        .on_press(move |s: &mut App| {
            // While a page turn is in flight the rows on screen are the page being left:
            // selecting one would select it under the ranks of the page being loaded.
            if s.feed.request.is_loading() {
                return;
            }
            if let Some(index) = s.feed.stories.iter().position(|story| story.id == id) {
                s.feed.selected = index;
                s.open_id(id);
            }
        })
        .on_pointer_move_local(move |s: &mut App, ev, tx| {
            if s.pointer_can_select(ev) && !s.feed.request.is_loading() && let Some(index) = s.feed.stories.iter().position(|story| story.id == id)
                && s.feed.selected != index {
                s.feed.selected = index;
                s.keyboard_selection = false;
            } else {
                tx.live_only();
            }
        })
        .on_hover_change(move |s: &mut App, inside: bool, _tx: &mut Tx| {
            if inside {
                s.hover = Some(id);
            } else if s.hover == Some(id) {
                s.hover = None;
            }
        });
    r = r.child(
        el().w(2.0)
            .h(26.0)
            .radius(1.0)
            .fill(if selected { accent } else { Color::TRANSPARENT })
            .motion(if reduced || s.keyboard_selection { Spring::NONE } else { Spring::new(0.12, 0.0) })
            .transition(Transition::none()),
    );
    // The saved marker is a dot beside the rank, never right-aligned chrome. The Saved view is
    // already a list of saved stories, so its rows carry no dot.
    let mut lead = row().gap(6.0).center_y().child(
        row().w(18.0).justify_end().child(
            text(format!("{rank}"))
                .key(format!("rank-{rank}"))
                .font(type_::LABEL)
                .color(if selected { accent } else { t.text_secondary }),
        ),
    );
    if !in_saved_view {
        let key = format!("saved-dot-{id}");
        lead = lead.child(if saved {
            let mut pop = Transition::asymmetric(Transition::scale(0.6), Transition::none())
                .springs(SpringPair::new(Spring::new(0.25, 0.15), Spring::NONE));
            pop.insertion.opacity = 1.0;
            saved_dot(key, accent).animated_changes()
                .transition(if reduced { Transition::none() } else { pop })
                .motion_intent(rune::MotionIntent::OVERSHOOT)
        } else {
            el().key(format!("unsaved-dot-{id}")).size(6.0, 6.0)
        });
    }
    r = r.child(lead).child(
        col()
            .grow(1.0)
            .gap(4.0)
            .min_w(0.0)
            .child(
                text(story.title.clone())
                    .key(format!("title-{id}"))
                    .font(type_::BODY)
                    .color(if failed { t.text_secondary } else { t.text })
                    .truncate(),
            )
            .child(
                text(meta(story, failed))
                    .font(type_::LABEL)
                    .color(t.text_secondary),
            ),
    );
    // The right edge is hover-actions only; nothing persists there.
    if s.hover == Some(id) {
        let save = if in_saved_view { "unsave" } else { "save" };
        r = r.child(act(save, accent, t, move |s: &mut App| s.toggle_saved_id(id)));
    }
    if selected {
        r = r.fill(t.card.mix(accent, 0.08));
    }
    r
}

fn meta(story: &HNCLIItem, failed: bool) -> String {
    if failed {
        return "Could not load this story · press r to retry".to_string();
    }
    // The terminal app's wording, singular included.
    let comments = match story.comments {
        Some(1) => "1 comment".to_string(),
        Some(n) => format!("{n} comments"),
        None => "discuss".to_string(),
    };
    format!(
        "{} points  \u{00b7}  by {}  \u{00b7}  {comments}",
        story.score, story.author
    )
}

/// A hover action on a row. It is a control in its own right: a press on it runs its own
/// handler, not the row's select, because Rune dispatches a press to the deepest handler.
fn act(label: &str, color: Color, t: tok::Theme, on_press: impl Fn(&mut App) + 'static) -> El {
    row()
        .role("button")
        .label(label)
        .h(20.0)
        .px(7.0)
        .center()
        .radius(4.0)
        .fill(t.control_off)
        .focusable(true)
        .on_press(on_press)
        .child(text(label).font(type_::LABEL).color(color))
}

fn footer(s: &App, k: &Kit, t: tok::Theme, accent: Color, reduced: bool, contrast: bool) -> El {
    let h = |keys: &[&str], label: &str| passive_hint(hint(k, t, keys, label), &s.hints, reduced, contrast);
    let mut f = row()
        .key("feed-footer")
        .motion(Spring::NONE)
        .instant_changes()
        // Match the 18 pt page controls plus vertical padding while a label replaces them.
        .min_h(42.0)
        .w_full()
        .px(24.0)
        .py(12.0)
        .gap(18.0)
        .center_y();
    if !s.feed.stories.is_empty() {
        f = f.child(h(&["j", "k"], "move"))
        .child(h(&["←", "→"], "column / page"))
        .child(h(
            &["s"],
            if s.feed.tab == StoryType::Saved {
                "unsave"
            } else {
                "save"
            },
        ))
        .child(h(&["n", "p"], "page"));
    }
    f = f.child(h(&["1", "4"], "tabs"))
        .child(el().grow(1.0));
    if !s.feed.request.is_loading() && s.feed.exhausted(s.cells()) {
        f = f.child(
            text("No more stories")
                .font(type_::LABEL)
                .color(t.text_secondary),
        );
    }
    if s.feed.request.is_loading() && s.feed.stories.is_empty() && s.feed.error.is_none() {
        f = f.child(text(loading_label(s.feed.tab)).font(type_::LABEL).color(t.text_secondary).a11y_hidden());
    } else if s.feed.request.is_loading() && !s.feed.stories.is_empty() {
        let label = if matches!(s.feed.request, RequestLifecycle::Loading(_, Ranking::Fresh, _, _)) {
            "Refreshing…"
        } else {
            "Loading page…"
        };
        f = f.child(row().role("status").label(label).busy(true)
            .child(text(label).font(type_::LABEL).color(t.text_secondary).a11y_hidden()));
    } else if s.feed.tab != StoryType::Saved {
        f = f.child(stepper(s.feed.stepper(), t, accent));
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

/// `‹ N ›` — the current page only. The HN API reports no total, so there is no "of N" to show.
fn stepper(page: u32, t: tok::Theme, accent: Color) -> El {
    row()
        .key("page-stepper")
        .role("group")
        .label(format!("Page {page}"))
        .gap(2.0)
        .center_y()
        .child(chev("\u{2039}", "Previous page", t, |s: &mut App| {
            let cells = s.cells().get();
            let action = feed_state::prev_page(&s.feed.state(), cells);
            s.apply(action);
        }))
        .child(
            row()
                .w(20.0)
                .center()
                .child(text(format!("{page}")).font(type_::LABEL).color(accent)),
        )
        .child(chev("\u{203a}", "Next page", t, |s: &mut App| {
            let cells = s.cells().get();
            let action = feed_state::next_page(&s.feed.state(), cells);
            s.apply(action);
        }))
}

/// One arrow of the page stepper. It is a button, not decoration: a press turns the page.
fn chev(glyph: &str, label: &str, t: tok::Theme, on_press: impl Fn(&mut App) + 'static) -> El {
    row()
        .role("button")
        .label(label)
        .size(18.0, 18.0)
        .center()
        .radius(4.0)
        .fill(t.control_off)
        .focusable(true)
        .on_press(on_press)
        .child(text(glyph).font(type_::LABEL).color(t.text_secondary))
}

/// The saved-file banner keeps recovery actions next to the error.
fn banner(s: &App, error: &str, k: &Kit, t: tok::Theme) -> El {
    let message = if s.saved.read_failed() { "Your saved list could not be read. Nothing was changed." }
        else { "Your saved list could not be updated. Nothing was changed." };
    let banner = row()
        .key("saved-banner")
        .role("alert")
        .w_full()
        .px(24.0)
        .py(10.0)
        .gap(12.0)
        .center_y()
        .fill(t.card)
        .child(col().grow(1.0).min_w(0.0).gap(2.0)
            .child(text(message).font(type_::LABEL).color(t.text).truncate())
            .child(text(error.to_string()).font(type_::LABEL).color(t.text_secondary).truncate()))
        .child(
            quiet_button("Reload")
                .on_press(|s: &mut App| {
                    s.saved.reload();
                    s.refresh();
                })
                .build(k).key("saved-reload").cursor(rune::Cursor::Pointer),
        );
    #[cfg(target_os = "macos")]
    let banner = if s.saved.path().is_some() {
        banner.child(quiet_button("Reveal in Finder").on_press(|s: &mut App| {
            if !s.headless && let Some(path) = s.saved.path() {
                let _ = std::process::Command::new("/usr/bin/open").arg("-R").arg(path).spawn();
            }
        }).build(k).key("saved-reveal").cursor(rune::Cursor::Pointer))
    } else { banner };
    banner
}

/// The feed's own failure: the ranking or the window could not be loaded. It is a state with a
/// retry, never a blank list.
fn feed_error(error: &str, k: &Kit, t: tok::Theme) -> El {
    row()
        .key("feed-error")
        .role("alert")
        .w_full()
        .px(24.0)
        .py(10.0)
        .gap(12.0)
        .center_y()
        .fill(t.card)
        .child(
            text("Could not load the feed")
                .font(type_::LABEL)
                .color(t.text),
        )
        .child(
            text(error.to_string())
                .font(type_::LABEL)
                .color(t.text_secondary)
                .truncate_middle(),
        )
        .child(el().grow(1.0))
        .child(
            quiet_button("Retry")
                .on_press(|s: &mut App| s.refresh())
                .build(k).key("feed-retry").cursor(rune::Cursor::Pointer),
        )
}

/// Decorative rows occupy the same viewport and lanes as the feed, with one status owner.
pub(super) fn feed_skeleton(s: &App, t: tok::Theme, cols: Columns, reduced: bool) -> El {
    let rows = s.measured.get().unwrap_or(DEFAULT_GRID).rows.get();
    let cols = cols.count().get();
    let transition = if reduced { Transition::none() } else {
        let mut transition = Transition::asymmetric(Transition::opacity(), Transition::opacity())
            .springs(SpringPair::new(Spring::new(0.20, 0.0), Spring::new(0.12, 0.0)));
        transition.insertion.opacity = 0.35;
        transition
    };
    let mut skeleton = col().key("feed-placeholder").transition(transition).instant_changes().motion(Spring::NONE).w_full()
        .pt(FEED_GUTTER).pb(FEED_GUTTER)
        .gap(if cols == 2 { 0.0 } else { 2.0 }).a11y_hidden();
    for r in 0..rows {
        let mut line = row().w_full().h(ROW_HEIGHT);
        for c in 0..cols {
            if c > 0 { line = line.child(el().w(1.0).h_full().fill(t.separator)); }
            let width = [0.72, 0.88, 0.62, 0.81][(r + c) % 4];
            line = line.child(row().grow(1.0).h_full().px(12.0).gap(14.0).center_y()
                .child(el().w(2.0).h(26.0))
                .child(el().w(18.0).h(12.0).radius(3.0).fill(t.control_off))
                .child(col().grow(1.0).gap(8.0)
                    .child(el().w_frac(width).h(12.0).radius(3.0).fill(t.control_off))
                    .child(el().w_frac(width * 0.56).h(9.0).radius(3.0).fill(t.control_off))));
        }
        skeleton = skeleton.child(line);
    }
    col().key("feed-list").w_full().role("status").label(loading_label(s.feed.tab)).busy(true).child(skeleton)
}

fn loading_label(tab: StoryType) -> String {
    format!("Loading {} stories", match tab { StoryType::Top => "Top", StoryType::New => "New", StoryType::Best => "Best", StoryType::Saved => "Saved" })
}
