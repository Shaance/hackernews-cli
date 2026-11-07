//! rune-hn — a native-UI Hacker News "top stories" list built with Rune.
//!
//! Fetches live from the HN Firebase API and falls back to a bundled fixture, so the headless
//! render (`--snap` / `--tree`) produces a populated list offline.
//!
//! ```sh
//! cargo run --release                       # the window
//! cargo run --release -- --snap out.png --tree tree.json --size 760x620 --scale 2
//! ```

use std::time::Duration;

use rune::tok::{self, radius, type_};
use rune::{AppSpec, Color, Cx, El, Key, KeyEvent, ScrollSpec, Tx, col, el, row, text};
use serde::Deserialize;

const TOP_STORIES: &str = "https://hackernews.firebaseio.com/v0/topstories.json";
const ITEM: &str = "https://hackernews.firebaseio.com/v0/item/";
const USER_AGENT: &str = "rune-hn-prototype/0.1 (+https://github.com/Shaance/hackernews-cli)";
const FIXTURE: &str = include_str!("../fixtures/topstories.json");
const LIMIT: usize = 12;

/// One HN item, as `/v0/item/<id>.json` returns it. The fixture uses the same shape, so one
/// parser reads both.
#[derive(Clone, Deserialize)]
struct Item {
    id: u64,
    title: String,
    #[serde(default)]
    by: String,
    #[serde(default)]
    score: u64,
    #[serde(default)]
    descendants: u64,
}

/// Where the rows on screen came from. Shown in the header and in `--tree`, so a headless run
/// says which path it took.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    Live,
    Fixture,
}

impl Source {
    fn label(self) -> &'static str {
        match self {
            Source::Live => "live",
            Source::Fixture => "fixture",
        }
    }
}

struct State {
    stories: Vec<Item>,
    selected: usize,
    source: Source,
    note: String,
}

fn init() -> State {
    match fetch_live() {
        Some(stories) => State { stories, selected: 0, source: Source::Live, note: String::new() },
        None => State {
            stories: serde_json::from_str(FIXTURE).unwrap_or_default(),
            selected: 0,
            source: Source::Fixture,
            note: "HN API unreachable — showing the bundled snapshot".to_string(),
        },
    }
}

/// The top stories, or `None` when the API is unreachable or returns too little to be useful.
///
/// This runs in `init`, before the first frame, because a headless `--snap` renders frame 0 on a
/// virtual clock and would otherwise capture a loading state. A shipping app would start it from
/// `AppSpec::setup` on a worker and land it in `poll` (see `rune::Latest`).
fn fetch_live() -> Option<Vec<Item>> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(6)))
        .build()
        .into();
    let ids: Vec<u64> = serde_json::from_str(&get(&agent, TOP_STORIES)?).ok()?;
    let mut stories = Vec::new();
    for id in ids.into_iter().take(LIMIT * 2) {
        let Some(body) = get(&agent, &format!("{ITEM}{id}.json")) else { continue };
        let Ok(item) = serde_json::from_str::<Item>(&body) else { continue };
        if !item.title.is_empty() {
            stories.push(item);
        }
        if stories.len() == LIMIT {
            break;
        }
    }
    // A couple of dead ids are normal; a near-empty list means the API is having a bad day.
    (stories.len() >= 3).then_some(stories)
}

fn get(agent: &ureq::Agent, url: &str) -> Option<String> {
    let mut res = agent.get(url).header("User-Agent", USER_AGENT).call().ok()?;
    res.body_mut().read_to_string().ok()
}

fn view(s: &State, _: &mut Cx<State>) -> El {
    let t = tok::theme(true);
    let accent = tok::accent(0, true);

    let header = row().w_full().px(20.0).py(14.0).gap(10.0).center_y()
        .child(text("Hacker News").font(type_::TITLE).color(t.text))
        .child(text("Top Stories").font(type_::LABEL).color(t.text_secondary))
        .child(el().grow(1.0))
        .child(badge(s.source, t, accent));

    let mut list = col().key("stories").gap(2.0);
    for (i, story) in s.stories.iter().enumerate() {
        list = list.child(story_row(i, story, i == s.selected, t, accent));
    }

    let mut footer = row().w_full().px(20.0).py(10.0).gap(16.0).center_y()
        .child(text("j / k  or  \u{2191} \u{2193}   move").font(type_::LABEL).color(t.text_secondary))
        .child(text("click a row to select").font(type_::LABEL).color(t.text_secondary))
        .child(el().grow(1.0));
    if !s.note.is_empty() {
        footer = footer.child(text(s.note.clone()).font(type_::LABEL).color(t.text_secondary));
    }

    col().w_full().h_full().fill(t.bg)
        .child(header)
        .child(el().w_full().h(1.0).fill(t.separator))
        // `.grow(1.0)` is `flex: 1 1 0`, so the viewport needs a bounded ancestor (`h_full` above)
        // and `.min_h(0.0)` so it can shrink below its content and actually scroll.
        .child(list.scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0))
        .child(el().w_full().h(1.0).fill(t.separator))
        .child(footer)
}

fn badge(source: Source, t: tok::Theme, accent: Color) -> El {
    row().key("source").gap(6.0).center_y().px(8.0).py(3.0).radius(radius::SM)
        .fill(t.control_off)
        .child(el().size(6.0, 6.0).radius(3.0).fill(accent))
        .child(text(source.label()).font(type_::LABEL).color(t.text_secondary))
}

fn story_row(rank: usize, story: &Item, selected: bool, t: tok::Theme, accent: Color) -> El {
    let index = rank;
    let author = if story.by.is_empty() { "unknown" } else { story.by.as_str() };
    let meta = format!(
        "{} points  \u{00b7}  by {author}  \u{00b7}  {} comments",
        story.score, story.descendants
    );
    let mut r = row().key(format!("story-{}", story.id))
        .w_full().h(52.0).px(20.0).gap(14.0).center_y().radius(radius::SM)
        .on_press(move |s: &mut State| s.selected = index)
        .child(
            text(format!("{}", rank + 1)).font(type_::LABEL)
                .color(if selected { accent } else { t.text_secondary }),
        )
        .child(
            col().grow(1.0).gap(3.0).min_w(0.0)
                .child(text(story.title.clone()).font(type_::BODY).color(t.text))
                .child(text(meta).font(type_::LABEL).color(t.text_secondary)),
        );
    if selected {
        r = r.fill(t.card);
    }
    r
}

fn on_key(s: &mut State, ev: &KeyEvent, tx: &mut Tx) -> bool {
    let last = s.stories.len().saturating_sub(1);
    let next = match &ev.key {
        Key::Down => (s.selected + 1).min(last),
        Key::Up => s.selected.saturating_sub(1),
        Key::Home => 0,
        Key::End => last,
        Key::Text(t) if t.as_str() == "j" => (s.selected + 1).min(last),
        Key::Text(t) if t.as_str() == "k" => s.selected.saturating_sub(1),
        _ => return false,
    };
    s.selected = next;
    if let Some(story) = s.stories.get(next) {
        tx.scroll_into_view(format!("story-{}", story.id));
    }
    true
}

fn main() {
    rune::run(AppSpec {
        on_key: Some(on_key),
        ..AppSpec::new("Hacker News", (760.0, 620.0), init, view)
    });
}
