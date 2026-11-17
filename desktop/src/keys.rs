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
use hn_lib::app::StoryType;
use hn_lib::event::{handle_stories_key, StoryAction};
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

/// The feed action through the terminal table, except desktop column arrows.
pub fn story_action(ev: &KeyEvent) -> StoryAction {
    match parity_key(ev) {
        Some(KeyCode::Left | KeyCode::Right) => StoryAction::None,
        Some(code) => handle_stories_key(code),
        None => StoryAction::None,
    }
}

/// `O` (capital, no modifiers): open the real page in the system browser.
///
/// The TUI's own table maps `Enter`/`o`/`O` all to `StoryAction::OpenUrl`
/// (`src/event.rs:27`) and distinguishes `O` afterwards (`src/reader_actions.rs:42,74`);
/// the window does the same: this is checked before [`story_action`], so `O` never
/// opens the extracted article split.
pub fn is_open_external(ev: &KeyEvent) -> bool {
    let m = ev.mods;
    if m.cmd || m.ctrl || m.alt || m.logo {
        return false;
    }
    matches!(&ev.key, Key::Text(t) if t == "O")
}

/// What a key means while a story is open.
///
/// The terminal app splits these across `Reader::key` (`src/reader/interaction.rs:7`) and
/// `reader_actions::handle` (`src/reader_actions.rs:60`); the desktop also scrolls the summary
/// viewport with `J`/`K`; the source viewer uses the same comment navigation as discussion.
///
/// `h` and `l` are the one place the two tables disagree. README.md's discussion table has
/// `h` as back and `l` as expand/collapse; DESIGN.md's keyboard table has `h`/`l` as
/// fold/unfold, which is what a tree needs and what the task asked for. `←`, `Esc` and `q`
/// still leave, so nothing is lost, and `c` folds as README.md says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoryKey {
    /// `Esc`, `q`, `Left`: leave the source viewer, then the story.
    Back,
    /// `Tab`: select the next citation.
    NextCitation,
    /// `Shift+Tab`: select the previous citation.
    PrevCitation,
    /// `Enter` / `o`: read the selected citation, else expand or re-root the focused comment.
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
    /// `h` folds this comment; `c` also collapses its nearest open ancestor.
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
    /// `J` / `K`: scroll the summary.
    NextSummaryScreen,
    PrevSummaryScreen,
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
            Some('o') => StoryKey::Enter,
            Some('q') => StoryKey::Back,
            Some('j') => StoryKey::NextComment,
            Some('J') => StoryKey::NextSummaryScreen,
            Some('k') => StoryKey::PrevComment,
            Some('K') => StoryKey::PrevSummaryScreen,
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
            Some('r' | 'R') => StoryKey::Refresh,
            Some('M') => StoryKey::Settings,
            Some('L') => StoryKey::Links,
            _ => StoryKey::None,
        },
        _ => StoryKey::None,
    }
}

/// A window-level binding: the keys the shell owns rather than the feed or the story.
///
/// These are chords, so they never reach the terminal app's tables — `parity_key` refuses a
/// modifier, and `story_key` refuses one too. DESIGN.md's window table is the source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowKey {
    /// `⌘,`: the summary settings dialog.
    Settings,
    /// `⌘?`: the keyboard sheet.
    Help,
    /// `⌘\`: show or hide the inspector.
    ToggleSplit,
    /// A key the window does not bind.
    None,
}

/// The window action a Rune event means.
///
/// `⌘?` is `⌘⇧/`, and a shifted shortcut arrives as the character Shift makes — `?` on a US
/// layout, `/` on a German one — so both spellings are accepted while Shift is held.
pub fn window_key(ev: &KeyEvent) -> WindowKey {
    let m = ev.mods;
    if !(m.cmd || m.logo) || m.ctrl || m.alt {
        return WindowKey::None;
    }
    let Key::Command(c) = &ev.key else {
        return WindowKey::None;
    };
    match c {
        ',' => WindowKey::Settings,
        '?' => WindowKey::Help,
        '/' if m.shift => WindowKey::Help,
        '\\' => WindowKey::ToggleSplit,
        _ => WindowKey::None,
    }
}

/// What a help row does, so the sheet can be checked against the routing instead of trusted.
#[derive(Clone, Debug, PartialEq)]
pub enum HelpAction {
    Feed(StoryAction),
    Story(StoryKey),
    Window(WindowKey),
    /// `O` (capital): open the real page in the system browser. Not a `StoryAction` —
    /// the TUI's table maps `O` to `OpenUrl` like `Enter`/`o`, and the window
    /// intercepts it before that table (`reader_actions.rs:42,74` does the same).
    External,
    ColumnLeft,
    ColumnRight,
    /// The sheet's own dismissal. `Esc` is the runtime's (the dialog's `on_close`), and `?` is
    /// the feed's `ToggleHelp`, which the sheet reads as "close me".
    Close,
}

/// One row of the help sheet: the keycaps it draws, what it does, and the action it stands for.
pub struct HelpRow {
    pub keys: &'static [&'static str],
    pub label: &'static str,
    pub action: HelpAction,
}

/// One column section of the help sheet.
pub struct HelpGroup {
    pub title: &'static str,
    /// Which of the sheet's two columns the group draws in. The split is by height, not by
    /// halving the list: Feed and Summary are 19 rows together, Discussion and Window 23.
    pub column: usize,
    pub rows: &'static [HelpRow],
}

/// The help sheet's contents, in DESIGN.md's four groups.
///
/// This is the key map, not a copy of it: every row's keys are run back through [`story_action`],
/// [`story_key`] and [`window_key`] by the tests below, and every action those tables can return
/// must have a row. A binding added to the routing without a row here fails the build's tests.
pub const HELP: &[HelpGroup] = &[
    HelpGroup {
        title: "Feed",
        column: 0,
        rows: &[
            HelpRow {
                keys: &["j", "\u{2193}"],
                label: "Next story",
                action: HelpAction::Feed(StoryAction::NextStory),
            },
            HelpRow {
                keys: &["k", "\u{2191}"],
                label: "Previous story",
                action: HelpAction::Feed(StoryAction::PrevStory),
            },
            HelpRow {
                keys: &["\u{2190}"],
                label: "Left column / previous page",
                action: HelpAction::ColumnLeft,
            },
            HelpRow {
                keys: &["\u{2192}"],
                label: "Right column / next page",
                action: HelpAction::ColumnRight,
            },
            HelpRow {
                keys: &["g"],
                label: "First story",
                action: HelpAction::Feed(StoryAction::FirstStory),
            },
            HelpRow {
                keys: &["G"],
                label: "Last story",
                action: HelpAction::Feed(StoryAction::LastStory),
            },
            HelpRow {
                keys: &["n", "Space", "Page Down"],
                label: "Next page",
                action: HelpAction::Feed(StoryAction::NextPage),
            },
            HelpRow {
                keys: &["p", "Page Up"],
                label: "Previous page",
                action: HelpAction::Feed(StoryAction::PrevPage),
            },
            HelpRow {
                keys: &["1"],
                label: "Top",
                action: HelpAction::Feed(StoryAction::SetType(StoryType::Top)),
            },
            HelpRow {
                keys: &["2"],
                label: "New",
                action: HelpAction::Feed(StoryAction::SetType(StoryType::New)),
            },
            HelpRow {
                keys: &["3"],
                label: "Best",
                action: HelpAction::Feed(StoryAction::SetType(StoryType::Best)),
            },
            HelpRow {
                keys: &["4"],
                label: "Saved",
                action: HelpAction::Feed(StoryAction::SetType(StoryType::Saved)),
            },
            HelpRow {
                keys: &["Enter", "o"],
                label: "Open the story",
                action: HelpAction::Feed(StoryAction::OpenUrl),
            },
            HelpRow {
                keys: &["⇧O"],
                label: "Open in browser",
                action: HelpAction::External,
            },
            HelpRow {
                keys: &["c"],
                label: "Open the discussion",
                action: HelpAction::Feed(StoryAction::ViewComments),
            },
            HelpRow {
                keys: &["r"],
                label: "Refresh the feed",
                action: HelpAction::Feed(StoryAction::Refresh),
            },
            HelpRow {
                keys: &["s"],
                label: "Save or unsave",
                action: HelpAction::Feed(StoryAction::ToggleBookmark),
            },
            HelpRow {
                keys: &["q"],
                label: "Quit",
                action: HelpAction::Feed(StoryAction::Quit),
            },
        ],
    },
    HelpGroup {
        title: "Discussion",
        column: 1,
        rows: &[
            HelpRow {
                keys: &["j", "\u{2193}"],
                label: "Next comment",
                action: HelpAction::Story(StoryKey::NextComment),
            },
            HelpRow {
                keys: &["k", "\u{2191}"],
                label: "Previous comment",
                action: HelpAction::Story(StoryKey::PrevComment),
            },
            HelpRow {
                keys: &["g"],
                label: "First comment",
                action: HelpAction::Story(StoryKey::FirstComment),
            },
            HelpRow {
                keys: &["⇧G"],
                label: "Last comment",
                action: HelpAction::Story(StoryKey::LastComment),
            },
            HelpRow {
                keys: &["]"],
                label: "Next thread",
                action: HelpAction::Story(StoryKey::NextSibling),
            },
            HelpRow {
                keys: &["["],
                label: "Previous thread",
                action: HelpAction::Story(StoryKey::PrevSibling),
            },
            HelpRow {
                keys: &["u"],
                label: "Parent comment",
                action: HelpAction::Story(StoryKey::Parent),
            },
            HelpRow {
                keys: &["h", "c"],
                label: "Fold replies (c: parent)",
                action: HelpAction::Story(StoryKey::Fold),
            },
            HelpRow {
                keys: &["l", "\u{2192}"],
                label: "Unfold, or re-root at the cap",
                action: HelpAction::Story(StoryKey::Unfold),
            },
            HelpRow {
                keys: &["f"],
                label: "Focus mode",
                action: HelpAction::Story(StoryKey::Focus),
            },
            HelpRow {
                keys: &["r"],
                label: "Refresh comments",
                action: HelpAction::Story(StoryKey::Refresh),
            },
            HelpRow {
                keys: &["\u{232b}"],
                label: "Up one ancestor",
                action: HelpAction::Story(StoryKey::PopTrail),
            },
            HelpRow {
                keys: &["Page Down"],
                label: "Next screen",
                action: HelpAction::Story(StoryKey::NextScreen),
            },
            HelpRow {
                keys: &["Page Up"],
                label: "Previous screen",
                action: HelpAction::Story(StoryKey::PrevScreen),
            },
            HelpRow {
                keys: &["Enter", "o"],
                label: "Toggle replies, or read citation",
                action: HelpAction::Story(StoryKey::Enter),
            },
            HelpRow {
                keys: &["Tab"],
                label: "Next citation or source",
                action: HelpAction::Story(StoryKey::NextCitation),
            },
            HelpRow {
                keys: &["\u{21e7}Tab"],
                label: "Previous citation or source",
                action: HelpAction::Story(StoryKey::PrevCitation),
            },
            HelpRow {
                keys: &["⇧L"],
                label: "Choose a link",
                action: HelpAction::Story(StoryKey::Links),
            },
            HelpRow {
                keys: &["s"],
                label: "Save or unsave",
                action: HelpAction::Story(StoryKey::ToggleSaved),
            },
            HelpRow {
                keys: &["⇧O"],
                label: "Open in browser",
                action: HelpAction::External,
            },
            HelpRow {
                keys: &["Esc", "q", "\u{2190}"],
                label: "Back to discussion or feed",
                action: HelpAction::Story(StoryKey::Back),
            },
        ],
    },
    HelpGroup {
        title: "Summary",
        column: 0,
        rows: &[
            HelpRow {
                keys: &["⇧S"],
                label: "Show or hide the summary",
                action: HelpAction::Story(StoryKey::ToggleSummary),
            },
            HelpRow {
                keys: &["⇧J"],
                label: "Scroll summary down",
                action: HelpAction::Story(StoryKey::NextSummaryScreen),
            },
            HelpRow {
                keys: &["⇧K"],
                label: "Scroll summary up",
                action: HelpAction::Story(StoryKey::PrevSummaryScreen),
            },
            HelpRow {
                keys: &["⇧M"],
                label: "Summary settings",
                action: HelpAction::Story(StoryKey::Settings),
            },
        ],
    },
    HelpGroup {
        title: "Window",
        column: 1,
        rows: &[
            HelpRow {
                keys: &["\u{2318},"],
                label: "Summary settings",
                action: HelpAction::Window(WindowKey::Settings),
            },
            HelpRow {
                keys: &["?", "\u{2318}?"],
                label: "Keyboard shortcuts",
                action: HelpAction::Window(WindowKey::Help),
            },
            HelpRow {
                keys: &["\u{2318}\\"],
                label: "Show or hide the inspector",
                action: HelpAction::Window(WindowKey::ToggleSplit),
            },
            HelpRow {
                keys: &["Esc", "?"],
                label: "Close this sheet",
                action: HelpAction::Close,
            },
        ],
    },
];

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
            (Key::Right, StoryAction::None),
            (Key::Text(" ".into()), StoryAction::NextPage),
            (Key::PageDown, StoryAction::NextPage),
            (Key::Text("p".into()), StoryAction::PrevPage),
            (Key::Left, StoryAction::None),
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
    fn capital_o_is_open_external_not_open_story() {
        let plain = |key: Key| KeyEvent {
            key,
            mods: Mods::default(),
        };
        // `O` is intercepted before the TUI table (which maps it to OpenUrl like Enter/o).
        assert!(is_open_external(&plain(Key::Text("O".into()))));
        assert!(!is_open_external(&plain(Key::Text("o".into()))));
        assert!(!is_open_external(&plain(Key::Enter)));
        let cmd_o = KeyEvent {
            key: Key::Text("O".into()),
            mods: Mods {
                cmd: true,
                ..Mods::default()
            },
        };
        assert!(!is_open_external(&cmd_o));
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
            (Key::Text("o".into()), StoryKey::Enter),
            (Key::Text("S".into()), StoryKey::ToggleSummary),
            (Key::Text("r".into()), StoryKey::Refresh),
            (Key::Text("R".into()), StoryKey::Refresh),
            (Key::Text("M".into()), StoryKey::Settings),
            (Key::Text("L".into()), StoryKey::Links),
            (Key::Text("s".into()), StoryKey::ToggleSaved),
            (Key::Text("j".into()), StoryKey::NextComment),
            (Key::Text("J".into()), StoryKey::NextSummaryScreen),
            (Key::Down, StoryKey::NextComment),
            (Key::Text("k".into()), StoryKey::PrevComment),
            (Key::Text("K".into()), StoryKey::PrevSummaryScreen),
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
        for key in ["n", "p", "?", "1", "2", "3", "4", "O"] {
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

    /// The Rune event a help row's keycap stands for. The sheet draws keycaps, so the test has
    /// to read them back: a row whose cap does not parse is a row the sheet cannot honour.
    fn event(cap: &str) -> KeyEvent {
        let plain = |key: Key| KeyEvent {
            key,
            mods: Mods::default(),
        };
        let shifted = |key: Key| KeyEvent {
            key,
            mods: Mods {
                shift: true,
                ..Mods::default()
            },
        };
        let chord = |c: char, shift: bool| KeyEvent {
            key: Key::Command(c),
            mods: Mods {
                cmd: true,
                logo: true,
                shift,
                ..Mods::default()
            },
        };
        match cap {
            "Esc" => plain(Key::Escape),
            "Enter" => plain(Key::Enter),
            "Tab" => plain(Key::Tab),
            "\u{21e7}Tab" => shifted(Key::Tab),
            "⇧O" => shifted(Key::Text("O".into())),
            "⇧G" => shifted(Key::Text("G".into())),
            "⇧L" => shifted(Key::Text("L".into())),
            "⇧M" => shifted(Key::Text("M".into())),
            "⇧S" => shifted(Key::Text("S".into())),
            "⇧J" => shifted(Key::Text("J".into())),
            "⇧K" => shifted(Key::Text("K".into())),
            "\u{232b}" => plain(Key::Backspace),
            "Space" => plain(Key::Text(" ".into())),
            "Page Down" => plain(Key::PageDown),
            "Page Up" => plain(Key::PageUp),
            "\u{2193}" => plain(Key::Down),
            "\u{2191}" => plain(Key::Up),
            "\u{2190}" => plain(Key::Left),
            "\u{2192}" => plain(Key::Right),
            "\u{2318}," => chord(',', false),
            "\u{2318}?" => chord('?', true),
            "\u{2318}\\" => chord('\\', false),
            _ => {
                let mut chars = cap.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => plain(Key::Text(c.to_string())),
                    _ => panic!("the help sheet has no keycap {cap:?}"),
                }
            }
        }
    }

    /// The action a key means in the context a group stands for: the shell's chords, the
    /// story's table while a story is open, or the feed's. `j` is both a feed key and a story
    /// key, which is why the sheet groups by context rather than by key.
    fn routed(ev: &KeyEvent, context: &HelpAction) -> HelpAction {
        match context {
            HelpAction::Feed(_) => HelpAction::Feed(story_action(ev)),
            HelpAction::Story(_) => HelpAction::Story(story_key(ev)),
            HelpAction::Window(WindowKey::Help) if story_action(ev) == StoryAction::ToggleHelp => HelpAction::Window(WindowKey::Help),
            HelpAction::Window(_) => HelpAction::Window(window_key(ev)),
            HelpAction::External => {
                if is_open_external(ev) {
                    HelpAction::External
                } else {
                    HelpAction::Close
                }
            }
            HelpAction::ColumnLeft | HelpAction::ColumnRight => match parity_key(ev) {
                Some(KeyCode::Left) => HelpAction::ColumnLeft,
                Some(KeyCode::Right) => HelpAction::ColumnRight,
                _ => HelpAction::Feed(StoryAction::None),
            },
            HelpAction::Close => HelpAction::Close,
        }
    }

    #[test]
    fn every_help_row_is_a_binding_the_window_actually_has() {
        for group in HELP {
            for row in group.rows {
                // The group is the context: a Feed row must be a feed binding, and so on.
                let context = match (group.title, &row.action) {
                    ("Feed", HelpAction::Feed(_))
                    | ("Discussion", HelpAction::Story(_))
                    | ("Summary", HelpAction::Story(_))
                    | ("Window", HelpAction::Window(_))
                    | ("Feed" | "Discussion", HelpAction::External)
                    | ("Feed", HelpAction::ColumnLeft | HelpAction::ColumnRight)
                    | ("Window", HelpAction::Close) => row.action.clone(),
                    (title, action) => panic!("{title} group holds {action:?}"),
                };
                for cap in row.keys {
                    let ev = event(cap);
                    if row.action == HelpAction::Close {
                        // The sheet's own dismissal is not a routing table's: `Esc` is the
                        // runtime's modal dismiss and `?` is the feed's `ToggleHelp`, which the
                        // sheet reads as "close me". Nothing else may claim to close it.
                        assert!(
                            matches!(cap, &"Esc" | &"?"),
                            "{}: {cap:?} does not close the sheet",
                            group.title
                        );
                        continue;
                    }
                    assert_eq!(
                        routed(&ev, &context),
                        row.action,
                        "{}: {cap:?} does not do {:?}",
                        group.title,
                        row.label
                    );
                }
            }
        }
    }

    #[test]
    fn every_binding_the_window_has_is_in_the_help_sheet() {
        // Spelled out, not sampled: a variant added to either enum is a compile error here
        // until the sheet says what it does.
        let story_keys = [
            StoryKey::Back,
            StoryKey::NextCitation,
            StoryKey::PrevCitation,
            StoryKey::Enter,
            StoryKey::ToggleSummary,
            StoryKey::Refresh,
            StoryKey::Settings,
            StoryKey::Links,
            StoryKey::ToggleSaved,
            StoryKey::NextComment,
            StoryKey::PrevComment,
            StoryKey::NextSibling,
            StoryKey::PrevSibling,
            StoryKey::Parent,
            StoryKey::Fold,
            StoryKey::Unfold,
            StoryKey::FirstComment,
            StoryKey::LastComment,
            StoryKey::NextScreen,
            StoryKey::PrevScreen,
            StoryKey::NextSummaryScreen,
            StoryKey::PrevSummaryScreen,
            StoryKey::Focus,
            StoryKey::PopTrail,
        ];
        let feed_actions = [
            StoryAction::NextStory,
            StoryAction::PrevStory,
            StoryAction::FirstStory,
            StoryAction::LastStory,
            StoryAction::NextPage,
            StoryAction::PrevPage,
            StoryAction::SetType(StoryType::Top),
            StoryAction::SetType(StoryType::New),
            StoryAction::SetType(StoryType::Best),
            StoryAction::SetType(StoryType::Saved),
            StoryAction::OpenUrl,
            StoryAction::ViewComments,
            StoryAction::Refresh,
            StoryAction::ToggleBookmark,
            StoryAction::ToggleHelp,
            StoryAction::Quit,
        ];
        let window_keys = [WindowKey::Settings, WindowKey::Help, WindowKey::ToggleSplit];
        let rows: Vec<HelpAction> = HELP
            .iter()
            .flat_map(|group| group.rows.iter().map(|row| row.action.clone()))
            .collect();
        for action in story_keys.map(HelpAction::Story) {
            assert!(rows.contains(&action), "no help row for {action:?}");
        }
        for action in feed_actions.map(|action| if action == StoryAction::ToggleHelp {
            HelpAction::Window(WindowKey::Help)
        } else { HelpAction::Feed(action) }) {
            assert!(rows.contains(&action), "no help row for {action:?}");
        }
        for action in window_keys.map(HelpAction::Window) {
            assert!(rows.contains(&action), "no help row for {action:?}");
        }
    }

    #[test]
    fn the_window_chords_are_the_ones_design_md_names() {
        let chord = |c: char, shift: bool| KeyEvent {
            key: Key::Command(c),
            mods: Mods {
                cmd: true,
                logo: true,
                shift,
                ..Mods::default()
            },
        };
        assert_eq!(window_key(&chord(',', false)), WindowKey::Settings);
        assert_eq!(window_key(&chord('?', true)), WindowKey::Help);
        // A shifted shortcut arrives as the character Shift makes, which moves between
        // layouts: `⌘⇧/` is `?` on a US layout and `/` on a German one.
        assert_eq!(window_key(&chord('/', true)), WindowKey::Help);
        assert_eq!(window_key(&chord('\\', false)), WindowKey::ToggleSplit);
        // A bare `/` is not the help chord, and neither is a chord with another modifier.
        assert_eq!(window_key(&chord('/', false)), WindowKey::None);
        let ctrl = KeyEvent {
            key: Key::Command(','),
            mods: Mods {
                cmd: true,
                ctrl: true,
                ..Mods::default()
            },
        };
        assert_eq!(window_key(&ctrl), WindowKey::None);
        // The feed's own keys are not window chords.
        assert_eq!(
            window_key(&KeyEvent {
                key: Key::Text("j".into()),
                mods: Mods::default(),
            }),
            WindowKey::None
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
