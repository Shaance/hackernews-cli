//! Deterministic feed, reading and dialog scenes.

use super::{App, Notice, TOAST_SECONDS};
use hn_lib::app::StoryType;
use hn_lib::reader::Config;
use crate::bookmarks::SavedView;
use crate::settings::Settings;
use crate::story::SummaryState;

pub(super) fn scene_feed(_s: &mut App) {}

pub(super) fn scene_feed_saved(s: &mut App) {
    s.toast = Some(Notice {
        message: "Saved · Press 4 for Saved".to_string(),
        until: rune::scene_clock().now + TOAST_SECONDS,
        show_dot: true,
    });
}

pub(super) fn scene_saved_view(s: &mut App) {
    s.feed.tab = StoryType::Saved;
    // The hover action is the only way to unsave from this view, so the scene shows it.
    s.hover = s.saved.ids().first().copied();
}

pub(super) fn scene_saved_corrupt(s: &mut App) {
    s.saved = SavedView::corrupt(
        std::path::PathBuf::from("~/.local/share/hn/saved.json"),
        "expected value at line 1 column 2",
    );
    s.feed.tab = StoryType::Saved;
}

pub(super) fn scene_resize(_s: &mut App) {}

/// The story split, settled: the article, the summary card and the discussion.
pub(super) fn scene_story(s: &mut App) {
    s.open_story(crate::fixtures::item(
        crate::fixtures::STORY_INDEX,
        crate::fixtures::STORY_ID,
    ));
    if let Some(story) = s.story.as_mut() {
        story.thread = hn_lib::comments::Thread::from_comments(&crate::fixtures::thread(), Some(crate::story::VISIBLE_COMMENT_LEVELS), crate::story::REPLY_BATCH_SIZE);
    }
}

/// The discussion as it opens: the tree at the story's top level, the cursor on the first
/// comment, no re-root, with focus mode on.
pub(super) fn scene_comments(s: &mut App) {
    scene_story(s);
}

/// A thread you did not start: re-rooted at the comment that hit the depth cap, the cursor on
/// its first reply. The trail carries the four ancestors above it.
pub(super) fn scene_thread(s: &mut App) {
    scene_story(s);
    if let Some(story) = &mut s.story {
        story.thread.reroot(901000);
        story.thread.select(9010000);
    }
}

/// The citation source viewer: the first citation has replies to inspect.
pub(super) fn scene_citation(s: &mut App) {
    scene_story(s);
    if let Some(story) = &mut s.story {
        story.next_citation();
        story.open_source();
    }
}

/// Generating: the article and the discussion have landed, the summary has not.
pub(super) fn scene_summary_generating(s: &mut App) {
    scene_story(s);
    if let Some(story) = &mut s.story {
        story.summary = SummaryState::Generating {
            previous: None,
        };
    }
}

/// A restored reading with a fixed clock and a newer discussion count.
pub(super) fn scene_summary_saved(s: &mut App) {
    use hn_lib::reader::Article;
    use hn_lib::reader::saved::{ArticleData, DiscussionContext, DiscussionData, Header, Record, SummaryStore};

    let mut store = SummaryStore::memory(1_700_000_000);
    let mut header = Header::new(crate::fixtures::STORY_ID, Default::default(), Default::default());
    header.saved_at = 1_700_000_000 - 5 * 3600;
    store.save_article(Record {
        header: header.clone(),
        data: ArticleData { article: Article { text: "The article describes Libghostty and its design.".into() }, partial: false },
    }, false).expect("the in-memory article summary fixture serializes");
    store.save_discussion(Record {
        header,
        data: DiscussionData {
            discussion: crate::fixtures::summary(),
            context: DiscussionContext {
                comments_at_save: 110,
                coverage: "Top-level comment excerpts".into(),
                sources: crate::fixtures::sources(),
            },
        },
    }).expect("the in-memory discussion summary fixture serializes");
    s.summary_store = Some(store);
    scene_story(s);
}

/// The summary settings dialog, open on a draft that already holds a key. The key is a fixture
/// and the field is secure, so the golden proves the mask rather than the value: the string
/// appears nowhere in the tree.
pub(super) fn scene_settings(s: &mut App) {
    let mut config = Config::default();
    config.api_key = "sk-or-v1-EXAMPLEKEYNOTREAL".to_string();
    s.settings = Some(Settings::open(&config, None, false));
}

/// Settings over an open story, including its regenerate action.
pub(super) fn scene_settings_story(s: &mut App) {
    scene_story(s);
    scene_settings(s);
}

/// The keyboard sheet, open: two columns of the key map, generated from `keyboard_help::HELP`.
pub(super) fn scene_help(s: &mut App) {
    s.help = true;
}
