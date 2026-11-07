//! TUI composition layer: the crossterm event loop, its async message type, and the
//! action handlers that wire [`App`] to the service.
//!
//! This is the terminal front end, not part of the reader boundary. A non-terminal
//! front end binds to [`crate::reader`] and [`crate::feed_state`] instead of this module.
use crate::{
    app::{App, CommentState, StoryType, View},
    event::{handle_comments_key, handle_stories_key, CommentAction, StoryAction},
    reader, reader_actions,
    url_open::open_story_url,
    HackerNewsCliService, HackerNewsCliServiceImpl,
};
use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind};
use futures::StreamExt;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::time::sleep_until;

/// Messages sent from async tasks to the main loop
#[derive(Debug)]
pub enum AppMessage {
    Stories {
        story_type: StoryType,
        page: u32,
        request_generation: u64,
        result: Result<Vec<crate::HNCLIItem>>,
    },
    Comments {
        story_id: i32,
        view_generation: u64,
        result: Result<Vec<crate::app::Comment>>,
    },
    CommentChildren {
        story_id: i32,
        view_generation: u64,
        comment_id: i32,
        child_load_generation: u64,
        result: Result<Vec<crate::app::Comment>>,
    },
}


pub(crate) fn next_deadline(
    now: Instant,
    started: Instant,
    animating: bool,
    pending: bool,
) -> Option<Instant> {
    if animating {
        Some(now + Duration::from_millis(16))
    } else if pending {
        let frame = now.saturating_duration_since(started).as_millis() / 80;
        Some(started + Duration::from_millis((frame as u64 + 1) * 80))
    } else {
        None
    }
}

pub async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    tx: mpsc::UnboundedSender<AppMessage>,
    rx: &mut mpsc::UnboundedReceiver<AppMessage>,
    service: HackerNewsCliServiceImpl,
    bookmark_path: &mut Option<PathBuf>,
    summaries: &mut reader::Reader,
) -> Result<()> {
    let mut events = EventStream::new();
    let started = Instant::now();
    const FRAME_MS: u64 = 80;

    loop {
        // Render UI
        summaries.poll();
        drain_app_messages(app, rx);
        let frame = (started.elapsed().as_millis() / u128::from(FRAME_MS)) as u64;
        terminal.draw(|f| {
            if summaries.is_open() {
                summaries.render(f, app, frame as usize);
            } else {
                crate::ui::render(f, app, frame as usize);
            }
            if let Some(notice) = app.preview_notice {
                crate::ui::widgets::render_modal(
                    f,
                    app.theme(),
                    None,
                    "Preview limit",
                    |width, _| {
                        let mut lines: Vec<_> = textwrap::wrap(notice, usize::from(width).max(1))
                            .into_iter()
                            .map(|line| ratatui::text::Line::raw(line.into_owned()))
                            .collect();
                        lines.push(ratatui::text::Line::raw(""));
                        lines.push(ratatui::text::Line::styled(
                            "Press any key to return",
                            app.theme().dim,
                        ));
                        (lines, None)
                    },
                );
            }
        })?;

        // Input and app results wake immediately; only polled work needs a timer.
        let now = Instant::now();
        let deadline = next_deadline(
            now,
            started,
            app.has_active_motion(now) || summaries.has_active_motion(now),
            app.has_pending_work() || summaries.has_pending_work(),
        );
        let event = tokio::select! {
            event = events.next() => {
                let Some(event) = event else { break };
                event?
            }
            Some(message) = rx.recv() => {
                handle_app_message(app, message);
                continue;
            }
            _ = sleep_until(deadline.unwrap_or(now).into()), if deadline.is_some() => {
                continue;
            }
        };

        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                let key = key.code;
                if app.preview_notice.take().is_some() {
                    continue;
                }
                if reader_actions::handle(
                    summaries,
                    app,
                    key,
                    tx.clone(),
                    service.clone(),
                    bookmark_path,
                )
                .await?
                {
                    continue;
                }
                if app.show_links {
                    handle_link_key(app, key);
                    continue;
                }
                // Handle key based on current view
                match &app.view {
                    View::Stories => {
                        let action = handle_stories_key(key);
                        handle_story_action(
                            app,
                            action,
                            tx.clone(),
                            service.clone(),
                            bookmark_path,
                        )
                        .await?;
                    }
                    View::Comments { .. } => {
                        let action = handle_comments_key(key);
                        handle_comment_action(
                            app,
                            action,
                            tx.clone(),
                            service.clone(),
                            bookmark_path.as_deref(),
                        )
                        .await?;
                    }
                }
            }
            _ => {}
        }

        // Check if we should quit
        if app.should_quit {
            break;
        }
    }

    Ok(())
}

/// Handle story view actions
pub(crate) async fn handle_story_action(
    app: &mut App,
    action: StoryAction,
    tx: mpsc::UnboundedSender<AppMessage>,
    service: HackerNewsCliServiceImpl,
    bookmark_path: &mut Option<PathBuf>,
) -> Result<()> {
    match action {
        StoryAction::NextStory => {
            if app.next_story() && app.preview_notice.is_none() {
                request_stories(app, tx, service, false);
            }
        }
        StoryAction::PrevStory => {
            if app.prev_story() {
                request_stories(app, tx, service, false);
            }
        }
        StoryAction::FirstStory => app.jump_selection(0),
        StoryAction::LastStory => app.jump_selection(app.stories.len().saturating_sub(1)),
        StoryAction::NextPage => {
            app.next_page();
            if app.preview_notice.is_none() {
                request_stories(app, tx, service, false);
            }
        }
        StoryAction::PrevPage => {
            app.prev_page();
            request_stories(app, tx, service, false);
        }
        StoryAction::SetType(story_type) => {
            app.set_story_type(story_type);
            request_stories(app, tx, service, false);
        }
        StoryAction::OpenUrl => {
            if let Some(story) = app.selected_story() {
                if let Err(err) = open_story_url(&story.url, story.id) {
                    app.set_story_error(err.to_string());
                }
            }
        }
        StoryAction::ViewComments => {
            if let Some(story) = app.selected_story() {
                let story_id = story.id;
                let story_title = story.title.clone();
                let story_url = story.url.clone();
                let service = service.clone();

                app.view_comments(story_id, story_title, story_url);
                if !app.is_loading() {
                    return Ok(());
                }
                let view_generation = app.comment_view_generation();

                // Fetch comments
                tokio::spawn(async move {
                    let result = service.fetch_top_level_comments(story_id).await;
                    let _ = tx.send(AppMessage::Comments {
                        story_id,
                        view_generation,
                        result,
                    });
                });
            }
        }
        StoryAction::Refresh => {
            if app.story_type == StoryType::Saved {
                *bookmark_path = crate::bookmarks::reload(app);
                app.show_saved_stories();
            } else {
                request_stories(app, tx, service, true);
            }
        }
        StoryAction::ToggleBookmark => toggle_bookmark(app, bookmark_path.as_deref()),
        StoryAction::ToggleHelp => app.toggle_help(),
        StoryAction::Quit => app.should_quit = true,
        StoryAction::None => {}
    }

    Ok(())
}

/// Load stories for the current selection, using cache when available
pub fn request_stories(
    app: &mut App,
    tx: mpsc::UnboundedSender<AppMessage>,
    service: HackerNewsCliServiceImpl,
    force_refresh: bool,
) {
    if app.story_type == StoryType::Saved {
        app.show_saved_stories();
        return;
    }
    if force_refresh {
        app.story_cache.remove(&(app.story_type, app.current_page));
    }

    let story_type = app.story_type;
    let page = app.current_page;

    if !force_refresh {
        if let Some(cached) = app.cached_stories() {
            app.set_stories_for(story_type, page, cached);
            return;
        }
    }

    app.set_story_loading(true);
    let request_generation = app.next_story_request_generation();

    let page_size = app.page_size;
    tokio::spawn(async move {
        let result = service
            .fetch_stories_page(story_type.as_str(), page_size, page)
            .await;
        let _ = tx.send(AppMessage::Stories {
            story_type,
            page,
            request_generation,
            result,
        });
    });
}

/// Handle comment view actions
pub(crate) async fn handle_comment_action(
    app: &mut App,
    action: CommentAction,
    tx: mpsc::UnboundedSender<AppMessage>,
    service: HackerNewsCliServiceImpl,
    bookmark_path: Option<&Path>,
) -> Result<()> {
    match action {
        CommentAction::NextComment => app.next_comment(),
        CommentAction::PrevComment => app.prev_comment(),
        CommentAction::NextSibling => app.next_comment_sibling(),
        CommentAction::PrevSibling => app.prev_comment_sibling(),
        CommentAction::Parent => app.parent_comment(),
        CommentAction::FirstComment => app.first_comment(),
        CommentAction::LastComment => app.last_comment(),
        CommentAction::PageDown => app.scroll_comment_page(true),
        CommentAction::PageUp => app.scroll_comment_page(false),
        CommentAction::ToggleBookmark => toggle_bookmark(app, bookmark_path),
        CommentAction::ShowLinks => {
            app.show_links = true;
            app.overlay_opened = app.theme().motion.then(std::time::Instant::now);
            app.link_cursor = 0;
        }
        CommentAction::Refresh => {
            if let View::Comments { story_id, .. } = app.view {
                let view_generation = app.begin_comment_refresh();
                let previous = app.comments.clone();
                tokio::spawn(async move {
                    let result = service.refresh_comments(story_id, &previous).await;
                    let _ = tx.send(AppMessage::Comments {
                        story_id,
                        view_generation,
                        result,
                    });
                });
            }
        }
        CommentAction::ToggleExpand => {
            let active_story_id = match &app.view {
                View::Comments { story_id, .. } => *story_id,
                View::Stories => return Ok(()),
            };
            let view_generation = app.comment_view_generation();
            let child_load_generation = app.next_comment_child_load_generation();

            if let Some(comment) = app.selected_comment_mut() {
                match &comment.state {
                    CommentState::Collapsed => {
                        if !comment.child_ids.is_empty() {
                            let ids = comment.child_ids.clone();
                            let depth = comment.depth + 1;
                            let comment_id = comment.id;
                            let service = service.clone();

                            // Set to loading
                            comment.state = CommentState::Loading {
                                generation: child_load_generation,
                            };
                            app.rebuild_visible_comments();
                            app.set_comment_loading(true);

                            // Spawn task to fetch children
                            tokio::spawn(async move {
                                let result = service.fetch_comment_children(&ids, depth).await;
                                let _ = tx.send(AppMessage::CommentChildren {
                                    story_id: active_story_id,
                                    view_generation,
                                    comment_id,
                                    child_load_generation,
                                    result,
                                });
                            });
                        }
                    }
                    CommentState::Expanded { .. } => {
                        // Collapse - just change state back to collapsed (child_ids preserved)
                        comment.state = CommentState::Collapsed;
                        app.rebuild_visible_comments();
                        app.recompute_comment_loading(app.has_loading_comments());
                    }
                    CommentState::Loading { .. } => {
                        // Do nothing while loading
                    }
                }
            }
        }
        CommentAction::CollapseThread => {
            app.collapse_current_thread();
        }
        CommentAction::OpenUrl => {
            if let View::Comments {
                story_id,
                story_url,
                ..
            } = &app.view
            {
                if let Err(err) = open_story_url(story_url, *story_id) {
                    app.set_comment_error(err.to_string());
                }
            }
        }
        CommentAction::ToggleHelp => app.toggle_help(),
        CommentAction::Back => app.view_stories(),
        CommentAction::None => {}
    }

    Ok(())
}

/// Handle messages from async tasks
pub(crate) fn handle_app_message(app: &mut App, msg: AppMessage) {
    match msg {
        AppMessage::Stories {
            story_type,
            page,
            request_generation,
            result,
        } => {
            if !app.is_current_story_request(story_type, page, request_generation) {
                return;
            }

            match result {
                Ok(stories) => {
                    app.apply_stories_page(story_type, page, stories);
                }
                Err(e) => {
                    app.apply_stories_error(format!("Failed to load stories: {e}"));
                }
            }
        }
        AppMessage::Comments {
            story_id,
            view_generation,
            result,
        } => {
            if !is_current_comments_view(app, story_id, view_generation) {
                return;
            }

            match result {
                Ok(comments) => app.set_comments(comments),
                Err(e) => app.apply_comment_error(format!("Failed to load comments: {e}")),
            }
        }
        AppMessage::CommentChildren {
            story_id,
            view_generation,
            comment_id,
            child_load_generation,
            result,
        } => {
            if !is_current_comments_view(app, story_id, view_generation) {
                return;
            }

            match result {
                Ok(children) => {
                    let applied = app.replace_loading_comment_state(
                        comment_id,
                        child_load_generation,
                        CommentState::Expanded { children },
                    );
                    if applied {
                        app.rebuild_visible_comments();
                        app.recompute_comment_loading(app.has_loading_comments());
                    }
                }
                Err(e) => {
                    let applied = app.replace_loading_comment_state(
                        comment_id,
                        child_load_generation,
                        CommentState::Collapsed,
                    );
                    if applied {
                        app.set_comment_error(format!("Failed to load comment children: {e}"));
                        app.rebuild_visible_comments();
                        app.recompute_comment_loading(app.has_loading_comments());
                    }
                }
            }
        }
    }
}

fn drain_app_messages(app: &mut App, rx: &mut mpsc::UnboundedReceiver<AppMessage>) {
    while let Ok(msg) = rx.try_recv() {
        handle_app_message(app, msg);
    }
}

pub(crate) fn toggle_bookmark(app: &mut App, path: Option<&Path>) {
    let story = app
        .bookmark_story()
        .map(|s| (s.id, app.is_bookmarked(s.id)));
    match crate::bookmarks::toggle(app, path) {
        Err(error) => app.bookmark_notice = Some(error.to_string()),
        Ok(()) => {
            app.save_flash = story
                .filter(|(id, was_saved)| {
                    !was_saved && app.is_bookmarked(*id) && app.theme().motion
                })
                .map(|(id, _)| (id, std::time::Instant::now()));
        }
    }
}

pub(crate) fn handle_link_key(app: &mut App, key: KeyCode) {
    let links = app.current_links();
    match key {
        KeyCode::Char('j') | KeyCode::Down => {
            app.link_cursor = (app.link_cursor + 1).min(links.len().saturating_sub(1));
        }
        KeyCode::Char('k') | KeyCode::Up => app.link_cursor = app.link_cursor.saturating_sub(1),
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('L') => app.show_links = false,
        KeyCode::Enter | KeyCode::Char('o') => {
            if let Some(url) = links.get(app.link_cursor) {
                if let Err(error) = open_story_url(url, 0) {
                    app.set_comment_error(error.to_string());
                }
            }
            app.show_links = false;
        }
        _ => {}
    }
}

fn is_current_comments_view(app: &App, expected_story_id: i32, expected_generation: u64) -> bool {
    matches!(
        &app.view,
        View::Comments { story_id, .. } if *story_id == expected_story_id
    ) && app.comment_view_generation() == expected_generation
}


#[cfg(test)]
#[path = "tui_tests.rs"]
mod tests;
