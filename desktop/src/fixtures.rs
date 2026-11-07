//! The deterministic story set the headless scenes and the fake service render.
//!
//! Real front-page items captured 2026-11-07 from the Algolia HN API
//! (`/api/v1/search?tags=front_page&hitsPerPage=40`) — the same capture
//! `prototypes/rune-hn/fixtures/topstories.json` ships, extended to a full page so the
//! two-column grid has enough rows to fill. Nothing here is fetched at run time.

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
