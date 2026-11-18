//! Window setup and asynchronous result polling.

use super::{App, Live, Notice, Ranking, RequestLifecycle, Arrival, ScrollPacing, DEFAULT_GRID, TOAST_SECONDS, LOAD_TIMEOUT, LAYOUT_SECONDS, SCROLL_PACING_SECONDS};
use std::time::Duration;
use hn_lib::app::StoryType;
use rune::Tx;
use rune::Waker;
use crate::story_io::StoryUpdate;
use crate::story::SummaryState;
use crate::webview;

/// One aggregate fill request, then the waker that brings its answer back to `poll`.
pub(super) fn setup(s: &mut App, waker: Waker) {
    webview::set_waker(waker.clone());
    s.scroll_waker = Some(waker.clone());
    s.bridge.set_waker(waker);
    // The remembered tab and page decide what the first request asks for, so they land before
    // it. The measured grid re-requests if the window is not the default size.
    s.restore();
    s.request(DEFAULT_GRID, Ranking::Fresh);
}

pub(super) fn poll(s: &mut App, now: f64, tx: &mut Tx) -> bool {
    let back_requested = matches!(&s.live, Live::Created { child, .. } if child.take_back());
    poll_updates(s, now, tx, back_requested)
}

pub(super) fn poll_updates(s: &mut App, now: f64, tx: &mut Tx, back_requested: bool) -> bool {
    let native_back = back_requested && s.story_back(tx);
    let mut changed = native_back;
    let mut toast_expired = false;
    let mut feed_arrived = false;
    let mut summary_arrived = false;
    let mut replies_arrived = false;
    match s.scroll_pacing.get() {
        ScrollPacing::Moved => {
            s.scroll_pacing.set(ScrollPacing::Until(now + SCROLL_PACING_SECONDS));
            tx.wake_after(Duration::ZERO);
        }
        ScrollPacing::Until(until) if now < until => tx.wake_after(Duration::ZERO),
        ScrollPacing::Until(_) => s.scroll_pacing.set(ScrollPacing::Idle),
        ScrollPacing::Idle => {}
    }
    if let Some(filled) = s.bridge.take() {
        let landed = s.feed.apply_fill(filled);
        changed |= landed;
        if landed { feed_arrived = s.accept_arrival(); }
    }
    while let Some(update) = {
        let configured = s.configured();
        let article = s.story.as_ref().is_some_and(|story|
            matches!(story.article_summary, crate::story::ArticleSummaryState::Generating { .. }));
        let discussion = s.story.as_ref().is_some_and(|story| story.summary.is_generating());
        s.bridge.take_story(&s.config, configured && article, configured && discussion)
    } {
        let summary_finished = matches!(&update,
            StoryUpdate::Summary { .. } | StoryUpdate::Discussion { comments: Err(_), .. });
        let visible = s.inspector.get().is_open() && s.settings.is_none() && !s.help;
        // A landing for a story that is no longer open is dropped by `Story::land_update`.
        let landed = s.story.as_mut().map(|open| {
            let first_result = match &update {
                StoryUpdate::ArticleSummary { summary: Ok(_), .. } => matches!(
                    open.article_summary, crate::story::ArticleSummaryState::Generating { previous: None }),
                StoryUpdate::Summary { summary: Ok(_), .. } => matches!(
                    open.summary, SummaryState::Generating { previous: None }),
                _ => false,
            };
            let show = visible && open.summary_open && open.source().is_none();
            let visible_success = show && matches!(&update, StoryUpdate::Summary { summary: Ok(_), .. });
            let landed = open.land_update(update);
            (landed, landed && first_result && show,
                (landed && summary_finished && !visible_success).then(|| open.summary_notice()).flatten())
        });
        if let Some((landed, arrival, notice)) = landed {
            changed |= landed;
            summary_arrived |= arrival;
            // The card is in the inspector and may be folded or scrolled away, so the summary's
            // outcome is confirmed where the reader is looking.
            if let Some(message) = notice {
                s.toast = Some(Notice {
                    message,
                    until: f64::INFINITY,
                    show_dot: true,
                });
            }
        }
    }
    if let Some(result) = s.bridge.take_refresh() {
        changed |= s.land_comment_refresh(result);
    }
    if let Some(filled) = s.bridge.take_replies() {
        if let Some(story) = &mut s.story {
            let before: std::collections::HashSet<_> = match story.source() {
                Some(source) => story.thread.source_rows(&source.nav),
                None => story.thread.rows(),
            }.into_iter().map(|row| row.id).collect();
            let landed = story.land_replies(filled);
            let after = match story.source() {
                Some(source) => story.thread.source_rows(&source.nav),
                None => story.thread.rows(),
            };
            replies_arrived = landed && s.inspector.get().is_open() && s.settings.is_none() && !s.help
                && after.iter().any(|row| !before.contains(&row.id));
            changed |= landed;
        }
    }
    // `O` / "Open in browser": the real page, in the system browser, via the TUI's safe
    // opener (scheme-checked, HN item-page fallback). Headless runs never open anything.
    if std::mem::take(&mut s.pending_external) {
        s.open_current_external();
    }
    // `--open-story <id>`: open the story once the feed holds it. It waits for the first fill
    // rather than opening something else, and says so if the id is not on this page.
    if let Some(id) = s.open_story.take() {
        if s.feed.stories.iter().any(|story| story.id == id) {
            s.open_id(id);
            changed = true;
        } else if s.feed.request.is_loading() || s.feed.stories.is_empty() {
            s.open_story = Some(id);
        } else {
            s.feed.error = Some(format!("--open-story {id}: not on this page"));
            changed = true;
        }
    }
    // The live article pane: build the child view, load the open story's page, and keep it in
    // step with the split. A no-op in a headless run, where the pane is the extracted text.
    changed |= s.sync_live();
    // One request per invalidation: the measured grid is the only thing that moves it. A
    // resize keeps an in-flight fresh request fresh; otherwise it uses the accepted ranking.
    if let Some(grid) = s.measured.get() {
        let need = s.feed.fill(grid);
        if s.feed.tab == StoryType::Saved
            && matches!(s.feed.request, RequestLifecycle::Loading(_, _, previous, _)
                | RequestLifecycle::Settled(_, _, previous) if previous != grid)
        {
            if let Some(id) = s.feed.selected_id() {
                tx.scroll_into_view(format!("story-{id}"));
            }
            tx.instant();
            changed = true;
        }
        let ranking = match &mut s.feed.request {
            RequestLifecycle::Loading(fill, _, previous, _) | RequestLifecycle::Settled(fill, _, previous)
                if *fill == need => { *previous = grid; None }
            RequestLifecycle::Loading(_, ranking, _, _) => Some(*ranking),
            RequestLifecycle::BeforeFirst | RequestLifecycle::Settled(..) => Some(Ranking::Keep),
        };
        if let Some(ranking) = ranking {
            // A page turn has its own destination. Reflow the page on screen only when
            // there is no such turn, or when an earlier reflow is already pending.
            if s.feed.tab != StoryType::Saved
                && (s.feed.pending_cursor.is_none() || s.feed.pending_anchor.is_some())
                && let Some(id) = s.feed.selected_id()
            {
                let offset = (s.feed.selected / need.count) * need.count.get();
                if offset > 0 || s.feed.pending_cursor.is_some() {
                    s.feed.pending_anchor.get_or_insert(id);
                    s.feed.pending_cursor = (offset > 0).then_some(s.feed.display_cursor + offset);
                }
            }
            if !matches!(s.pending_arrival, Some(Arrival::Initial | Arrival::Tab(_))) {
                s.pending_arrival = None;
            }
            s.request(grid, ranking);
            changed = true;
        }
    }
    // A request that never lands must not leave the feed loading forever.
    match s.feed.poll_timeout(now, LOAD_TIMEOUT) {
        Some(true) => {
            s.bridge.cancel_feed();
            s.pending_arrival = None;
            s.arrival = None;
            changed = true;
        }
        Some(false) => tx.wake_after(Duration::from_secs_f64(LOAD_TIMEOUT)),
        None => {}
    }
    // On macOS the native creation hook restores the frame before the first layout. Other
    // platforms retain the existing one-time size restore.
    if !cfg!(target_os = "macos") && !s.headless && !s.restored {
        s.restored = true;
        if let Some(size) = s.layout.window {
            tx.resize_window_instant(size);
            changed = true;
        }
    }
    // The window's own state, written when it moves and at most every [`LAYOUT_SECONDS`].
    if !s.headless {
        let snapshot = s.snapshot();
        if snapshot != s.layout_written {
            let waited = now - s.layout_saved_at;
            if waited >= LAYOUT_SECONDS {
                s.write_layout(snapshot, now);
            } else {
                tx.wake_after(Duration::from_secs_f64(LAYOUT_SECONDS - waited));
            }
        }
    }
    if let Some(notice) = &mut s.toast {
        if notice.until.is_infinite() {
            notice.until = now + TOAST_SECONDS;
            tx.wake_after(Duration::from_secs_f64(TOAST_SECONDS));
        } else if now >= notice.until {
            s.toast = None;
            changed = true;
            toast_expired = true;
        }
    }
    if changed {
        if (native_back || feed_arrived || summary_arrived || replies_arrived || toast_expired) && !s.reduced {
            tx.animated();
        } else {
            tx.instant();
        }
    }
    changed
}
