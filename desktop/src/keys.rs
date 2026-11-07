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
/// bindings the window has no counterpart for (`o` opens a browser tab, `J`/`K` scroll the
/// summary, `j`/`k`/`g`/`G` scroll the source viewer — the panes scroll themselves).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoryKey {
    /// `Esc`, `q`, `h`, `Left`: leave the source viewer, else close the story.
    Back,
    /// `Tab`: select the next citation.
    NextCitation,
    /// `Shift+Tab`: select the previous citation.
    PrevCitation,
    /// `Enter` with a citation selected: open the source viewer on it.
    OpenSource,
    /// `S`: show or hide the summary card.
    ToggleSummary,
    /// `r`: refetch the discussion.
    Refresh,
    /// `M`: summary settings. The dialog is WS-E; the window only notes the key.
    Settings,
    /// `L`: link chooser, deferred to WS-D.
    Links,
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
        Key::Tab if m.shift => StoryKey::PrevCitation,
        Key::Tab => StoryKey::NextCitation,
        Key::Enter => StoryKey::OpenSource,
        Key::Text(text) => match single_char(text) {
            Some('q') | Some('h') => StoryKey::Back,
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
        // `src/reader/interaction.rs:7` and `src/reader_actions.rs:60`, as Rune keys.
        let plain = |key: Key| KeyEvent {
            key,
            mods: Mods::default(),
        };
        let bindings = [
            (Key::Escape, StoryKey::Back),
            (Key::Text("q".into()), StoryKey::Back),
            (Key::Text("h".into()), StoryKey::Back),
            (Key::Left, StoryKey::Back),
            (Key::Tab, StoryKey::NextCitation),
            (Key::Enter, StoryKey::OpenSource),
            (Key::Text("S".into()), StoryKey::ToggleSummary),
            (Key::Text("r".into()), StoryKey::Refresh),
            (Key::Text("M".into()), StoryKey::Settings),
            (Key::Text("L".into()), StoryKey::Links),
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
        // The feed's own bindings are not story bindings.
        for key in ["j", "k", "g", "G", "n", "p", "s", "?", "c", "1"] {
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
