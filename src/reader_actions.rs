use crate::{handle_comment_action, handle_story_action, reader::Reader, AppMessage};
use anyhow::Result;
use crossterm::event::KeyCode;
use hn_lib::{
    app::{App, View},
    event::{handle_comments_key, StoryAction},
    HackerNewsCliServiceImpl,
};
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
    if reader.is_open() {
        if key == KeyCode::Char('o') {
            if let Some(story) = reader.current_story() {
                reader.read_original(story, true);
            }
            return Ok(true);
        }
        let position = (app.comment_cursor, app.comment_scroll);
        if key == KeyCode::Char('r') {
            handle_comment_action(
                app,
                handle_comments_key(key),
                tx,
                service,
                bookmark_path.as_deref(),
            )
            .await?;
            if position != (app.comment_cursor, app.comment_scroll) {
                reader.comments_moved();
            }
            return Ok(true);
        }
        let key = if key == KeyCode::Char('O') {
            KeyCode::Char('o')
        } else {
            key
        };
        if matches!(
            key,
            KeyCode::Char('s') | KeyCode::Char('o') | KeyCode::Char('?')
        ) || !reader.key(key)
        {
            // Paging is applied by App during the next render, after completions are polled.
            if matches!(key, KeyCode::PageDown | KeyCode::PageUp) {
                reader.comments_moved();
            }
            handle_comment_action(
                app,
                handle_comments_key(key),
                tx,
                service,
                bookmark_path.as_deref(),
            )
            .await?;
        } else if !reader.is_open() {
            app.view_stories();
        }
        if position != (app.comment_cursor, app.comment_scroll) {
            reader.comments_moved();
        }
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
