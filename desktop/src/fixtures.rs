//! The deterministic story set the headless scenes and the fake service render.
//!
//! Real front-page items captured 2026-11-07 from the Algolia HN API
//! (`/api/v1/search?tags=front_page&hitsPerPage=40`) — the same capture
//! `prototypes/rune-hn/fixtures/topstories.json` ships, extended to a full page so the
//! two-column grid has enough rows to fill. Nothing here is fetched at run time.

use hn_lib::app::{Comment, CommentState};
use hn_lib::reader::{Discussion, Point, Source};
use hn_lib::HNCLIItem;

/// `(title, author, score, comments, saved)` for one page of 30, in rank order.
const STORIES: [(&str, &str, i32, i32, bool); 30] = [
    ("Tesla to wind down Cybertruck production", "PLenz", 1057, 1913, true),
    ("Libghostty 1.0", "mfiguiere", 771, 122, true),
    (
        "A Jev that can ask Opus for help gives Opus accuracy at 4x lower cost",
        "theanomaly",
        513,
        122,
        false,
    ),
    (
        "US FAA says it will phase out floppy disks from air traffic control by 2028",
        "ChrisArchitect",
        356,
        289,
        false,
    ),
    ("Gemini Omni 1.2 Flash", "thisisauserid", 347, 234, false),
    ("Firecracker VM Escape: PwnEscape Part 2", "SerCe", 257, 110, true),
    (
        "First steps in bare-metal programming using UEFI",
        "LaSombra",
        233,
        46,
        false,
    ),
    (
        "Every Mac comes with an (undocumented) family member classifier",
        "infwhispers",
        219,
        106,
        false,
    ),
    ("Upload Your RAM", "susanthenerd", 180, 129, false),
    ("Devvm.dev: One VM for every project", "luispa", 169, 64, false),
    ("GitHub's new Git storage architecture", "DAlperin", 168, 38, true),
    (
        "RealPage rent-pricing case wraps with nearly $360M to renters",
        "raybb",
        143,
        84,
        false,
    ),
    (
        "Need for Speed Underground 2 online in the browser",
        "fluxusars",
        141,
        43,
        false,
    ),
    (
        "Restaurant Delivery Payment-Skimming Campaign",
        "fiej",
        138,
        71,
        false,
    ),
    (
        "Greece has its own Pompeii: Thera buried Akrotiri in ash 17th-16th century BCE",
        "derbOac",
        138,
        15,
        false,
    ),
    (
        "A barbarian tribe's mythic origin story is true, ancient DNA reveals",
        "benbreen",
        136,
        104,
        false,
    ),
    (
        "Chomsky and the two cultures of statistical learning (2011)",
        "andsoitis",
        135,
        104,
        false,
    ),
    (
        "Can GROUP BY still be optimized after all these years?",
        "ntur1337",
        95,
        16,
        false,
    ),
    ("Notes on San Marino", "yorwba", 90, 34, false),
    (
        "Google has acquired Crusoe's data center site in southeast Wyoming",
        "rexbee",
        86,
        33,
        false,
    ),
    ("The Components of Agent Memory", "geoffbp", 76, 40, false),
    ("The Flash C Compiler", "AlexeyBrin", 64, 19, false),
    (
        "Every Windows 11 app should kill Electron, and Discord just showed why",
        "thunderbong",
        47,
        34,
        false,
    ),
    ("PostgreSQL Internals [pdf]", "Foe", 45, 5, false),
    ("The great butter and cheese fire of 1991", "subroutine", 43, 16, false),
    ("8088 Domination Post-Mortem", "cosmojg", 30, 5, false),
    ("Running a law firm on Free software", "DeepLogin", 28, 3, false),
    ("ToothPaste", "speckx", 22, 5, false),
    ("Port 0: The Port That Does Not Exist", "NewCzech", 22, 4, false),
    (
        "Show HN: Spindle – a full-featured, type-safe web framework for OCaml 5",
        "zaiev",
        7,
        0,
        false,
    ),
];

/// The fixture item at `index` (0-based rank), with `id` as its story id.
pub fn item(index: usize, id: i32) -> HNCLIItem {
    let (title, author, score, comments, _) = STORIES[index % STORIES.len()];
    HNCLIItem {
        id,
        title: title.to_string(),
        url: format!("https://example.com/{id}"),
        author: author.to_string(),
        time: "2026-11-07T09:00:00Z".to_string(),
        time_ago: "2 hours ago".to_string(),
        score,
        comments: Some(comments),
        text: String::new(),
        links: Vec::new(),
    }
}

/// The ids the fixture marks as saved, in save-time order (newest first).
pub fn saved_ids() -> Vec<i32> {
    STORIES
        .iter()
        .enumerate()
        .filter(|(_, story)| story.4)
        .map(|(index, _)| index as i32 + 1)
        .rev()
        .collect()
}

/// A full page of fixture items, ids `1..=30`.
pub fn page() -> Vec<HNCLIItem> {
    (0..STORIES.len()).map(|index| item(index, index as i32 + 1)).collect()
}

/// The story the story-split scenes open: rank 2, "Libghostty 1.0".
pub const STORY_INDEX: usize = 1;
/// That story's id in the fixture page.
pub const STORY_ID: i32 = 2;

/// The captured article, as Markdown: a heading, prose, a fenced block the kit's highlighter
/// colours, a list and a quote. Nothing here is fetched at run time.
pub const ARTICLE: &str = "\
Today we are splitting Ghostty's core into a library. The app you run keeps its window, its \
fonts and its configuration; the terminal underneath becomes something another program can \
embed.

## The surface is deliberately small

A host hands libghostty a native surface and a size, and gets back a grid of cells it can draw \
however it likes. Input arrives as an event queue, not as callbacks into the host's event loop.

```zig
pub const Surface = struct {
    grid: *Grid,
    size: Size,
    input: *EventQueue,

    pub fn resize(self: *Surface, size: Size) void {
        self.size = size;
        self.grid.reflow(size);
    }
};
```

What does not change:

- the parser, and its escape-sequence coverage
- the renderer's cell model
- the configuration language

What does: everything that used to assume it owned a window.

> An event queue instead of callbacks is the whole design in one sentence.

The ABI is versioned by struct size, so an embedder built against an older header keeps working \
until it asks for a field that did not exist yet.";

/// The top-level comments of [`STORY_ID`], in HN's own order.
pub fn comments() -> Vec<Comment> {
    THREAD
        .iter()
        .filter(|(_, parent, ..)| parent.is_none())
        .map(|row| comment(row, 0))
        .collect()
}

/// The replies to `ids`, one level down: what `fetch_comment_children` answers with.
pub fn replies(ids: &[i32]) -> Vec<Comment> {
    THREAD
        .iter()
        .filter(|(_, parent, ..)| parent.is_some_and(|parent| ids.contains(&parent)))
        .map(|row| comment(row, depth_of(row.0)))
        .collect()
}

/// The whole discussion of [`STORY_ID`]: the top-level comments and every reply the fixture
/// holds, level by level, as the desktop's tree walk collects them.
pub fn thread() -> Vec<Comment> {
    let mut all = comments();
    let mut frontier: Vec<i32> = all.iter().map(|c| c.id).collect();
    while !frontier.is_empty() {
        let children = replies(&frontier);
        frontier = children.iter().map(|c| c.id).collect();
        all.extend(children);
    }
    all
}

fn comment(row: &(i32, Option<i32>, &str, &str, &str, &[&str]), depth: usize) -> Comment {
    let (id, _, author, age, text, links) = *row;
    Comment {
        id,
        author: author.to_string(),
        text: text.to_string(),
        links: links.iter().map(|link| (*link).to_string()).collect(),
        time_ago: age.to_string(),
        state: CommentState::Collapsed,
        depth,
        deleted: false,
        child_ids: THREAD
            .iter()
            .filter(|(_, parent, ..)| *parent == Some(id))
            .map(|(child, ..)| *child)
            .collect(),
    }
}

fn depth_of(id: i32) -> usize {
    let mut depth = 0;
    let mut at = id;
    while let Some((_, Some(parent), ..)) = THREAD.iter().find(|(row, ..)| *row == at) {
        depth += 1;
        at = *parent;
    }
    depth
}

/// The same comments as the summary's captured sources: the excerpt the model saw, and the
/// full text the citation viewer shows underneath it.
pub fn sources() -> Vec<Source> {
    comments()
        .iter()
        .map(|comment| Source {
            id: comment.id,
            parent: None,
            author: comment.author.clone(),
            text: comment.text.clone(),
            excerpt: comment.text.chars().take(1800).collect(),
        })
        .collect()
}

/// The summary the fake provider returns for [`STORY_ID`]: three bullets citing the comments
/// above, in the shape `Network::discussion` produces.
pub fn summary() -> Discussion {
    Discussion {
        overview: "Commenters read the split as a narrow ABI bet, and split on whether it pays."
            .to_string(),
        points: vec![
            Point {
                text: "libghostty splits Ghostty's core out of the app so another frontend can \
embed the terminal without the macOS shell."
                    .to_string(),
                sources: vec![901, 902],
            },
            Point {
                text: "The ABI is deliberately narrow: a surface, a grid of cells and an input \
queue, with no windowing or font policy in the library."
                    .to_string(),
                sources: vec![903],
            },
            Point {
                text: "Commenters split on whether a stable C ABI is worth the loss of Zig's \
comptime ergonomics inside the core."
                    .to_string(),
                sources: vec![904, 905],
            },
        ],
    }
}

/// One row of [`THREAD`]: `(id, parent, author, age, text, links)`.
type ThreadRow = (i32, Option<i32>, &'static str, &'static str, &'static str, &'static [&'static str]);

/// The story's discussion, in HN's own order: the five top-level comments, then their replies
/// level by level. The tree is deep enough to reach the depth cap (901000 draws at depth 3 with
/// replies) and to give the trail four segments.
const THREAD: [ThreadRow; 17] = [
    (
        901,
        None,
        "tptacek",
        "2 hours ago",
        "The interesting part is what they left out. No windowing, no font stack, no config \
language — those all stay in the app. That is the whole reason this can be embedded.",
        &[],
    ),
    (
        902,
        None,
        "kstrauser",
        "1 hour ago",
        "I have wanted this for years. Every terminal library I have tried eventually grows its \
own opinion about how a window should look.",
        &[],
    ),
    (
        903,
        None,
        "jmillikin",
        "1 hour ago",
        "A narrow ABI is great until you need one more callback. Then you version the struct and \
every embedder has to care.",
        &[],
    ),
    (
        904,
        None,
        "simonw",
        "58 minutes ago",
        "Does the host get to choose the font, or does the library still own the cell grid? That \
is the line I keep tripping over when I try to imagine embedding this in a browser.",
        &[],
    ),
    (
        905,
        None,
        "dang",
        "52 minutes ago",
        "Related from 2024: the original libghostty design notes. The section on input is the one \
worth reading first.",
        &["https://mitchellh.com/writing/libghostty-is-coming"],
    ),
    (
        9010,
        Some(901),
        "kstrauser",
        "1 hour ago",
        "The windowing split is the part I keep coming back to. Every terminal library I have \
tried eventually grows its own opinion about how a window should look, and then you are \
fighting it instead of using it.",
        &[],
    ),
    (
        90100,
        Some(9010),
        "simonw",
        "55 minutes ago",
        "Does the host get to choose the font, or does the library still own the cell grid? That \
is the line I keep tripping over when I try to imagine embedding this in a browser.",
        &[],
    ),
    (
        901000,
        Some(90100),
        "dang",
        "50 minutes ago",
        "The library owns the grid; the host owns the surface. Fonts are the host's, which is why \
the cell metrics come in through the surface rather than through the config.",
        &[],
    ),
    (
        9010000,
        Some(901000),
        "tptacek",
        "44 minutes ago",
        "That is the right line, but it means the host has to do shaping and fallback, and that is \
where most of the bugs in a terminal actually live.",
        &[],
    ),
    (
        90100000,
        Some(9010000),
        "kstrauser",
        "40 minutes ago",
        "Shaping is the part I would happily hand to HarfBuzz and never think about again.",
        &["https://harfbuzz.github.io/"],
    ),
    (
        90100001,
        Some(9010000),
        "simonw",
        "38 minutes ago",
        "Until you need a fallback chain for emoji and the shaping library disagrees with the \
system about which font wins.",
        &[],
    ),
    (
        9010001,
        Some(901000),
        "jmillikin",
        "42 minutes ago",
        "The versioning story is the part I would want written down before anyone embeds this. \
Struct size is fine until someone reorders a field.",
        &[],
    ),
    (
        9010002,
        Some(901000),
        "dang",
        "36 minutes ago",
        "Versioning by struct size is the one thing here I would copy from Vulkan without \
hesitation.",
        &[],
    ),
    (
        90101,
        Some(9010),
        "dang",
        "48 minutes ago",
        "The design notes are worth reading for the input section alone. An event queue instead \
of callbacks is the whole design in one sentence.",
        &[],
    ),
    (
        9011,
        Some(901),
        "jmillikin",
        "1 hour ago",
        "The interesting question is what happens to the app's own escape-sequence handling once \
the library owns the parser. Two parsers in one process is a bad time.",
        &[],
    ),
    (
        9030,
        Some(903),
        "dang",
        "50 minutes ago",
        "There is only one parser; the app talks to the surface, not to the wire.",
        &[],
    ),
    (
        90300,
        Some(9030),
        "tptacek",
        "45 minutes ago",
        "Which is also why the input queue is an event queue and not callbacks. Callbacks would \
put the app back on the parser's stack.",
        &[],
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_story_fixture_is_the_one_the_scenes_open() {
        let story = item(STORY_INDEX, STORY_ID);
        assert_eq!(story.title, "Libghostty 1.0");
        assert_eq!(story.id, STORY_ID);
    }

    #[test]
    fn every_summary_citation_is_a_captured_comment() {
        let sources = sources();
        for point in summary().points {
            for id in point.sources {
                assert!(
                    sources.iter().any(|source| source.id == id),
                    "citation {id} has no captured comment"
                );
            }
        }
    }

    #[test]
    fn the_article_fixture_has_a_fence_and_a_heading() {
        assert!(ARTICLE.contains("```zig"));
        assert!(ARTICLE.contains("## The surface is deliberately small"));
    }

    #[test]
    fn the_fixture_is_a_full_page_with_unique_ids() {
        let page = page();
        assert_eq!(page.len(), 30);
        let mut ids: Vec<i32> = page.iter().map(|story| story.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 30);
        assert!(page.iter().all(|story| !story.title.is_empty()));
    }

    #[test]
    fn saved_ids_are_newest_first() {
        let saved = saved_ids();
        assert_eq!(saved, vec![11, 6, 2, 1]);
    }

    #[test]
    fn the_thread_is_deep_enough_to_reach_the_cap() {
        let thread = thread();
        assert_eq!(thread.len(), 17, "five top-level comments and twelve replies");
        assert_eq!(comments().len(), 5, "the capture's top level is unchanged");
        assert_eq!(replies(&[901]).len(), 2);
        // 901000 draws at depth 3, which is the cap: its replies are behind `continue thread`.
        assert_eq!(depth_of(901000), 3);
        assert_eq!(depth_of(90100000), 5);
        // Every reply's parent is in the capture, so no row is orphaned.
        for comment in &thread {
            if let Some(parent) = THREAD
                .iter()
                .find(|(id, ..)| *id == comment.id)
                .and_then(|(_, parent, ..)| *parent)
            {
                assert!(
                    thread.iter().any(|other| other.id == parent),
                    "{} has no parent row",
                    comment.id
                );
            }
        }
    }

    #[test]
    fn a_comment_carries_the_links_the_chooser_offers() {
        let thread = thread();
        let linked: Vec<i32> = thread
            .iter()
            .filter(|comment| !comment.links.is_empty())
            .map(|comment| comment.id)
            .collect();
        assert_eq!(linked, vec![905, 90100000]);
    }
}
