use crate::{
    app::{App, View},
    event::{handle_comments_key, StoryAction},
    reader::Reader,
    tui::{handle_comment_action, handle_story_action, AppMessage},
    HackerNewsCliServiceImpl,
};
use anyhow::Result;
use crossterm::event::KeyCode;
use std::path::PathBuf;
use tokio::sync::mpsc;

pub async fn handle(
    reader: &mut Reader,
    app: &mut App,
    key: KeyCode,
    tx: mpsc::UnboundedSender<AppMessage>,
    service: HackerNewsCliServiceImpl,
    bookmark_path: &mut Option<PathBuf>,
) -> Result<bool> {
    if reader.settings_open() {
        reader.key(key);
        return Ok(true);
    }
    if app.show_links {
        return Ok(false);
    }
    if app.show_help {
        if matches!(key, KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?')) {
            app.show_help = false;
        }
        return Ok(true);
    }
    if app.snapshot.is_some() && matches!(key, KeyCode::Char('r') | KeyCode::Char('L')) {
        app.preview_notice = Some(if key == KeyCode::Char('r') {
            "This is a fixed snapshot. Get the full CLI on GitHub for live stories and complete discussions."
        } else {
            "Follow links in the article pane, or get the full CLI on GitHub to open links from comments."
        });
        return Ok(true);
    }
    if app.snapshot.is_some() && key == KeyCode::Char('O') {
        if let Some(story) = reader
            .current_story()
            .filter(|_| reader.is_open())
            .or_else(|| app.selected_story().cloned())
        {
            reader.read_original(story, true);
        }
        return Ok(true);
    }
    if reader.is_open() {
        let source = reader.source_id();
        if key == KeyCode::Char('o') && source.is_none() && !app.can_continue_thread() {
            if let Some(story) = reader.current_story() {
                reader.read_original(story, true);
            }
            return Ok(true);
        }
        let position = (app.thread.cursor(), app.comment_scroll);
        if key == KeyCode::Char('r') {
            handle_comment_action(
                app,
                handle_comments_key(key),
                tx,
                service,
                bookmark_path.as_deref(),
            )
            .await?;
            if position != (app.thread.cursor(), app.comment_scroll) {
                reader.comments_moved();
            }
            return Ok(true);
        }
        let request = reader.source_key(app, key);
        if let Some(request) = request {
            crate::tui::spawn_comment_request(app, request, &tx, &service);
        }
        if matches!(key, KeyCode::Char('s' | 'O' | '?')) || !reader.key(key) {
            // Paging is applied by App during the next render, after completions are polled.
            if matches!(key, KeyCode::PageDown | KeyCode::PageUp) {
                reader.comments_moved();
            }
            handle_comment_action(
                app,
                handle_comments_key(key),
                tx.clone(),
                service.clone(),
                bookmark_path.as_deref(),
            )
            .await?;
        } else if !reader.is_open() {
            app.view_stories();
        }
        if let Some(id) = reader.source_id().filter(|id| Some(*id) != source) {
            if let Some(request) = app.request_citation(id, false) {
                crate::tui::spawn_comment_request(app, request, &tx, &service);
            }
        }
        if position != (app.thread.cursor(), app.comment_scroll) {
            reader.comments_moved();
        }
        return Ok(true);
    }
    if matches!(app.view, View::Stories) && key == KeyCode::Char('M') {
        reader.edit_settings();
        return Ok(true);
    }
    if matches!(app.view, View::Stories)
        && matches!(
            key,
            KeyCode::Enter | KeyCode::Char('o') | KeyCode::Char('c')
        )
    {
        if let Some(story) = app.selected_story().cloned() {
            handle_story_action(app, StoryAction::ViewComments, tx, service, bookmark_path).await?;
            reader.open(story.clone());
            let read_article = key != KeyCode::Char('c');
            if read_article || reader.has_browser() {
                reader.read_original(story, read_article);
            }
        }
        return Ok(true);
    }
    Ok(false)
}
