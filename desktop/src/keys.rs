//! The total Rune `Key` → crossterm `KeyCode` table, and the feed's and story's key routing.
//!
//! The terminal app's bindings live in `hn_lib::event::handle_stories_key`, which takes a
//! crossterm `KeyCode`. The window gets Rune `Key`s, so every Rune key has to be translated
//! before parity can be claimed. [`key_code`] is that table: total over the pinned variants,
//! with `None` for the keys the terminal app has no binding for.
//!
//! The story split's keys are [`story_key`]. They are not in `handle_stories_key` — the reader
//! owns them in the terminal app, in `src/reader/interaction.rs:7` — so that file is the parity
//! source, and the test below spells its bindings out.

use crossterm::event::KeyCode;
use hn_lib::event::{StoryAction, handle_stories_key};
use rune::{Key, KeyEvent};

/// The crossterm key a Rune key stands for, or `None` when the terminal app has no
/// equivalent (a bare modifier, a key Rune adds later).
pub fn key_code(key: &Key) -> Option<KeyCode> {
    match key {
        Key::Text(text) => single_char(text).map(KeyCode::Char),
        Key::Enter => Some(KeyCode::Enter),
        Key::Backspace => Some(KeyCode::Backspace),
        Key::Delete => Some(KeyCode::Delete),
        Key::Left => Some(KeyCode::Left),
        Key::Right => Some(KeyCode::Right),
        Key::Up => Some(KeyCode::Up),
        Key::Down => Some(KeyCode::Down),
        Key::Home => Some(KeyCode::Home),
        Key::End => Some(KeyCode::End),
        Key::PageUp => Some(KeyCode::PageUp),
        Key::PageDown => Some(KeyCode::PageDown),
        Key::Insert => Some(KeyCode::Insert),
        Key::PrintScreen => Some(KeyCode::PrintScreen),
        Key::ScrollLock => Some(KeyCode::ScrollLock),
        Key::Pause => Some(KeyCode::Pause),
        Key::Escape => Some(KeyCode::Esc),
        Key::Tab => Some(KeyCode::Tab),
        Key::Command(c) => Some(KeyCode::Char(*c)),
        Key::Function(n) => Some(KeyCode::F(*n)),
        Key::ContextMenu => Some(KeyCode::Menu),
        Key::Modifier { .. } => None,
        // `Key` is `#[non_exhaustive]`: a variant Rune adds later has no terminal binding
        // until this table is extended, and the totality test below lists the pinned set.
        _ => None,
    }
}

/// The crossterm key a Rune event stands for, when the event is a plain key press.
///
/// A chord (⌘S, ⌃N) is not a terminal binding: the TUI never sees modifiers, so routing one
/// through `handle_stories_key` would invent a binding. Shift is not a chord — `G` and `?`
/// arrive as `Text` with `mods.shift` set, and the case is already in the text.
pub fn parity_key(ev: &KeyEvent) -> Option<KeyCode> {
    let m = ev.mods;
    if m.cmd || m.ctrl || m.alt || m.logo {
        return None;
    }
    key_code(&ev.key)
}

/// The feed action a Rune event means, through the terminal app's own table.
pub fn story_action(ev: &KeyEvent) -> StoryAction {
    match parity_key(ev) {
        Some(code) => handle_stories_key(code),
        None => StoryAction::None,
    }
}

/// What a key means while a story is open.
///
/// The terminal app splits these across `Reader::key` (`src/reader/interaction.rs:7`) and
/// `reader_actions::handle` (`src/reader_actions.rs:60`); this is the union, minus the
/// bindings the window has no counterpart for (`o`/`O` open a browser tab, `J`/`K` scroll the
/// summary — the card is not a viewport, `j`/`k`/`g`/`G` scroll the source viewer — the panes
/// scroll themselves).
///
/// `h` and `l` are the one place the two tables disagree. README.md's discussion table has
/// `h` as back and `l` as expand/collapse; DESIGN.md's keyboard table has `h`/`l` as
/// fold/unfold, which is what a tree needs and what the task asked for. `←`, `Esc` and `q`
/// still leave, so nothing is lost, and `c` folds as README.md says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoryKey {
    /// `Esc`, `q`, `Left`: leave the source viewer, then focus mode, then the story.
    Back,
    /// `Tab`: select the next citation.
    NextCitation,
    /// `Shift+Tab`: select the previous citation.
    PrevCitation,
    /// `Enter`: read the selected citation, else expand or re-root the focused comment.
    Enter,
    /// `S`: show or hide the summary card.
    ToggleSummary,
    /// `r`: refetch the discussion.
    Refresh,
    /// `M`: summary settings. The dialog is WS-E; the window only notes the key.
    Settings,
    /// `L`: the link chooser for the post and the focused comment.
    Links,
    /// `s`: save or unsave the open story.
    ToggleSaved,
    /// `j`, `Down`: the next visible comment.
    NextComment,
    /// `k`, `Up`: the previous visible comment.
    PrevComment,
    /// `]`: the next sibling thread.
    NextSibling,
    /// `[`: the previous sibling thread.
    PrevSibling,
    /// `u`: the parent comment.
    Parent,
    /// `h`, `c`: fold the focused comment's replies.
    Fold,
    /// `l`, `Right`: unfold the focused comment, or re-root at it when it is at the cap.
    Unfold,
    /// `g`: the first visible comment.
    FirstComment,
    /// `G`: the last visible comment.
    LastComment,
    /// `PageDown`: the next screen of the discussion.
    NextScreen,
    /// `PageUp`: the previous screen of the discussion.
    PrevScreen,
    /// `f`: toggle focus mode.
    Focus,
    /// `Backspace`: pop one segment off the ancestor trail.
    PopTrail,
    /// A key the story view does not bind.
    None,
}

/// The story action a Rune event means.
///
/// `Tab` and `Shift+Tab` are one Rune key: Rune has no `BackTab`, so the shift modifier is
/// what distinguishes them. Every other binding is a plain key, and a chord is not one.
pub fn story_key(ev: &KeyEvent) -> StoryKey {
    let m = ev.mods;
    if m.cmd || m.ctrl || m.alt || m.logo {
        return StoryKey::None;
    }
    match &ev.key {
        Key::Escape => StoryKey::Back,
        Key::Left => StoryKey::Back,
        Key::Right => StoryKey::Unfold,
        Key::Up => StoryKey::PrevComment,
        Key::Down => StoryKey::NextComment,
        Key::PageUp => StoryKey::PrevScreen,
        Key::PageDown => StoryKey::NextScreen,
        Key::Backspace => StoryKey::PopTrail,
        Key::Tab if m.shift => StoryKey::PrevCitation,
        Key::Tab => StoryKey::NextCitation,
        Key::Enter => StoryKey::Enter,
        Key::Text(text) => match single_char(text) {
            Some('q') => StoryKey::Back,
            Some('j') => StoryKey::NextComment,
            Some('k') => StoryKey::PrevComment,
            Some(']') => StoryKey::NextSibling,
            Some('[') => StoryKey::PrevSibling,
            Some('u') => StoryKey::Parent,
            Some('h') | Some('c') => StoryKey::Fold,
            Some('l') => StoryKey::Unfold,
            Some('g') => StoryKey::FirstComment,
            Some('G') => StoryKey::LastComment,
            Some('f') => StoryKey::Focus,
            Some('s') => StoryKey::ToggleSaved,
            Some('S') => StoryKey::ToggleSummary,
            Some('r') => StoryKey::Refresh,
            Some('M') => StoryKey::Settings,
            Some('L') => StoryKey::Links,
            _ => StoryKey::None,
        },
        _ => StoryKey::None,
    }
}

fn single_char(text: &str) -> Option<char> {
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::Mods;

    /// Every `Key` variant at the pinned rev, with the crossterm key it must map to. A variant
    /// Rune adds later is not in this list, so the table's `_` arm is the only place it can
    /// land — which is why the list is spelled out rather than sampled.
    fn pinned() -> Vec<(Key, Option<KeyCode>)> {
        vec![
            (Key::Text("j".into()), Some(KeyCode::Char('j'))),
            (Key::Text("G".into()), Some(KeyCode::Char('G'))),
            (Key::Text(" ".into()), Some(KeyCode::Char(' '))),
            (Key::Text("ab".into()), None),
            (Key::Text(String::new()), None),
            (Key::Enter, Some(KeyCode::Enter)),
            (Key::Backspace, Some(KeyCode::Backspace)),
            (Key::Delete, Some(KeyCode::Delete)),
            (Key::Left, Some(KeyCode::Left)),
            (Key::Right, Some(KeyCode::Right)),
            (Key::Up, Some(KeyCode::Up)),
            (Key::Down, Some(KeyCode::Down)),
            (Key::Home, Some(KeyCode::Home)),
            (Key::End, Some(KeyCode::End)),
            (Key::PageUp, Some(KeyCode::PageUp)),
            (Key::PageDown, Some(KeyCode::PageDown)),
            (Key::Insert, Some(KeyCode::Insert)),
            (Key::PrintScreen, Some(KeyCode::PrintScreen)),
            (Key::ScrollLock, Some(KeyCode::ScrollLock)),
            (Key::Pause, Some(KeyCode::Pause)),
            (Key::Escape, Some(KeyCode::Esc)),
            (Key::Tab, Some(KeyCode::Tab)),
            (Key::Command('s'), Some(KeyCode::Char('s'))),
            (Key::Function(5), Some(KeyCode::F(5))),
            (Key::ContextMenu, Some(KeyCode::Menu)),
            (
                Key::Modifier {
                    modifier: rune::Modifier::ShiftLeft,
                    pressed: true,
                },
                None,
            ),
        ]
    }

    #[test]
    fn every_pinned_key_variant_has_a_defined_mapping() {
        for (key, expected) in pinned() {
            assert_eq!(key_code(&key), expected, "{key:?}");
        }
    }

    #[test]
    fn parity_covers_every_binding_in_the_terminal_table() {
        // The bindings `handle_stories_key` defines, as Rune keys. If the terminal table gains
        // a binding, this list is where the window has to say what it means.
        let bindings = [
            (Key::Text("j".into()), StoryAction::NextStory),
            (Key::Down, StoryAction::NextStory),
            (Key::Text("k".into()), StoryAction::PrevStory),
            (Key::Up, StoryAction::PrevStory),
            (Key::Text("g".into()), StoryAction::FirstStory),
            (Key::Text("G".into()), StoryAction::LastStory),
            (Key::Text("n".into()), StoryAction::NextPage),
            (Key::Right, StoryAction::NextPage),
            (Key::Text(" ".into()), StoryAction::NextPage),
            (Key::PageDown, StoryAction::NextPage),
            (Key::Text("p".into()), StoryAction::PrevPage),
            (Key::Left, StoryAction::PrevPage),
            (Key::PageUp, StoryAction::PrevPage),
            (
                Key::Text("1".into()),
                StoryAction::SetType(hn_lib::app::StoryType::Top),
            ),
            (
                Key::Text("2".into()),
                StoryAction::SetType(hn_lib::app::StoryType::New),
            ),
            (
                Key::Text("3".into()),
                StoryAction::SetType(hn_lib::app::StoryType::Best),
            ),
            (
                Key::Text("4".into()),
                StoryAction::SetType(hn_lib::app::StoryType::Saved),
            ),
            (Key::Enter, StoryAction::OpenUrl),
            (Key::Text("o".into()), StoryAction::OpenUrl),
            (Key::Text("O".into()), StoryAction::OpenUrl),
            (Key::Text("c".into()), StoryAction::ViewComments),
            (Key::Text("r".into()), StoryAction::Refresh),
            (Key::Text("s".into()), StoryAction::ToggleBookmark),
            (Key::Text("?".into()), StoryAction::ToggleHelp),
            (Key::Text("q".into()), StoryAction::Quit),
        ];
        for (key, expected) in bindings {
            let ev = KeyEvent {
                key: key.clone(),
                mods: Mods::default(),
            };
            assert_eq!(story_action(&ev), expected, "{key:?}");
        }
    }

    #[test]
    fn case_is_preserved_and_chords_are_not_bindings() {
        let plain = |key: Key| KeyEvent {
            key,
            mods: Mods::default(),
        };
        assert_eq!(
            story_action(&plain(Key::Text("G".into()))),
            StoryAction::LastStory
        );
        assert_eq!(
            story_action(&plain(Key::Text("g".into()))),
            StoryAction::FirstStory
        );
        // ⌘S is not `s`: the terminal app never sees a modifier, so the window must not
        // invent a save binding for it.
        let chord = KeyEvent {
            key: Key::Command('s'),
            mods: Mods {
                cmd: true,
                logo: true,
                ..Mods::default()
            },
        };
        assert_eq!(story_action(&chord), StoryAction::None);
        // Shift alone is not a chord.
        let shifted = KeyEvent {
            key: Key::Text("G".into()),
            mods: Mods {
                shift: true,
                ..Mods::default()
            },
        };
        assert_eq!(story_action(&shifted), StoryAction::LastStory);
        // Esc stays inert at the feed, as in the terminal app.
        assert_eq!(story_action(&plain(Key::Escape)), StoryAction::None);
    }

    #[test]
    fn story_keys_match_the_reader_bindings() {
        // `src/reader/interaction.rs:7` and `src/reader_actions.rs:60`, as Rune keys, plus
        // DESIGN.md's thread table (`h`/`l`, `f`, `⌫`, `Enter` on `continue thread`).
        let plain = |key: Key| KeyEvent {
            key,
            mods: Mods::default(),
        };
        let bindings = [
            (Key::Escape, StoryKey::Back),
            (Key::Text("q".into()), StoryKey::Back),
            (Key::Left, StoryKey::Back),
            (Key::Tab, StoryKey::NextCitation),
            (Key::Enter, StoryKey::Enter),
            (Key::Text("S".into()), StoryKey::ToggleSummary),
            (Key::Text("r".into()), StoryKey::Refresh),
            (Key::Text("M".into()), StoryKey::Settings),
            (Key::Text("L".into()), StoryKey::Links),
            (Key::Text("s".into()), StoryKey::ToggleSaved),
            (Key::Text("j".into()), StoryKey::NextComment),
            (Key::Down, StoryKey::NextComment),
            (Key::Text("k".into()), StoryKey::PrevComment),
            (Key::Up, StoryKey::PrevComment),
            (Key::Text("]".into()), StoryKey::NextSibling),
            (Key::Text("[".into()), StoryKey::PrevSibling),
            (Key::Text("u".into()), StoryKey::Parent),
            (Key::Text("h".into()), StoryKey::Fold),
            (Key::Text("c".into()), StoryKey::Fold),
            (Key::Text("l".into()), StoryKey::Unfold),
            (Key::Right, StoryKey::Unfold),
            (Key::Text("g".into()), StoryKey::FirstComment),
            (Key::Text("G".into()), StoryKey::LastComment),
            (Key::PageDown, StoryKey::NextScreen),
            (Key::PageUp, StoryKey::PrevScreen),
            (Key::Text("f".into()), StoryKey::Focus),
            (Key::Backspace, StoryKey::PopTrail),
        ];
        for (key, expected) in bindings {
            assert_eq!(story_key(&plain(key.clone())), expected, "{key:?}");
        }
        // Rune has no `BackTab`: Shift+Tab is `Tab` with the shift modifier.
        let back_tab = KeyEvent {
            key: Key::Tab,
            mods: Mods {
                shift: true,
                ..Mods::default()
            },
        };
        assert_eq!(story_key(&back_tab), StoryKey::PrevCitation);
        // The feed's page and tab keys are not story bindings: the discussion pages with
        // Page Up/Down, and `n`/`p`/`1`-`4` belong to the feed.
        for key in ["n", "p", "?", "1", "2", "3", "4", "o", "O"] {
            assert_eq!(
                story_key(&plain(Key::Text(key.into()))),
                StoryKey::None,
                "{key}"
            );
        }
        // A chord is not a binding, and neither is a bare modifier.
        let chord = KeyEvent {
            key: Key::Text("S".into()),
            mods: Mods {
                cmd: true,
                logo: true,
                ..Mods::default()
            },
        };
        assert_eq!(story_key(&chord), StoryKey::None);
        assert_eq!(
            story_key(&plain(Key::Modifier {
                modifier: rune::Modifier::ShiftLeft,
                pressed: true,
            })),
            StoryKey::None
        );
    }

    #[test]
    fn esc_is_back_in_both_tables() {
        // The story view's Esc and the terminal app's Esc agree, so leaving the split and
        // leaving the reader are the same gesture.
        let ev = KeyEvent {
            key: Key::Escape,
            mods: Mods::default(),
        };
        assert_eq!(story_key(&ev), StoryKey::Back);
        assert!(matches!(
            hn_lib::event::handle_comments_key(KeyCode::Esc),
            hn_lib::event::CommentAction::Back
        ));
    }
}
