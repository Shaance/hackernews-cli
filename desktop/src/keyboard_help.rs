//! Keyboard shortcut catalog for the help sheet and routing parity tests.

use hn_lib::app::StoryType;
use hn_lib::event::StoryAction;

use crate::keys::{StoryKey, WindowKey};

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
/// This is the key map, not a copy of it: every row's keys are run back through [`crate::keys::story_action`],
/// [`crate::keys::story_key`] and [`crate::keys::window_key`] by the routing parity tests, and every action those tables can return
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
                keys: &["⇧G"],
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
                keys: &["\u{2318}=", "\u{2318}+"],
                label: "Zoom in",
                action: HelpAction::Window(WindowKey::ZoomIn),
            },
            HelpRow {
                keys: &["\u{2318}-"],
                label: "Zoom out",
                action: HelpAction::Window(WindowKey::ZoomOut),
            },
            HelpRow {
                keys: &["\u{2318}0"],
                label: "Actual size",
                action: HelpAction::Window(WindowKey::ActualSize),
            },
            HelpRow {
                keys: &["Esc", "?"],
                label: "Close this sheet",
                action: HelpAction::Close,
            },
        ],
    },
];

