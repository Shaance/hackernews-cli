//! Window and reading input routing.

use super::{App, Notice, DEFAULT_GRID, INSPECTOR_MIN, INSPECTOR_MAX};
use hn_lib::event::StoryAction;
use hn_lib::feed_state;
use rune::KeyEvent;
use rune::Tx;
use rune_kit::PaneState;
use crate::keys;
use crate::keys::StoryKey;
use crate::keys::WindowKey;
use crate::layout::ZOOM_STEPS;
use crate::story::Story;
use crate::webview;

impl App {
    pub(super) fn window_action(&mut self, action: WindowKey, tx: &mut Tx) -> bool {
        match action {
            WindowKey::Settings => self.open_summary_settings(),
            WindowKey::Help => self.help = !self.help,
            WindowKey::ToggleSplit => if self.story.is_some() {
                self.inspector.set_open(!self.inspector.is_open());
            },
            WindowKey::ZoomIn | WindowKey::ZoomOut | WindowKey::ActualSize => {
                let old_zoom = self.zoom;
                let index = ZOOM_STEPS.iter().position(|&step| step == self.zoom)
                    .expect("zoom is always a supported step");
                self.zoom = match action {
                    WindowKey::ZoomIn => ZOOM_STEPS[(index + 1).min(ZOOM_STEPS.len() - 1)],
                    WindowKey::ZoomOut => ZOOM_STEPS[index.saturating_sub(1)],
                    WindowKey::ActualSize => 1.0,
                    _ => unreachable!(),
                };
                if self.zoom != old_zoom {
                    let pane = self.inspector;
                    self.inspector = PaneState::new(pane.default_width() * old_zoom / self.zoom)
                        .range(INSPECTOR_MIN / self.zoom, INSPECTOR_MAX / self.zoom)
                        .with_width(pane.width() * old_zoom / self.zoom);
                    self.inspector.set_open(pane.is_open());
                }
                webview::set_zoom(self.zoom);
                // An unchanged view-point rect does not trigger on_layout when zoom changes.
                if let Some(rect) = &mut self.live_bounds {
                    *rect = rect.map(|value| value * self.zoom / old_zoom);
                    if self.live_shown { webview::set_bounds(*rect); }
                }
                if self.zoom != old_zoom || action == WindowKey::ActualSize {
                    self.toast = Some(Notice { message: format!("{:.0}%", self.zoom * 100.0),
                        until: f64::INFINITY, show_dot: false });
                }
                tx.instant();
            }
            WindowKey::None => return false,
        }
        true
    }

    pub(crate) fn keyboard_navigation(&mut self, ev: &KeyEvent) {
        if self.settings.is_some() || self.help || keys::parity_key(ev).is_none() { return; }
        if matches!(ev.key, rune::Key::Tab | rune::Key::Home | rune::Key::End | rune::Key::PageUp | rune::Key::PageDown)
            || (self.story.is_some() && keys::story_key(ev) != StoryKey::None)
            || (self.story.is_none() && (keys::story_action(ev) != StoryAction::None
                || matches!(ev.key, rune::Key::Left | rune::Key::Right)))
        {
            if self.story.is_none() && (matches!(ev.key, rune::Key::Left | rune::Key::Right)
                || matches!(keys::story_action(ev), StoryAction::NextStory | StoryAction::PrevStory
                    | StoryAction::FirstStory | StoryAction::LastStory | StoryAction::NextPage
                    | StoryAction::PrevPage | StoryAction::SetType(_)))
                || self.story.is_some() && matches!(keys::story_key(ev), StoryKey::NextComment | StoryKey::PrevComment
                    | StoryKey::NextSibling | StoryKey::PrevSibling | StoryKey::Parent | StoryKey::Fold
                    | StoryKey::Unfold | StoryKey::FirstComment | StoryKey::LastComment | StoryKey::PopTrail
                    | StoryKey::Enter | StoryKey::Back | StoryKey::Focus) {
                self.keyboard_selection = true;
            }
            self.hints.snap(1.0);
        }
    }

    pub(crate) fn pointer_wheel(s: &mut App, ev: rune::WheelEvent, _: &mut Tx) -> bool {
        if !ev.momentum && (ev.dx != 0.0 || ev.dy != 0.0) { s.hints.set(0.3); }
        false
    }
}

pub(super) fn on_key(s: &mut App, ev: &KeyEvent, tx: &mut Tx) -> bool {
    s.keyboard_navigation(ev);
    // AppKit consumes native menu equivalents first; this also handles headless and layout variants.
    if s.window_action(keys::window_key(ev), tx) { return true; }
    // A dialog owns the keys while it is open: the feed behind it must not move. `Esc` never
    // reaches here — the runtime dismisses the modal first — and `?` closes the sheet.
    if s.settings.is_some() {
        return false;
    }
    if s.help {
        if keys::story_action(ev) == StoryAction::ToggleHelp {
            s.help = false;
            return true;
        }
        return false;
    }
    if s.story.is_some() && keys::story_action(ev) == StoryAction::ToggleHelp {
        s.help = true;
        return true;
    }
    // `O` (capital) always opens the real page in the system browser — the TUI's
    // `reader_actions.rs:42,74` distinction — never the extracted article split.
    if keys::is_open_external(ev) {
        s.pending_external = true;
        return true;
    }
    if s.story.is_some() {
        return on_story_key(s, ev, tx);
    }
    // Desktop feed exception: the terminal feed has no summary-settings action.
    if matches!(&ev.key, rune::Key::Text(text) if text == "M")
        && matches!(keys::parity_key(ev), Some(crossterm::event::KeyCode::Char('M'))) {
        s.open_summary_settings();
        return true;
    }
    if matches!(
        keys::parity_key(ev),
        Some(crossterm::event::KeyCode::Left | crossterm::event::KeyCode::Right)
    ) {
        let (_, cols) = s.measured.get().unwrap_or(DEFAULT_GRID);
        let shown = s.feed.stories.len();
        if shown > 0 && !s.feed.request.is_loading() {
            let cols = cols.max(1);
            let per_column = shown.div_ceil(cols);
            let column = s.feed.selected / per_column;
            let right = matches!(ev.key, rune::Key::Right);
            if (right && column + 1 < cols && (column + 1) * per_column < shown)
                || (!right && column > 0)
            {
                let start = if right { (column + 1) * per_column } else { (column - 1) * per_column };
                s.feed.selected = (start + s.feed.selected % per_column).min(shown - 1);
            } else if !right || s.feed.display_cursor + shown < s.feed.id_snapshot.len() {
                let mut action = if right { feed_state::next_page(&s.feed.state(), s.cells()) }
                    else { feed_state::prev_page(&s.feed.state(), s.cells()) };
                if let feed_state::FeedAction::TurnPage { landing, .. } = &mut action {
                    *landing = Some(if right { hn_lib::app::PageLanding::Start } else { hn_lib::app::PageLanding::End });
                    s.apply(action);
                }
            }
            if let Some(id) = s.feed.selected_id() {
                tx.scroll_into_view(format!("story-{id}"));
            }
        }
        return true;
    }
    let cells = s.cells();
    let state = s.feed.state();
    let action = match keys::story_action(ev) {
        StoryAction::None => return false,
        StoryAction::Quit => {
            // The window's own state is written on the way out, whatever the throttle says.
            if !s.headless {
                let snapshot = s.snapshot();
                s.write_layout(snapshot, rune::scene_clock().now);
            }
            tx.quit();
            return true;
        }
        StoryAction::NextStory => feed_state::next_story(&state, cells),
        StoryAction::PrevStory => feed_state::prev_story(&state, cells),
        StoryAction::FirstStory => feed_state::FeedAction::Move { selected: 0 },
        StoryAction::LastStory => feed_state::FeedAction::Move {
            selected: state.stories.len().saturating_sub(1),
        },
        StoryAction::NextPage => feed_state::next_page(&state, cells),
        StoryAction::PrevPage => feed_state::prev_page(&state, cells),
        StoryAction::SetType(tab) => {
            tx.blur();
            feed_state::switch_tab(&state, tab)
        },
        StoryAction::ToggleBookmark => {
            s.toggle_saved();
            return true;
        }
        StoryAction::Refresh => {
            s.refresh();
            return true;
        }
        // `Enter`, `o` and `c` all open the story: the split is where the article and the
        // discussion live, so there is no separate "open in browser" to choose between.
        StoryAction::OpenUrl | StoryAction::ViewComments => {
            s.open_selected();
            return true;
        }
        StoryAction::ToggleHelp => {
            s.help = true;
            return true;
        }
    };
    s.apply(action);
    if let Some(id) = s.feed.selected_id() {
        tx.scroll_into_view(format!("story-{id}"));
    }
    true
}

/// The story split's keys, from `src/reader/interaction.rs:7` through [`keys::story_key`], plus
/// DESIGN.md's thread table.
///
/// The order is the layering, and `Esc` unwinds it in reverse: the link chooser takes keys while
/// it is open, then the source viewer, then the discussion, and only then the feed.
pub(super) fn on_story_key(s: &mut App, ev: &KeyEvent, tx: &mut Tx) -> bool {
    let key = keys::story_key(ev);
    if matches!(key, StoryKey::Back) { return s.story_back(tx); }
    let visible_comment = s.visible_comment.get().and_then(|(id, source, row, viewport)| {
        let y = row[0] - s.comment_offsets[usize::from(source)].get();
        let top = viewport[0] + if source { 0.0 } else { 27.0 };
        (y < viewport[0] + viewport[1] && y + row[1] > top).then_some((id, source))
    });
    let Some(story) = s.story.as_mut() else {
        return false;
    };
    // The chooser is a surface over the discussion: while it is open it owns j/k, Enter and Esc.
    if story.links.is_some() {
        match key {
            StoryKey::NextCitation | StoryKey::PrevCitation => return false,
            StoryKey::NextComment => {
                story.next_link();
                return true;
            }
            StoryKey::PrevComment => {
                story.prev_link();
                return true;
            }
            StoryKey::Enter => {
                if let Some(url) = story.take_link() {
                    let _ = tx.open_url(&url);
                }
                return true;
            }
            _ => {}
        }
    }
    // Source navigation shares the thread operations but keeps its own cursor and folds.
    if story.source.is_some() {
        match key {
            StoryKey::NextScreen => {
                tx.scroll_page("source-viewer", 1);
                return true;
            }
            StoryKey::PrevScreen => {
                tx.scroll_page("source-viewer", -1);
                return true;
            }
            StoryKey::NextCitation => {
                story.next_citation();
                tx.instant();
                tx.scroll_to_y("source-viewer", 0.0);
                s.load_source_replies();
                return true;
            }
            StoryKey::PrevCitation => {
                story.prev_citation();
                tx.instant();
                tx.scroll_to_y("source-viewer", 0.0);
                s.load_source_replies();
                return true;
            }
            StoryKey::Links => {
                s.toast = Some(Notice { message: "Open links from the discussion".to_string(), until: f64::INFINITY, show_dot: true });
                return true;
            }
            StoryKey::ToggleSummary | StoryKey::Refresh | StoryKey::Settings | StoryKey::ToggleSaved
            | StoryKey::NextSummaryScreen | StoryKey::PrevSummaryScreen => return true,
            _ => {}
        }
    }
    let in_source = story.source.is_some();
    let (root, cursor) = story.with_thread(|thread| (thread.root(), thread.cursor()));
    match key {
        StoryKey::None => false,
        StoryKey::Back => unreachable!("back is handled before borrowing the story"),
        StoryKey::NextCitation => {
            if story.citations().is_empty() { return false; }
            story.next_citation();
            reveal_citation(story, tx, false);
            true
        }
        StoryKey::PrevCitation => {
            if story.citations().is_empty() { return false; }
            story.prev_citation();
            reveal_citation(story, tx, false);
            true
        }
        StoryKey::Enter => {
            // A selected citation is what Enter reads; otherwise it is the thread's own expand,
            // which re-roots on a `continue thread` row.
            if story.open_source() {
                s.load_source_replies();
                return true;
            }
            let id = cursor;
            let can_activate = id.is_some_and(|id| story.thread.has_children(id));
            if let Some(id) = id { s.activate_comment(id); }
            if can_activate {
                if let Some(story) = s.story.as_mut() { thread_moved(story, root, true, visible_comment, tx); }
            }
            true
        }
        StoryKey::ToggleSummary => {
            story.toggle_summary();
            true
        }
        StoryKey::Refresh => {
            s.refresh_comments();
            true
        }
        StoryKey::Settings => {
            s.open_summary_settings();
            true
        }
        StoryKey::Links => { s.choose_links(); true },
        StoryKey::ToggleSaved => {
            let id = story.id();
            s.toggle_saved_id(id);
            true
        }
        StoryKey::NextComment | StoryKey::PrevComment | StoryKey::NextSibling | StoryKey::PrevSibling
        | StoryKey::Parent | StoryKey::Fold | StoryKey::FirstComment | StoryKey::LastComment
        | StoryKey::PopTrail => {
            if !in_source { story.citation_focus = false; }
            let source_root = story.source.as_ref().map(|source| source.id);
            let loading = story.pending_reply.as_ref().is_some_and(|request| Some(request.parent_id) == cursor);
            let changed = story.with_thread(|thread| match key {
                StoryKey::NextComment => thread.next(),
                StoryKey::PrevComment => thread.prev(),
                StoryKey::NextSibling if !in_source || cursor != root => thread.next_sibling(),
                StoryKey::PrevSibling if !in_source || cursor != root => thread.prev_sibling(),
                StoryKey::Parent if cursor != source_root => thread.to_parent(),
                StoryKey::Fold if matches!(&ev.key, rune::Key::Text(text) if text == "c") && !loading => thread.collapse(),
                StoryKey::Fold => thread.fold(),
                StoryKey::FirstComment => thread.first(),
                StoryKey::LastComment => thread.last(),
                StoryKey::PopTrail if cursor != source_root => thread.pop_trail(),
                _ => false,
            });
            if changed { thread_moved(story, root, key == StoryKey::Fold, visible_comment, tx); }
            true
        }
        StoryKey::Unfold => {
            if !in_source { story.citation_focus = false; }
            let id = cursor;
            let retry = id.is_some_and(|id| story.thread.children_of(id).is_empty()
                && story.thread.reply_batch(id, false).is_empty()
                && !story.thread.reply_batch(id, true).is_empty());
            if story.with_thread(|thread| thread.unfold()) { thread_moved(story, root, true, visible_comment, tx); }
            if let Some(id) = id { s.load_replies(id, retry); }
            true
        }
        StoryKey::NextScreen => {
            tx.scroll_page("discussion-scroll", 1);
            true
        }
        StoryKey::PrevScreen => {
            tx.scroll_page("discussion-scroll", -1);
            true
        }
        StoryKey::NextSummaryScreen => {
            tx.scroll_page("summary-scroll", 1);
            true
        }
        StoryKey::PrevSummaryScreen => {
            tx.scroll_page("summary-scroll", -1);
            true
        }
        StoryKey::Focus => {
            if !in_source { story.citation_focus = false; }
            story.with_thread(|thread| thread.toggle_focus());
            true
        }
    }
}

pub(super) fn reveal_citation(story: &Story, tx: &mut Tx, focus: bool) {
    // A replacement can remove every citation while its old source is still open.
    if focus {
        tx.focus("summary-scroll", true);
        if !story.citation_focus { tx.announce("Back to summary", rune::LiveRegion::Polite); }
    }
    if !story.citation_focus { return; }
    let Some(id) = story.citations().get(story.citation).copied() else { return; };
    let Some(index) = story.summary.readable().and_then(|summary| summary.bullets.iter().position(|point| point.sources.contains(&id))) else { return; };
    let key = format!("summary-text-{index}");
    tx.scroll_into_view_in("summary-scroll", key.clone());
    if focus {
        // Rune's inline links are accessibility nodes inside one text element, so the
        // viewport is the focusable return target; the selected reference stays visible.
        if let Some(source) = story.sources.iter().find(|source| source.id == id) {
            tx.announce(format!("Back to summary, comment by {} selected", source.author), rune::LiveRegion::Polite);
        }
    }
}

/// Keep the focused comment in view after a thread transition.
///
/// A transition that re-roots the tree glides the viewport to the new root on `GLIDE` — the
/// rows crossfade in place, so the offset is the only thing that travels. Anything else just
/// reveals the row the cursor landed on.
fn thread_moved(story: &mut Story, before: Option<i32>, fold: bool, visible: Option<(i32, bool)>, tx: &mut Tx) {
    let source = story.source.is_some();
    if fold {
        let (root, cursor) = story.with_thread(|thread| (thread.root(), thread.cursor()));
        if root == before && let Some(id) = cursor && visible == Some((id, source)) {
            tx.keep_on_screen(format!("{}comment-{id}", if source { "source-" } else { "" }));
            return;
        }
    }
    if let Some(source) = &story.source {
        if source.nav.root != before {
            tx.scroll_to_y("source-viewer", 0.0);
        } else if let Some(id) = source.nav.cursor {
            tx.scroll_into_view_in("source-viewer", format!("source-comment-{id}"));
        }
        return;
    }
    if story.thread.root() != before {
        tx.scroll_to("discussion-scroll", 0.0, 0.0);
    } else if let Some(id) = story.thread.cursor() {
        tx.scroll_into_view_in("discussion-scroll", format!("comment-{id}"));
    }
}
