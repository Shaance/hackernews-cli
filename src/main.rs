//! HackerNews TUI Application
//!
//! An interactive terminal user interface for browsing HackerNews stories and comments.

use anyhow::Result;
use crossterm::{
    event::DisableMouseCapture,
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use tokio::sync::mpsc;

use hn_lib::{
    app::{App, CommentState, StoryType, View},
    event::{
        handle_comments_key, handle_stories_key, CommentAction, Event, EventHandler, StoryAction,
    },
    HackerNewsCliService, HackerNewsCliServiceImpl,
};

/// Messages sent from async tasks to the main loop
#[derive(Debug)]
enum AppMessage {
    StoriesLoaded {
        story_type: StoryType,
        page: u32,
        result: Result<Vec<hn_lib::HNCLIItem>>,
    },
    CommentsLoaded(Result<Vec<hn_lib::app::Comment>>),
    CommentChildrenLoaded {
        comment_id: i32,
        result: Result<Vec<hn_lib::app::Comment>>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let mut app = App::new();

    // Create channel for async task communication
    let (tx, mut rx) = mpsc::unbounded_channel();

    // Load initial stories
    request_stories(&mut app, tx.clone(), false);

    // Run the app
    let result = run_app(&mut terminal, &mut app, tx, &mut rx).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

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

                // Check for messages from async tasks
                while let Ok(msg) = rx.try_recv() {
                    handle_app_message(app, msg);
                }
            }
            Event::Key(key) => {
                // Handle key based on current view
                match &app.view {
                    View::Stories => {
                        let action = handle_stories_key(key);
                        handle_story_action(app, action, tx.clone()).await?;
                    }
                    View::Comments { .. } => {
                        let action = handle_comments_key(key);
                        handle_comment_action(app, action, tx.clone()).await?;
                    }
                }
            }
        }

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
) -> Result<()> {
    match action {
        StoryAction::NextStory => app.next_story(),
        StoryAction::PrevStory => app.prev_story(),
        StoryAction::NextPage => {
            app.next_page();
            request_stories(app, tx, false);
        }
        StoryAction::PrevPage => {
            app.prev_page();
            request_stories(app, tx, false);
        }
        StoryAction::SetType(story_type) => {
            app.set_story_type(story_type);
            request_stories(app, tx, false);
        }
        StoryAction::OpenUrl => {
            if let Some(story) = app.selected_story() {
                let url = story.url.clone();
                tokio::spawn(async move {
                    let _ = open::that(url);
                });
            }
        }
        StoryAction::ViewComments => {
            if let Some(story) = app.selected_story() {
                let story_id = story.id;
                let story_title = story.title.clone();
                let story_url = story.url.clone();

                app.view_comments(story_id, story_title, story_url);

                // Fetch comments
                tokio::spawn(async move {
                    let service = HackerNewsCliServiceImpl::new();
                    let result = service.fetch_top_level_comments(story_id).await;
                    let _ = tx.send(AppMessage::CommentsLoaded(result));
                });
            }
        }
        StoryAction::Refresh => {
            request_stories(app, tx, true);
        }
        StoryAction::ToggleHelp => app.toggle_help(),
        StoryAction::Quit => app.should_quit = true,
        StoryAction::None => {}
    }

    Ok(())
}

/// Load stories for the current selection, using cache when available
fn request_stories(app: &mut App, tx: mpsc::UnboundedSender<AppMessage>, force_refresh: bool) {
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

    app.set_loading(true);

    let page_size = app.page_size;
    tokio::spawn(async move {
        let service = HackerNewsCliServiceImpl::new();
        let result = service
            .fetch_stories_page(story_type.as_str(), page_size, page)
            .await;
        let _ = tx.send(AppMessage::StoriesLoaded {
            story_type,
            page,
            result,
        });
    });
}

/// Handle comment view actions
async fn handle_comment_action(
    app: &mut App,
    action: CommentAction,
    tx: mpsc::UnboundedSender<AppMessage>,
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
            if let Some(comment) = app.selected_comment_mut() {
                match &comment.state {
                    CommentState::Collapsed => {
                        if !comment.child_ids.is_empty() {
                            let ids = comment.child_ids.clone();
                            let depth = comment.depth + 1;
                            let comment_id = comment.id;

                            // Set to loading
                            comment.state = CommentState::Loading;
                            app.rebuild_visible_comments();

                            // Spawn task to fetch children
                            tokio::spawn(async move {
                                let service = HackerNewsCliServiceImpl::new();
                                let result = service.fetch_comment_children(&ids, depth).await;
                                let _ = tx
                                    .send(AppMessage::CommentChildrenLoaded { comment_id, result });
                            });
                        }
                    }
                    CommentState::Expanded { .. } => {
                        // Collapse - just change state back to collapsed (child_ids preserved)
                        comment.state = CommentState::Collapsed;
                        app.rebuild_visible_comments();
                    }
                    CommentState::Loading => {
                        // Do nothing while loading
                    }
                }
            }
        }
        CommentAction::CollapseThread => {
            app.collapse_current_thread();
        }
        CommentAction::OpenUrl => {
            if let View::Comments { story_url, .. } = &app.view {
                let url = story_url.clone();
                tokio::spawn(async move {
                    let _ = open::that(url);
                });
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
        AppMessage::StoriesLoaded {
            story_type,
            page,
            result,
        } => match result {
            Ok(stories) => {
                app.apply_stories_page(story_type, page, stories);
            }
            Err(e) => {
                if app.story_type == story_type && app.current_page == page {
                    app.set_error(format!("Failed to load stories: {}", e));
                    app.stories.clear();
                }
            }
        },
        AppMessage::CommentsLoaded(result) => match result {
            Ok(comments) => app.set_comments(comments),
            Err(e) => app.set_error(format!("Failed to load comments: {}", e)),
        },
        AppMessage::CommentChildrenLoaded { comment_id, result } => {
            match result {
                Ok(children) => {
                    // Find the comment at any level and update its state
                    app.update_comment_by_id(comment_id, |comment| {
                        comment.state = CommentState::Expanded {
                            children: children.clone(),
                        };
                    });
                    app.rebuild_visible_comments();
                    app.set_loading(false);
                }
                Err(e) => {
                    app.set_error(format!("Failed to load comment children: {}", e));
                    // Revert the comment state back to collapsed
                    app.update_comment_by_id(comment_id, |comment| {
                        if let CommentState::Loading = comment.state {
                            comment.state = CommentState::Collapsed;
                        }
                    });
                    app.rebuild_visible_comments();
                }
            }
        }
    }
}
