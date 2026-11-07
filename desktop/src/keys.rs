//! The total Rune `Key` → crossterm `KeyCode` table, and the feed's key routing.
//!
//! The terminal app's bindings live in `hn_lib::event::handle_stories_key`, which takes a
//! crossterm `KeyCode`. The window gets Rune `Key`s, so every Rune key has to be translated
//! before parity can be claimed. [`key_code`] is that table: total over the pinned variants,
//! with `None` for the keys the terminal app has no binding for.

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
}
