//! HackerNews TUI Application
//!
//! An interactive terminal user interface for browsing HackerNews stories and comments.

use anyhow::Result;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use tokio::sync::mpsc;

mod terminal_session;

use hn_lib::{
    app::{App, CommentState, StoryType, View},
    event::{
        handle_comments_key, handle_stories_key, CommentAction, Event, EventHandler, StoryAction,
    },
    url_open::open_story_url,
    HackerNewsCliService, HackerNewsCliServiceImpl,
};
use terminal_session::TerminalSession;

/// Messages sent from async tasks to the main loop
#[derive(Debug)]
enum AppMessage {
    Stories {
        story_type: StoryType,
        page: u32,
        request_generation: u64,
        result: Result<Vec<hn_lib::HNCLIItem>>,
    },
    Comments {
        story_id: i32,
        view_generation: u64,
        result: Result<Vec<hn_lib::app::Comment>>,
    },
    CommentChildren {
        story_id: i32,
        view_generation: u64,
        comment_id: i32,
        child_load_generation: u64,
        result: Result<Vec<hn_lib::app::Comment>>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut terminal = TerminalSession::new()?;

    // Create app state
    let mut app = App::new();
    let service = HackerNewsCliServiceImpl::new();

    // Create channel for async task communication
    let (tx, mut rx) = mpsc::unbounded_channel();

    // Load initial stories
    request_stories(&mut app, tx.clone(), service.clone(), false);

    // Run the app
    let result = run_app(terminal.terminal_mut(), &mut app, tx, &mut rx, service).await;
    terminal.restore();

    if let Err(err) = result {
        eprintln!("Error: {}", err);
    }

    Ok(())
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    tx: mpsc::UnboundedSender<AppMessage>,
    rx: &mut mpsc::UnboundedReceiver<AppMessage>,
    service: HackerNewsCliServiceImpl,
) -> Result<()> {
    let event_handler = EventHandler::default();
    let mut tick_count = 0usize;

    loop {
        // Render UI
        terminal.draw(|f| hn_lib::ui::render(f, app, tick_count))?;

        // Handle events (non-blocking poll)
        let event = event_handler.next()?;

        match event {
            Event::Tick => {
                tick_count = tick_count.wrapping_add(1);
            }
            Event::Key(key) => {
                // Handle key based on current view
                match &app.view {
                    View::Stories => {
                        let action = handle_stories_key(key);
                        handle_story_action(app, action, tx.clone(), service.clone()).await?;
                    }
                    View::Comments { .. } => {
                        let action = handle_comments_key(key);
                        handle_comment_action(app, action, tx.clone(), service.clone()).await?;
                    }
                }
            }
        }

        drain_app_messages(app, rx);

        // Check if we should quit
        if app.should_quit {
            break;
        }
    }

    Ok(())
}

/// Handle story view actions
async fn handle_story_action(
    app: &mut App,
    action: StoryAction,
    tx: mpsc::UnboundedSender<AppMessage>,
    service: HackerNewsCliServiceImpl,
) -> Result<()> {
    match action {
        StoryAction::NextStory => app.next_story(),
        StoryAction::PrevStory => app.prev_story(),
        StoryAction::NextPage => {
            app.next_page();
            request_stories(app, tx, service, false);
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
            request_stories(app, tx, service, true);
        }
        StoryAction::ToggleHelp => app.toggle_help(),
        StoryAction::Quit => app.should_quit = true,
        StoryAction::None => {}
    }

    Ok(())
}

/// Load stories for the current selection, using cache when available
fn request_stories(
    app: &mut App,
    tx: mpsc::UnboundedSender<AppMessage>,
    service: HackerNewsCliServiceImpl,
    force_refresh: bool,
) {
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
async fn handle_comment_action(
    app: &mut App,
    action: CommentAction,
    tx: mpsc::UnboundedSender<AppMessage>,
    service: HackerNewsCliServiceImpl,
) -> Result<()> {
    match action {
        CommentAction::NextComment => app.next_comment(),
        CommentAction::PrevComment => app.prev_comment(),
        CommentAction::NextSibling => app.next_comment_sibling(),
        CommentAction::PrevSibling => app.prev_comment_sibling(),
        CommentAction::Parent => app.parent_comment(),
        CommentAction::FirstComment => app.first_comment(),
        CommentAction::LastComment => app.last_comment(),
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
fn handle_app_message(app: &mut App, msg: AppMessage) {
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
                    app.apply_stories_error(
                        story_type,
                        page,
                        format!("Failed to load stories: {}", e),
                    );
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
                Err(e) => app.set_comment_error(format!("Failed to load comments: {}", e)),
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
                        app.set_comment_error(format!("Failed to load comment children: {}", e));
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

fn is_current_comments_view(app: &App, expected_story_id: i32, expected_generation: u64) -> bool {
    matches!(
        &app.view,
        View::Comments { story_id, .. } if *story_id == expected_story_id
    ) && app.comment_view_generation() == expected_generation
}

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;
