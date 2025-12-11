//! Event handling for keyboard input

use anyhow::Result;
use crossterm::event::{self, Event as CrosstermEvent, KeyCode, KeyEventKind};
use std::time::Duration;

/// Application events
#[derive(Debug, Clone, Copy)]
pub enum Event {
    /// A key was pressed
    Key(KeyCode),
    /// Tick event for animations/updates
    Tick,
}

/// Event handler
pub struct EventHandler {
    /// Tick rate for animations
    tick_rate: Duration,
}

impl Default for EventHandler {
    fn default() -> Self {
        Self::new(Duration::from_millis(250))
    }
}

impl EventHandler {
    /// Create a new event handler
    pub fn new(tick_rate: Duration) -> Self {
        Self { tick_rate }
    }

    /// Poll for the next event
    pub fn next(&self) -> Result<Event> {
        // Poll for events with timeout
        if event::poll(self.tick_rate)? {
            match event::read()? {
                CrosstermEvent::Key(key) => {
                    // Only handle key press events (ignore release)
                    if key.kind == KeyEventKind::Press {
                        return Ok(Event::Key(key.code));
                    }
                }
                _ => {}
            }
        }
        Ok(Event::Tick)
    }
}

/// Handle key events for stories view
pub fn handle_stories_key(key: KeyCode) -> StoryAction {
    match key {
        // Navigation
        KeyCode::Char('j') | KeyCode::Down => StoryAction::NextStory,
        KeyCode::Char('k') | KeyCode::Up => StoryAction::PrevStory,

        // Pagination
        KeyCode::Char('n') | KeyCode::Right => StoryAction::NextPage,
        KeyCode::Char('p') | KeyCode::Left => StoryAction::PrevPage,

        // Story type
        KeyCode::Char('1') => StoryAction::SetType(crate::app::StoryType::Top),
        KeyCode::Char('2') => StoryAction::SetType(crate::app::StoryType::New),
        KeyCode::Char('3') => StoryAction::SetType(crate::app::StoryType::Best),

        // Actions
        KeyCode::Enter | KeyCode::Char('o') => StoryAction::OpenUrl,
        KeyCode::Char('c') => StoryAction::ViewComments,
        KeyCode::Char('r') => StoryAction::Refresh,

        // UI
        KeyCode::Char('?') => StoryAction::ToggleHelp,
        KeyCode::Char('q') | KeyCode::Esc => StoryAction::Quit,

        _ => StoryAction::None,
    }
}

/// Handle key events for comments view
pub fn handle_comments_key(key: KeyCode) -> CommentAction {
    match key {
        // Navigation
        KeyCode::Char('j') | KeyCode::Down => CommentAction::NextComment,
        KeyCode::Char('k') | KeyCode::Up => CommentAction::PrevComment,
        KeyCode::Char(']') => CommentAction::NextSibling,
        KeyCode::Char('[') => CommentAction::PrevSibling,
        KeyCode::Char('u') => CommentAction::Parent,
        KeyCode::Char('g') => CommentAction::FirstComment,
        KeyCode::Char('G') => CommentAction::LastComment,

        // Expand/collapse
        KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => CommentAction::ToggleExpand,
        KeyCode::Char('c') => CommentAction::CollapseThread,

        // Actions
        KeyCode::Char('o') => CommentAction::OpenUrl,

        // UI
        KeyCode::Char('?') => CommentAction::ToggleHelp,
        KeyCode::Char('q') | KeyCode::Esc | KeyCode::Char('h') | KeyCode::Left => {
            CommentAction::Back
        }

        _ => CommentAction::None,
    }
}

/// Actions that can be performed in stories view
#[derive(Debug, Clone)]
pub enum StoryAction {
    NextStory,
    PrevStory,
    NextPage,
    PrevPage,
    SetType(crate::app::StoryType),
    OpenUrl,
    ViewComments,
    Refresh,
    ToggleHelp,
    Quit,
    None,
}

/// Actions that can be performed in comments view
#[derive(Debug, Clone)]
pub enum CommentAction {
    NextComment,
    PrevComment,
    FirstComment,
    LastComment,
    NextSibling,
    PrevSibling,
    Parent,
    ToggleExpand,
    CollapseThread,
    OpenUrl,
    ToggleHelp,
    Back,
    None,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_story_key_mapping() {
        assert!(matches!(
            handle_stories_key(KeyCode::Char('j')),
            StoryAction::NextStory
        ));
        assert!(matches!(
            handle_stories_key(KeyCode::Down),
            StoryAction::NextStory
        ));
        assert!(matches!(
            handle_stories_key(KeyCode::Char('q')),
            StoryAction::Quit
        ));
    }

    #[test]
    fn test_comment_key_mapping() {
        assert!(matches!(
            handle_comments_key(KeyCode::Char('j')),
            CommentAction::NextComment
        ));
        assert!(matches!(
            handle_comments_key(KeyCode::Char(']')),
            CommentAction::NextSibling
        ));
        assert!(matches!(
            handle_comments_key(KeyCode::Char('[')),
            CommentAction::PrevSibling
        ));
        assert!(matches!(
            handle_comments_key(KeyCode::Char('u')),
            CommentAction::Parent
        ));
        assert!(matches!(
            handle_comments_key(KeyCode::Enter),
            CommentAction::ToggleExpand
        ));
        assert!(matches!(
            handle_comments_key(KeyCode::Esc),
            CommentAction::Back
        ));
    }
}
