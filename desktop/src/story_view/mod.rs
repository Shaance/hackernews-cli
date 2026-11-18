//! The story split's view: the article pane, the pinned summary card, the comment tree.
//!
//! Every function here is a pure function of [`Story`] plus the theme, so a scene, a scripted
//! key and a landed request all draw the same tree. The card's four states are
//! [`crate::story::SummaryState`]'s; this module only draws them.
//!
//! The discussion is DESIGN.md's "Comments: reading a thread you did not start", drawn from
//! [`crate::comments::Thread`]: a tree with one indent guide per ancestor, a sticky ancestor
//! trail above the focused comment, a depth cap that turns deeper replies into
//! `continue thread → N replies`, a focus rail on the selected comment, and a
//! folded comment's derived `N replies hidden`. The state is the thread's; this module only
//! draws it and hands presses back to it.
//!
//! The article is `rune_kit::Markdown`, so headings, lists, quotes and fenced code are the
//! kit's, including its syntax colouring. Nothing here parses Markdown.

pub mod article;
mod discussion;
pub mod reading_chrome;
mod summary;

use rune::tok;
use rune::{Color, El, Spring, SpringPair, TextStyle, Transition, col};
use rune_kit::Kit;

use crate::story::Story;

use self::discussion::{discussion_pane, source_viewer};
use self::reading_chrome::reading_transition;
use self::summary::summary_card;

/// The article's measure: 66 characters at [`rune::tok::type_::BODY`] (13 pt), 6.8 pt per character.
/// DESIGN.md's reading measure, and the reason the article column does not fill the pane.
pub const MEASURE: f32 = 448.8;

/// The article's headline. `type_::TITLE` is a row title; a headline is a step above it.
const HEADLINE: TextStyle = TextStyle::new(21.0, 600.0);

/// The fallback banner's text. The pane is the live page; this is what the reader sees when a
/// child view cannot exist (headless, non-macOS) or could not be built.
pub const FALLBACK_BANNER: &str = "live page unavailable \u{2014} showing extracted text";

/// The inspector: the summary card pinned at the top, the discussion below it, or the source
/// viewer in their place.
///
/// The card and the discussion are siblings in a column that does not scroll, so the card is
/// pinned by layout rather than by `sticky_top`: the discussion has its own viewport and moves
/// under the card, and the card never moves at all. `overlays` takes the link chooser's surface,
/// which is anchored to a chip in the discussion header.
pub fn inspector_pane(
    s: &Story,
    k: &Kit,
    t: tok::Theme,
    accent: Color,
    overlays: &mut Vec<El>,
    reduced: bool,
    summary_height: f32,
    on_scroll: [impl Fn(rune::ScrollPos) + 'static; 3],
) -> El {
    let transition = reading_transition(reduced);
    let reflow = if reduced { Spring::NONE } else { Spring::new(0.18, 0.0) };
    let [discussion_scroll, summary_scroll, source_scroll] = on_scroll;
    let source = s.source();
    let hidden = source.is_some();
    let back = if hidden || reduced { Spring::NONE } else { Spring::new(0.22, 0.0) };
    let mut inspector = col()
        .key("inspector")
        .transition(Transition::none())
        .opacity(if hidden { 0.0 } else { 1.0 })
        .offset(if hidden && !reduced { -16.0 } else { 0.0 }, 0.0)
        .opacity_motion(back)
        .offset_motion(back)
        .w_full()
        .h_full()
        .child(
            col()
                .key("inspector-content")
                .transition(Transition::none())
                .w_full()
                .h_full()
                .px(14.0)
                .pb(12.0)
                .gap(10.0)
                .child(summary_card(s, k, t, accent, reduced, summary_height, summary_scroll))
                .child(
                    discussion_pane(s, k, t, accent, overlays, reduced, discussion_scroll)
                        .transition(transition)
                        .motion(reflow)
                        .grow(1.0)
                        .min_h(0.0),
                ),
        );
    if hidden {
        // Keep the scroll viewport mounted, but exclude it from input and accessibility.
        inspector = inspector.inert().a11y_hidden();
    }
    let mut stage = col().key("inspector-stage").w_full().h_full().child(inspector);
    if let Some(source) = source {
        let enter = if reduced { Transition::none() } else {
            let mut enter = Transition::asymmetric(
                Transition::offset(16.0, 0.0).combined(Transition::opacity()), Transition::none(),
            ).springs(SpringPair::new(Spring::new(0.22, 0.0), Spring::NONE));
            enter.insertion.opacity = 0.4;
            enter
        };
        stage = stage.child(source_viewer(s, source, k, t, accent, reduced, source_scroll)
            .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).transition(enter));
    }
    stage
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::{article::host_of, discussion::{comment_row, RenderContext}, reading_chrome::story_footer};
    use crate::fixtures;

    #[test]
    fn browser_button_keeps_its_compact_accessible_target() {
        fn footer(story: &Story, cx: &mut rune::Cx<Story>) -> El {
            let dark = cx.system_dark();
            story_footer(story, &Kit::neutral(dark), tok::theme(dark), &rune::Live::new(1.0), false, false)
        }
        let story = Story::opening(fixtures::item(fixtures::STORY_INDEX, fixtures::STORY_ID), false);
        let mut rt = rune::Runtime::new(story, footer, (760.0, 100.0), rune::Env::new(true, 1.0));
        rt.frame(0.0);
        let elements = rt.elements();
        let button = elements.iter().find(|e| e.key.as_deref() == Some("article-open-original")).unwrap();
        assert_eq!(button.label.as_deref(), Some("Open in browser"));
        assert!(button.focusable);
        assert!(button.rect[2] < 130.0);
        assert!(rt.accessibility_tree().nodes.iter().any(|(_, node)|
            node.label() == Some("Open in browser") && node.keyboard_shortcut() == Some("⇧O")));
    }

    fn nested_comment(story: &Story, cx: &mut rune::Cx<Story>) -> El {
        let dark = cx.system_dark();
        comment_row(story, 9010, 1, &Kit::neutral(dark), tok::theme(dark), tok::accent(3, dark), RenderContext::Discussion, false, true)
    }

    #[test]
    fn folded_comment_keeps_body_and_places_guide_outside_text() {
        let mut story = Story::opening(fixtures::item(fixtures::STORY_INDEX, fixtures::STORY_ID), true);
        story.thread = crate::comments::Thread::from_comments(&fixtures::thread());
        let mut rt = rune::Runtime::new(story, nested_comment, (420.0, 250.0), rune::Env::new(true, 1.0));
        rt.frame(0.0);
        let guide_x = rt.elements().iter().find(|e| e.key.as_deref() == Some("guide-9010-1")).unwrap().rect[0];
        rt.update(|story, tx| {
            assert!(story.thread.select(9010));
            assert!(story.thread.fold());
            tx.instant();
        });
        rt.frame(0.0);
        let elements = rt.elements();
        let find = |key: &str| elements.iter().find(|e| e.key.as_deref() == Some(key) && !e.exiting).unwrap();
        let author = find("comment-author-9010");
        let body = find("comment-text-9010");
        let guide = find("focus-rail-9010");
        let control = find("comment-hidden-9010");
        assert_eq!(guide.rect[0], guide_x, "selection shares the existing nesting guide");
        assert!(!elements.iter().any(|e| e.key.as_deref() == Some("guide-9010-1") && !e.exiting));
        assert!((author.rect[0] - body.rect[0]).abs() < 0.5);
        assert!(guide.rect[0] + guide.rect[2] < body.rect[0]);
        assert!(control.focusable && control.rect[2] < 200.0);
        assert!(control.label.as_deref().is_some_and(|label| label.starts_with("Show ")));
    }

    #[test]
    fn unreadable_parent_keeps_replies_without_author_or_body() {
        let mut comments = fixtures::thread();
        comments.iter_mut().find(|comment| comment.id == 9010).unwrap().deleted = true;
        let mut story = Story::opening(fixtures::item(fixtures::STORY_INDEX, fixtures::STORY_ID), true);
        story.thread = crate::comments::Thread::from_comments(&comments);
        let mut rt = rune::Runtime::new(story, nested_comment, (420.0, 250.0), rune::Env::new(true, 1.0));
        rt.frame(0.0);
        let elements = rt.elements();
        assert!(elements.iter().any(|e| e.key.as_deref() == Some("comment-group-9010") && e.focusable));
        for key in ["comment-author-9010", "comment-age-9010", "comment-text-9010"] {
            assert!(!elements.iter().any(|e| e.key.as_deref() == Some(key)));
        }
    }

    #[test]
    fn pending_reply_disclosure_matches_visible_loading_content() {
        for readable in [true, false] {
            let mut parent = fixtures::thread().into_iter().find(|comment| comment.id == 9010).unwrap();
            parent.deleted = !readable;
            let mut story = Story::opening(fixtures::item(fixtures::STORY_INDEX, fixtures::STORY_ID), true);
            story.thread = crate::comments::Thread::from_comments(&[parent]);
            assert!(story.request_replies(9010, false).is_some());
            let mut rt = rune::Runtime::new(story, nested_comment, (420.0, 250.0), rune::Env::new(true, 1.0));
            rt.frame(0.0);
            let elements = rt.elements();
            let row = elements.iter().find(|e| e.key.as_deref() == Some("comment-9010")).unwrap();
            assert_eq!(row.expanded, Some(true), "the pending disclosure has visible loading content");
            assert!(elements.iter().any(|e| e.text.as_deref() == Some("Loading replies…")));
            rt.update(|story, tx| { assert!(story.thread.fold()); tx.instant(); });
            rt.frame(0.0);
            let elements = rt.elements();
            let row = elements.iter().find(|e| e.key.as_deref() == Some("comment-9010")).unwrap();
            assert_eq!(row.expanded, Some(false));
            assert!(!elements.iter().any(|e| e.text.as_deref() == Some("Loading replies…") && !e.exiting));
        }
    }

    #[test]
    fn article_metadata_names_the_host_without_www() {
        assert_eq!(host_of("https://ghostty.org/blog/libghostty"), "ghostty.org");
        assert_eq!(host_of("https://www.example.com/a"), "example.com");
        assert_eq!(host_of(""), "the article");
    }
}
