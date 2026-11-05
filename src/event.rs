//! Event handling for keyboard input

use crossterm::event::KeyCode;

/// Handle key events for stories view
pub fn handle_stories_key(key: KeyCode) -> StoryAction {
    match key {
        // Navigation
        KeyCode::Char('j') | KeyCode::Down => StoryAction::NextStory,
        KeyCode::Char('k') | KeyCode::Up => StoryAction::PrevStory,
        KeyCode::Char('g') => StoryAction::FirstStory,
        KeyCode::Char('G') => StoryAction::LastStory,

        // Pagination
        KeyCode::Char('n') | KeyCode::Right | KeyCode::Char(' ') | KeyCode::PageDown => {
            StoryAction::NextPage
        }
        KeyCode::Char('p') | KeyCode::Left | KeyCode::PageUp => StoryAction::PrevPage,

        // Story type
        KeyCode::Char('1') => StoryAction::SetType(crate::app::StoryType::Top),
        KeyCode::Char('2') => StoryAction::SetType(crate::app::StoryType::New),
        KeyCode::Char('3') => StoryAction::SetType(crate::app::StoryType::Best),
        KeyCode::Char('4') => StoryAction::SetType(crate::app::StoryType::Saved),

        // Actions
        KeyCode::Enter | KeyCode::Char('o') | KeyCode::Char('O') => StoryAction::OpenUrl,
        KeyCode::Char('c') => StoryAction::ViewComments,
        KeyCode::Char('r') => StoryAction::Refresh,
        KeyCode::Char('s') => StoryAction::ToggleBookmark,

        // UI
        KeyCode::Char('?') => StoryAction::ToggleHelp,
        // Esc means "back" in the reader; at the feed it stays inert so it can't
        // quit the app by reflex. q is the quit key.
        KeyCode::Char('q') => StoryAction::Quit,

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
        KeyCode::PageDown | KeyCode::Char(' ') => CommentAction::PageDown,
        KeyCode::PageUp => CommentAction::PageUp,

        // Expand/collapse
        KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => CommentAction::ToggleExpand,
        KeyCode::Char('c') => CommentAction::CollapseThread,

        // Actions
        KeyCode::Char('o') => CommentAction::OpenUrl,
        KeyCode::Char('L') => CommentAction::ShowLinks,
        KeyCode::Char('s') => CommentAction::ToggleBookmark,
        KeyCode::Char('r') => CommentAction::Refresh,

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
    FirstStory,
    LastStory,
    NextPage,
    PrevPage,
    SetType(crate::app::StoryType),
    OpenUrl,
    ViewComments,
    Refresh,
    ToggleBookmark,
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
    PageDown,
    PageUp,
    NextSibling,
    PrevSibling,
    Parent,
    ToggleExpand,
    CollapseThread,
    OpenUrl,
    ShowLinks,
    ToggleBookmark,
    Refresh,
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
        // Esc no longer quits at the feed; it stays inert so it can't kill the
        // app by reflex (it means "back" in the reader).
        assert!(matches!(
            handle_stories_key(KeyCode::Esc),
            StoryAction::None
        ));
        assert!(matches!(
            handle_stories_key(KeyCode::Char('g')),
            StoryAction::FirstStory
        ));
        assert!(matches!(
            handle_stories_key(KeyCode::Char('G')),
            StoryAction::LastStory
        ));
        assert!(matches!(
            handle_stories_key(KeyCode::Char(' ')),
            StoryAction::NextPage
        ));
        assert!(matches!(
            handle_stories_key(KeyCode::PageDown),
            StoryAction::NextPage
        ));
        assert!(matches!(
            handle_stories_key(KeyCode::PageUp),
            StoryAction::PrevPage
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
        assert!(matches!(
            handle_comments_key(KeyCode::Char(' ')),
            CommentAction::PageDown
        ));
    }
}
