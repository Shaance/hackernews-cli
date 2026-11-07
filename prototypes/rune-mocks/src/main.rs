//! rune-mocks — the desktop screens for the `hn` design mockups, rendered with Rune.
//!
//! Seven scenes, each a static frame over fixture data:
//!   feed        the story list, one column, at 1180x760
//!   feed-wide   the same page at 1600x900: two columns, filled column-major
//!   feed-saved  the transient save confirmation: dot beside the rank plus a toast
//!   story       the split view: article on the left, pinned summary + comments on the right
//!   citation    the source viewer for one summary citation
//!   comments    the discussion overview: a deep chain folded behind "continue thread"
//!   thread      a deep reply under its ancestor trail, focus mode on
//!
//! ```sh
//! cargo run -- --snap verification/feed-wide.png --tree verification/feed-wide.json \
//!     --scene feed-wide --size 1600x900 --scale 2 --theme dark
//! ```

use rune::tok::{self, radius, type_};
use rune::{AppSpec, Color, Cx, El, ScrollSpec, TextStyle, col, el, row, text};
use rune_kit::{Divider, Keybind, Kit, SegmentedControl, Toast, ToastRegion};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Feed,
    FeedWide,
    FeedSaved,
    Story,
    Citation,
    Comments,
    Thread,
}

struct State {
    screen: Screen,
    selected: usize,
    tab: usize,
    /// Two columns. The real app reads the window width on resize; a static frame declares it.
    wide: bool,
    /// Rows that fit in the feed viewport at this scene's size. The real app measures it.
    visible_rows: usize,
    hover: Option<usize>,
    just_saved: Option<usize>,
}

/// The HN API returns 30 items per page and no total.
const PAGE_SIZE: usize = 30;

/// Real front-page items captured 2026-11-07 from the Algolia HN API
/// (`/api/v1/search?tags=front_page&hitsPerPage=40`) — the same capture
/// `prototypes/rune-hn/fixtures/topstories.json` ships, extended to a full page so the
/// two-column grid has enough rows to fill.
const STORIES: [(&str, &str, u32, u32, bool); 30] = [
    ("Tesla to wind down Cybertruck production", "PLenz", 1057, 1913, true),
    ("Libghostty 1.0", "mfiguiere", 771, 122, true),
    ("A Jev that can ask Opus for help gives Opus accuracy at 4x lower cost", "theanomaly", 513, 122, false),
    ("US FAA says it will phase out floppy disks from air traffic control by 2028", "ChrisArchitect", 356, 289, false),
    ("Gemini Omni 1.2 Flash", "thisisauserid", 347, 234, false),
    ("Firecracker VM Escape: PwnEscape Part 2", "SerCe", 257, 110, true),
    ("First steps in bare-metal programming using UEFI", "LaSombra", 233, 46, false),
    ("Every Mac comes with an (undocumented) family member classifier", "infwhispers", 219, 106, false),
    ("Upload Your RAM", "susanthenerd", 180, 129, false),
    ("Devvm.dev: One VM for every project", "luispa", 169, 64, false),
    ("GitHub's new Git storage architecture", "DAlperin", 168, 38, true),
    ("RealPage rent-pricing case wraps with nearly $360M to renters", "raybb", 143, 84, false),
    ("Need for Speed Underground 2 online in the browser", "fluxusars", 141, 43, false),
    ("Restaurant Delivery Payment-Skimming Campaign", "fiej", 138, 71, false),
    ("Greece has its own Pompeii: Thera buried Akrotiri in ash 17th-16th century BCE", "derbOac", 138, 15, false),
    ("A barbarian tribe's mythic origin story is true, ancient DNA reveals", "benbreen", 136, 104, false),
    ("Chomsky and the two cultures of statistical learning (2011)", "andsoitis", 135, 104, false),
    ("Can GROUP BY still be optimized after all these years?", "ntur1337", 95, 16, false),
    ("Notes on San Marino", "yorwba", 90, 34, false),
    ("Google has acquired Crusoe's data center site in southeast Wyoming", "rexbee", 86, 33, false),
    ("The Components of Agent Memory", "geoffbp", 76, 40, false),
    ("The Flash C Compiler", "AlexeyBrin", 64, 19, false),
    ("Every Windows 11 app should kill Electron, and Discord just showed why", "thunderbong", 47, 34, false),
    ("PostgreSQL Internals [pdf]", "Foe", 45, 5, false),
    ("The great butter and cheese fire of 1991", "subroutine", 43, 16, false),
    ("8088 Domination Post-Mortem", "cosmojg", 30, 5, false),
    ("Running a law firm on Free software", "DeepLogin", 28, 3, false),
    ("ToothPaste", "speckx", 22, 5, false),
    ("Port 0: The Port That Does Not Exist", "NewCzech", 22, 4, false),
    ("Show HN: Spindle – a full-featured, type-safe web framework for OCaml 5", "zaiev", 7, 0, false),
];

const POINTS: [(&str, &[&str]); 3] = [
    ("libghostty splits Ghostty's core out of the app so other frontends can embed the terminal without the macOS shell.", &["1", "2"]),
    ("The ABI is deliberately narrow: a surface, a grid of cells and an input queue, with no windowing or font policy in the library.", &["3"]),
    ("Commenters split on whether a stable C ABI is worth the loss of Zig's comptime ergonomics inside the core.", &["4", "5"]),
];

/// One comment in the thread, in document order. `depth` is the indent level; `hidden` is the
/// number of replies folded under a collapsed comment (0 when it is open).
struct Comment {
    id: &'static str,
    depth: usize,
    author: &'static str,
    age: &'static str,
    body: &'static str,
    hidden: usize,
}

/// Indent levels drawn before the guides stop. Past this the trail carries the depth instead,
/// so the measure never falls below ~66 characters.
const MAX_DEPTH: usize = 4;

const THREAD: [Comment; 14] = [
    Comment { id: "c1", depth: 0, author: "tptacek", age: "2 hours ago", hidden: 0,
        body: "The interesting part is what they left out. No windowing, no font stack, no config language — those all stay in the app. That is the whole reason this can be embedded." },
    Comment { id: "c2", depth: 1, author: "kstrauser", age: "1 hour ago", hidden: 0,
        body: "I have wanted this for years. Every terminal library I have tried eventually grows its own opinion about how a window should look." },
    Comment { id: "c3", depth: 2, author: "jmillikin", age: "1 hour ago", hidden: 0,
        body: "A narrow ABI is great until you need one more callback. Then you version the struct and every embedder has to care." },
    Comment { id: "c4", depth: 3, author: "simonw", age: "58 minutes ago", hidden: 0,
        body: "Does the host get to choose the font, or does the library still own the cell grid? That is the line I keep tripping over when I try to imagine embedding this in a browser." },
    Comment { id: "c5", depth: 4, author: "dang", age: "52 minutes ago", hidden: 0,
        body: "Related from 2024: the original libghostty design notes. The section on input is the one worth reading first." },
    Comment { id: "c6", depth: 5, author: "kstrauser", age: "40 minutes ago", hidden: 0,
        body: "That section is the one I keep coming back to. An event queue instead of callbacks is the whole design in one sentence." },
    Comment { id: "c7", depth: 6, author: "tptacek", age: "31 minutes ago", hidden: 0,
        body: "Which is also why the queue is bounded. An unbounded queue in a terminal is just a memory leak with extra steps." },
    Comment { id: "c8", depth: 7, author: "jmillikin", age: "22 minutes ago", hidden: 0,
        body: "Bounded, and the host decides what to drop. That is the part I would want documented before I trusted it under load." },
    Comment { id: "c9", depth: 8, author: "simonw", age: "18 minutes ago", hidden: 0,
        body: "The drop policy is the interesting bit. If the host drops input, the terminal shows a stale screen and nobody knows why." },
    Comment { id: "c9a", depth: 9, author: "dang", age: "12 minutes ago", hidden: 0,
        body: "There is a section on backpressure in the notes that covers exactly this." },
    Comment { id: "c9b", depth: 10, author: "kstrauser", age: "6 minutes ago", hidden: 0,
        body: "Read it. It argues for dropping whole frames rather than individual events, which is the only choice that keeps the grid consistent." },
    Comment { id: "c5b", depth: 4, author: "kstrauser", age: "44 minutes ago", hidden: 0,
        body: "The notes are also the only place the surface handshake is written down. The README skips it entirely." },
    Comment { id: "c10", depth: 1, author: "dang", age: "3 hours ago", hidden: 3,
        body: "Related from 2024: the original libghostty design notes. Worth reading alongside this." },
    Comment { id: "c11", depth: 1, author: "mfiguiere", age: "1 hour ago", hidden: 0,
        body: "Author here. Happy to answer questions about the ABI, but the design notes cover most of what people are asking." },
];

/// Replies under `i`: every following entry deeper than it, up to the next sibling.
fn replies_under(i: usize) -> usize {
    let d = THREAD[i].depth;
    THREAD[i + 1..].iter().take_while(|c| c.depth > d).count()
}

/// How many pages to fetch so the last visible row is never half empty.
///
/// `visible cells = visible rows × columns`, `pagesNeeded = ceil(visibleCells / pageSize)`.
/// At 1600×900 the list viewport is 796 pt tall, so 14 rows × 2 columns = 28 cells and one
/// page of 30 fills the grid. A 1600×1200 window fits 19 rows → 38 cells → two pages.
fn pages_needed(visible_rows: usize, cols: usize, page_size: usize) -> usize {
    (visible_rows * cols).div_ceil(page_size).max(1)
}

fn view(s: &State, _: &mut Cx<State>) -> El {
    let t = tok::theme(true);
    let k = Kit::neutral(true);
    let accent = tok::accent(3, true);
    let body = match s.screen {
        Screen::Feed | Screen::FeedWide | Screen::FeedSaved => feed(s, &k, t, accent),
        Screen::Story => story(&k, t, accent, false),
        Screen::Citation => story(&k, t, accent, true),
        Screen::Comments => discussion(&k, t, accent, false),
        Screen::Thread => discussion(&k, t, accent, true),
    };
    let base = col().w_full().h_full().fill(t.bg).child(body);
    if s.screen != Screen::FeedSaved {
        return base;
    }
    // The save confirmation is transient: it lives in the toast region, not on the row.
    let toast = Toast::new("saved", "Saved")
        .description("Press 4 for Saved")
        .lead(saved_dot("toast-dot", accent))
        .build(&k);
    rune_kit::root(base, [ToastRegion::new().toast(toast).build(&k)])
}

fn feed(s: &State, k: &Kit, t: tok::Theme, accent: Color) -> El {
    let tabs = SegmentedControl::new(&["Top", "New", "Best", "Saved"], Some(s.tab))
        .label("Feed")
        .fit()
        .build(k);
    // Navigation only. The titlebar already names the app and the feed, and the footer already
    // carries the key hints, so the toolbar repeats neither.
    let toolbar = row().w_full().px(24.0).py(16.0).gap(18.0).center_y().child(tabs);

    let footer = row().w_full().px(24.0).py(12.0).gap(18.0).center_y()
        .child(hint(k, t, &["j", "k"], "move"))
        .child(hint(k, t, &["Enter"], "open"))
        .child(hint(k, t, &["s"], "save"))
        .child(hint(k, t, &["n", "p"], "page"))
        .child(el().grow(1.0))
        .child(stepper(t));

    col().w_full().h_full()
        .child(toolbar)
        .child(el().w_full().h(1.0).fill(t.separator))
        .child(
            feed_list(s, t, accent)
                .scroll(ScrollSpec { stick_bottom: false })
                .scroll_edge_fade(20.0)
                .grow(1.0)
                .min_h(0.0),
        )
        .child(el().w_full().h(1.0).fill(t.separator))
        .child(footer)
}

/// Column-major: story `c * rows + r`, so the left column fills first and `j` walks straight
/// down it before it ever reaches the right column. One scroll viewport for both columns, and
/// every cell keeps its own key, so `Tx::scroll_into_view` reaches a story whichever column it
/// is in.
fn feed_list(s: &State, t: tok::Theme, accent: Color) -> El {
    let cols = if s.wide { 2 } else { 1 };
    let shown = (pages_needed(s.visible_rows, cols, PAGE_SIZE) * PAGE_SIZE).min(STORIES.len());
    let rows = shown.div_ceil(cols);
    let mut list = col().key("stories").w_full().gap(if cols == 2 { 0.0 } else { 2.0 });
    for r in 0..rows {
        let mut line = row().key(format!("line-{r}")).w_full().h(56.0);
        for c in 0..cols {
            let i = c * rows + r;
            if i >= shown {
                break;
            }
            if c > 0 {
                // The hairline between columns. Rows sit at gap 0 in two-column mode, so the
                // per-row rules join into one continuous divider.
                line = line.child(el().w(1.0).h_full().fill(t.separator));
            }
            line = line.child(story_row(s, i, t, accent).grow(1.0));
        }
        list = list.child(line);
    }
    list
}

fn story_row(s: &State, i: usize, t: tok::Theme, accent: Color) -> El {
    let (title, by, score, comments, saved) = STORIES[i];
    let selected = i == s.selected;
    let saved = saved || s.just_saved == Some(i);

    let mut r = row().key(format!("story-{i}")).h_full().px(12.0).gap(14.0).center_y().radius(radius::SM);
    if selected {
        r = r.child(el().w(2.0).h(26.0).radius(1.0).fill(accent));
    }
    // The saved marker is a dot beside the rank, never right-aligned chrome. The Saved view is
    // already a list of saved stories, so its rows carry no dot.
    let mut lead = row().gap(6.0).center_y().child(
        row().w(18.0).justify_end().child(
            text(format!("{}", i + 1))
                .font(type_::LABEL)
                .color(if selected { accent } else { t.text_secondary }),
        ),
    );
    if saved && s.tab != 3 {
        lead = lead.child(saved_dot(format!("saved-dot-{i}"), accent));
    }
    r = r.child(lead).child(
        col().grow(1.0).gap(4.0).min_w(0.0)
            .child(text(title).font(type_::BODY).color(t.text).truncate())
            .child(
                text(format!("{score} points  \u{00b7}  by {by}  \u{00b7}  {comments} comments"))
                    .font(type_::LABEL)
                    .color(t.text_secondary),
            ),
    );
    // The right edge is hover-actions only; nothing persists there.
    if s.hover == Some(i) {
        r = r.child(
            row().gap(6.0).center_y()
                .child(act("save", accent, t))
                .child(act("open", t.text_secondary, t))
                .child(act("browser", t.text_secondary, t)),
        );
    }
    if selected {
        r = r.fill(t.card);
    }
    r
}

/// The saved state, everywhere it appears: a 6 pt amber dot. The word lives in the accessible
/// label, not in the layout.
fn saved_dot(key: impl Into<String>, accent: Color) -> El {
    el().key(key).size(6.0, 6.0).radius(3.0).fill(accent).role("status").label("saved")
}

fn act(label: &str, color: Color, t: tok::Theme) -> El {
    row().h(20.0).px(7.0).center().radius(4.0).fill(t.control_off)
        .child(text(label).font(type_::LABEL).color(color))
}

/// `‹ N ›` — the current page only. The HN API reports no total, so there is no "of N" to show.
fn stepper(t: tok::Theme) -> El {
    row().key("page-stepper").role("group").label("Page 1").gap(2.0).center_y()
        .child(chev("\u{2039}", t))
        .child(row().w(20.0).center().child(text("1").font(type_::LABEL).color(t.text)))
        .child(chev("\u{203a}", t))
}

fn chev(glyph: &str, t: tok::Theme) -> El {
    row().size(18.0, 18.0).center().radius(4.0).fill(t.control_off)
        .child(text(glyph).font(type_::LABEL).color(t.text_secondary))
}

/// The story toolbar. `summary · fast` is the one place the summary's profile is stated: it is
/// the "a summary exists, and which one" indicator. The card below says only "Summary".
fn story_toolbar(t: tok::Theme, accent: Color) -> El {
    row().w_full().px(24.0).py(14.0).gap(14.0).center_y()
        .child(text("\u{2039}").font(type_::TITLE).color(t.text_secondary))
        .child(text("Libghostty 1.0").font(type_::TITLE).color(t.text))
        .child(saved_dot("story-saved", accent))
        .child(text("mfiguiere  \u{00b7}  771 points  \u{00b7}  122 comments").font(type_::LABEL).color(t.text_secondary))
        .child(el().grow(1.0))
        .child(pill("summary \u{00b7} fast", t, accent))
}

fn split_frame(t: tok::Theme, left: El, right: El, footer: El) -> El {
    col().w_full().h_full()
        .child(story_toolbar(t, tok::accent(3, true)))
        .child(el().w_full().h(1.0).fill(t.separator))
        .child(
            row().w_full().grow(1.0).min_h(0.0)
                .child(left)
                .child(grip(t))
                .child(right),
        )
        .child(el().w_full().h(1.0).fill(t.separator))
        .child(footer)
}

fn story(k: &Kit, t: tok::Theme, accent: Color, citation: bool) -> El {
    let right = if citation { source_viewer(t, accent) } else { comments(k, t, accent) };
    let footer = row().w_full().px(24.0).py(12.0).gap(18.0).center_y()
        .child(hint(k, t, &["j", "k"], "comment"))
        .child(hint(k, t, &["[", "]"], "sibling"))
        .child(hint(k, t, &["Tab"], "citation"))
        .child(hint(k, t, &["S"], "summary"))
        .child(el().grow(1.0))
        .child(text("Esc back to the feed").font(type_::LABEL).color(t.text_secondary));
    split_frame(t, article(t, citation), right, footer)
}

fn article(t: tok::Theme, dimmed: bool) -> El {
    let pane = col().w(636.0).h_full().px(28.0).py(22.0).gap(14.0).min_h(0.0)
        .child(text("ARTICLE").font(type_::SECTION).color(t.text_secondary))
        .child(text("Libghostty 1.0").font(TextStyle::new(22.0, 600.0)).color(t.text))
        .child(text("mfiguiere  \u{00b7}  ghostty.org  \u{00b7}  extracted, 4.2k words").font(type_::LABEL).color(t.text_secondary))
        .child(
            text("Today we are splitting Ghostty's core into a library. The app you run keeps its window, its fonts and its configuration; the terminal underneath becomes something another program can embed.")
                .font(type_::BODY).color(t.text).wrap(),
        )
        .child(
            text("The surface is deliberately small. A host hands libghostty a native surface and a size, and gets back a grid of cells it can draw however it likes. Input arrives as an event queue, not as callbacks into the host's event loop.")
                .font(type_::BODY).color(t.text).wrap(),
        )
        .child(
            col().w_full().px(12.0).py(10.0).radius(radius::SM).fill(t.card).gap(4.0)
                .child(text("$ zig build -Doptimize=ReleaseFast").font(type_::MONO_CAPTION).color(t.text))
                .child(text("$ ./zig-out/bin/libghostty-demo").font(type_::MONO_CAPTION).color(t.text_secondary)),
        )
        .child(
            text("What does not change: the parser, the renderer's cell model, and the escape-sequence coverage. What does: everything that used to assume it owned a window.")
                .font(type_::BODY).color(t.text).wrap(),
        );
    if dimmed { pane.dim(0.55) } else { pane }
}

fn grip(t: tok::Theme) -> El {
    col().w(9.0).h_full().items_center()
        .child(el().w(1.0).h(280.0).fill(t.separator))
        .child(el().w(3.0).h(28.0).radius(2.0).fill(t.indicator))
        .child(el().w(1.0).h(280.0).fill(t.separator))
}

fn comments(k: &Kit, t: tok::Theme, accent: Color) -> El {
    let list = discussion_rows(&[0, 1, 2, 3, 4, 12, 13], 0, &[], Some(1), t, accent)
        .scroll(ScrollSpec { stick_bottom: false }).scroll_edge_fade(20.0).grow(1.0).min_h(0.0);
    col().grow(1.0).h_full().min_h(0.0).min_w(0.0).px(22.0).py(18.0).gap(14.0)
        .child(summary_card(k, t, accent))
        .child(text("DISCUSSION  \u{00b7}  122 comments").font(type_::SECTION).color(t.text_secondary))
        .child(list)
}

/// The discussion screens: the same split, with the summary folded to one line so the thread
/// gets the room. `focused` turns on focus mode — the ancestor trail plus dimmed siblings.
fn discussion(k: &Kit, t: tok::Theme, accent: Color, focused: bool) -> El {
    let (rows, base, dim, selected): (&[usize], usize, &[usize], Option<usize>) = if focused {
        (&[4, 5, 6, 7, 8, 11], 4, &[11], Some(4))
    } else {
        (&[0, 1, 2, 3, 4, 12, 13], 0, &[], Some(1))
    };
    let mut list = col().key("discussion").w_full().gap(2.0);
    if focused {
        list = list.child(trail(&["tptacek", "kstrauser", "jmillikin", "simonw", "dang"], t, accent));
    }
    let list = list.child(discussion_rows(rows, base, dim, selected, t, accent))
        .scroll(ScrollSpec { stick_bottom: false }).scroll_edge_fade(20.0).grow(1.0).min_h(0.0);
    let footer = row().w_full().px(24.0).py(12.0).gap(18.0).center_y()
        .child(hint(k, t, &["j", "k"], "comment"))
        .child(hint(k, t, &["[", "]"], "sibling"))
        .child(hint(k, t, &["Enter"], "continue thread"))
        .child(hint(k, t, &["f"], "focus"))
        .child(hint(k, t, &["\u{232b}"], "up one level"))
        .child(el().grow(1.0))
        .child(text(if focused { "Esc leaves focus" } else { "Esc back to the feed" })
            .font(type_::LABEL).color(t.text_secondary));
    // The summary folds to one line on the discussion screens so the thread gets the height.
    let mut right = col().grow(1.0).h_full().min_h(0.0).min_w(0.0).px(22.0).py(18.0).gap(14.0)
        .child(summary_strip(t));
    if !focused {
        right = right.child(text("DISCUSSION  \u{00b7}  122 comments").font(type_::SECTION).color(t.text_secondary));
    }
    split_frame(t, article(t, false), right.child(list), footer)
}

/// The ancestor chain above the focused comment. Sticky: it holds the top of the scroll viewport
/// while the thread moves under it, so "where am I" never scrolls away.
fn trail(names: &[&str], t: tok::Theme, accent: Color) -> El {
    let mut r = row().key("thread-trail").role("group")
        .label(format!("thread: {}", names.join(" > ")))
        .w_full().px(10.0).py(9.0).gap(7.0).center_y().fill(t.card).sticky_top(0.0);
    for (i, n) in names.iter().enumerate() {
        if i > 0 {
            r = r.child(text("\u{203a}").font(type_::LABEL).color(t.text_secondary));
        }
        let last = i + 1 == names.len();
        r = r.child(text(*n).font(type_::LABEL).color(if last { accent } else { t.text_secondary }));
    }
    r.child(el().grow(1.0))
        .child(text("focus  \u{00b7}  \u{232b} up one level").font(type_::LABEL).color(t.text_secondary))
}

fn discussion_rows(rows: &[usize], base: usize, dim: &[usize], selected: Option<usize>, t: tok::Theme, accent: Color) -> El {
    let mut list = col().w_full().gap(2.0);
    for &i in rows {
        let c = &THREAD[i];
        let depth = c.depth.saturating_sub(base).min(MAX_DEPTH);
        // A comment whose replies are not in this view gets the "continue thread" affordance
        // instead of its children; a collapsed one gets the hidden count.
        let more = if replies_under(i) > 0 && !rows.contains(&(i + 1)) {
            Some(replies_under(i))
        } else {
            None
        };
        list = list.child(comment_row(c, depth, selected == Some(i), dim.contains(&i), more, t, accent));
    }
    list
}

fn comment_row(c: &Comment, depth: usize, selected: bool, dimmed: bool, more: Option<usize>, t: tok::Theme, accent: Color) -> El {
    let mut guides = row().h_full().gap(13.0);
    for _ in 0..depth {
        guides = guides.child(el().w(1.0).h_full().fill(t.separator));
    }
    let mut card = row()
        .key(format!("comment-{}", c.id))
        .role("treeitem")
        .label(format!("{} \u{00b7} depth {}", c.author, c.depth))
        .expanded(c.hidden == 0)
        .w_full().gap(12.0).items_start();
    if selected {
        card = card.px(12.0).py(10.0).radius(radius::SM).fill(t.card)
            .child(el().w(2.0).h(24.0).radius(1.0).fill(accent));
    }
    let mut body = col().grow(1.0).gap(6.0).min_w(0.0)
        .child(
            row().gap(8.0).center_y()
                .child(text(c.author).font(type_::LABEL).color(if selected { accent } else { t.text }))
                .child(text(c.age).font(type_::LABEL).color(t.text_secondary)),
        )
        .child(text(c.body).font(type_::BODY).color(t.text).wrap());
    if c.hidden > 0 {
        body = body.child(fold_row(format!("fold-{}", c.id), format!("{} replies hidden  \u{00b7}  Enter expands", c.hidden), t));
    }
    if let Some(n) = more {
        body = body.child(fold_row(format!("more-{}", c.id), format!("continue thread  \u{2192}  {n} replies"), t));
    }
    card = card.child(guides).child(body);
    if dimmed { card.dim(0.35) } else { card }
}

fn fold_row(key: String, label: String, t: tok::Theme) -> El {
    row().key(key).role("button").label(label.clone()).gap(6.0).center_y().h(20.0)
        .child(text(label).font(type_::LABEL).color(t.text_secondary))
}

fn summary_card(k: &Kit, t: tok::Theme, accent: Color) -> El {
    let mut points = col().w_full().gap(10.0);
    for (i, (body, cites)) in POINTS.iter().enumerate() {
        let mut chips = row().gap(6.0);
        for c in cites.iter() {
            chips = chips.child(chip(c, t, accent));
        }
        points = points.child(
            row().w_full().gap(10.0).items_start()
                .child(text(format!("{}", i + 1)).font(type_::LABEL).color(accent))
                .child(col().grow(1.0).gap(6.0).min_w(0.0).child(text(*body).font(type_::BODY).color(t.text).wrap()).child(chips)),
        );
    }
    // The header states the card's identity and its provenance. The profile ("fast") lives in
    // the toolbar pill and nowhere else — one state, one place.
    col().key("summary").w_full().px(16.0).py(14.0).gap(10.0).radius(radius::MD)
        .fill(t.card).border(1.0, t.separator)
        .child(
            row().w_full().gap(8.0).center_y()
                .child(text("Summary").font(type_::TITLE).color(t.text))
                .child(el().grow(1.0))
                .child(text("captured 2m ago  \u{00b7}  3 of 3 sources").font(type_::LABEL).color(t.text_secondary)),
        )
        .child(points)
        .child(Divider::horizontal().build(k))
        .child(text("Tab selects a citation  \u{00b7}  Enter reads its source").font(type_::LABEL).color(t.text_secondary))
}

/// The summary folded to one line, so a deep thread gets the height. `S` expands it.
fn summary_strip(t: tok::Theme) -> El {
    row().key("summary").w_full().px(14.0).py(10.0).gap(8.0).center_y().radius(radius::MD)
        .fill(t.card).border(1.0, t.separator)
        .child(text("\u{25b8}").font(type_::LABEL).color(t.text_secondary))
        .child(text("Summary").font(type_::TITLE).color(t.text))
        .child(text("3 bullets  \u{00b7}  captured 2m ago  \u{00b7}  3 of 3 sources").font(type_::LABEL).color(t.text_secondary))
        .child(el().grow(1.0))
        .child(text("S expands").font(type_::LABEL).color(t.text_secondary))
}

fn chip(label: &str, t: tok::Theme, accent: Color) -> El {
    row().key(format!("cite-{label}")).min_w(18.0).h(18.0).px(5.0).center().radius(4.0)
        .fill(t.control_off)
        .child(text(label).font(type_::LABEL).color(accent))
}

fn source_viewer(t: tok::Theme, accent: Color) -> El {
    col().grow(1.0).h_full().min_h(0.0).min_w(0.0).px(22.0).py(18.0).gap(14.0)
        .child(
            row().w_full().gap(10.0).center_y()
                .child(text("Source 2 of 3").font(type_::TITLE).color(t.text))
                .child(pill("citation 2", t, accent))
                .child(el().grow(1.0))
                .child(text("Esc returns to your place").font(type_::LABEL).color(t.text_secondary)),
        )
        .child(text("EXCERPT SENT TO THE MODEL").font(type_::SECTION).color(t.text_secondary))
        .child(
            col().w_full().px(14.0).py(12.0).gap(8.0).radius(radius::SM).fill(t.card).border(1.0, t.separator)
                .child(text("kstrauser, 1 hour ago").font(type_::LABEL).color(accent))
                .child(
                    text("I have wanted this for years. Every terminal library I have tried eventually grows its own opinion about how a window should look.")
                        .font(type_::BODY).color(t.text).wrap(),
                ),
        )
        .child(text("ADDITIONAL CAPTURED TEXT").font(type_::SECTION).color(t.text_secondary))
        .child(
            col().w_full().px(14.0).py(12.0).gap(8.0).radius(radius::SM).fill(t.control_track)
                .child(text("jmillikin, 1 hour ago").font(type_::LABEL).color(t.text_secondary))
                .child(
                    text("A narrow ABI is great until you need one more callback. Then you version the struct and every embedder has to care.")
                        .font(type_::BODY).color(t.text_secondary).wrap(),
                ),
        )
        .child(text("A citation links to captured evidence; it does not guarantee the model read it correctly.").font(type_::LABEL).color(t.text_secondary).wrap())
}

/// An amber pill: the kit's `Badge` accent is the theme's blue, and hn's accent is amber.
fn pill(label: &str, t: tok::Theme, accent: Color) -> El {
    row().role("status").label(label).gap(6.0).center_y().h(18.0).px(7.0).radius(4.0)
        .fill(t.control_off)
        .child(el().size(5.0, 5.0).radius(3.0).fill(accent))
        .child(text(label).font(type_::LABEL).color(accent))
}

fn hint(k: &Kit, t: tok::Theme, keys: &[&str], label: &str) -> El {
    row().gap(6.0).center_y()
        .child(Keybind::new(keys).build(k))
        .child(text(label).font(type_::LABEL).color(t.text_secondary))
}

fn main() {
    rune::run(AppSpec {
        scenes: &[
            ("feed", |s: &mut State| s.screen = Screen::Feed),
            ("feed-wide", |s: &mut State| {
                s.screen = Screen::FeedWide;
                s.wide = true;
                s.visible_rows = 14;
                s.hover = Some(3);
            }),
            ("feed-saved", |s: &mut State| {
                s.screen = Screen::FeedSaved;
                s.selected = 2;
                s.just_saved = Some(2);
            }),
            ("story", |s: &mut State| s.screen = Screen::Story),
            ("citation", |s: &mut State| s.screen = Screen::Citation),
            ("comments", |s: &mut State| s.screen = Screen::Comments),
            ("thread", |s: &mut State| s.screen = Screen::Thread),
        ],
        ..AppSpec::new(
            "hn",
            (1180.0, 760.0),
            || State { screen: Screen::Feed, selected: 1, tab: 0, wide: false, visible_rows: 11, hover: None, just_saved: None },
            view,
        )
    });
}
