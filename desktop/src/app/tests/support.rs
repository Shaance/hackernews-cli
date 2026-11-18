//! Shared app test setup and interaction helpers.

use super::super::*;
use std::time::Duration;
use rune::KeyEvent;
use rune::Tx;
use super::super::input::on_key;
use super::super::poll::poll;

pub(super) fn grid(rows: usize, columns: usize) -> Grid {
    Grid::measured(rows, match columns {
        1 => Columns::One,
        2 => Columns::Two,
        _ => panic!("invalid fixture columns"),
    })
}

pub(super) fn press(s: &mut App, key: rune::Key) {
    on_key(
        s,
        &KeyEvent {
            key,
            mods: rune::Mods::default(),
        },
        &mut Tx::default(),
    );
}

pub(super) fn poll_until(s: &mut App, ready: impl Fn(&App) -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while !ready(s) {
        poll(s, 0.0, &mut Tx::default());
        assert!(std::time::Instant::now() < deadline, "story phase did not arrive");
        std::thread::yield_now();
    }
}

pub(super) fn route(rt: &mut rune::Runtime<App>, key: rune::Key) {
    rt.key(KeyEvent { key, mods: Default::default() });
    rt.frame(0.3);
}

pub(super) fn summary_contents(summary: &crate::story::Summary) -> (String, Vec<(String, Vec<i32>)>) {
    (summary.overview.clone(), summary.bullets.iter().map(|bullet|
        (bullet.text.clone(), bullet.citations.iter().map(|citation| citation.source.id).collect())).collect())
}

/// The inspector the window builds: 543 points, clamped to 360..=640, collapsed.
pub(super) fn inspector() -> PaneState {
    PaneState::new(INSPECTOR_WIDTH)
        .range(INSPECTOR_MIN, INSPECTOR_MAX)
        .collapsed()
}

/// An app built the way `init` builds one, without the machine's files or the network.
/// `headless` runs the requests to completion, so a test sees the settled feed.
pub(super) fn app(headless: bool, saved: SavedView) -> App {
    App {
        feed: FeedModel::new(),
        saved,
        source: Arc::new(Source::fake()),
        bridge: Bridge::new().expect("a tokio runtime for the feed's requests"),
        measured: Cell::new(None),
        feed_revision: 0,
        pending_arrival: Some(Arrival::Initial),
        arrival: Some(Arrival::Initial),
        hover: None,
        keyboard_selection: true,
        hints: rune::Live::new(1.0),
        story: None,
        visible_comment: Cell::new(None),
        comment_offsets: [Rc::new(Cell::new(0.0)), Rc::new(Cell::new(0.0))],
        readings: HashMap::new(),
        scroll_pacing: Rc::new(Cell::new(ScrollPacing::Idle)),
        scroll_waker: None,
        inspector: Cell::new(inspector()),
        inspector_width: Cell::new(INSPECTOR_WIDTH),
        inspector_default_width: INSPECTOR_WIDTH,
        config: Config::default(),
        settings: None,
        help: false,
        zoom: ZoomStep::Z100,
        layout: Layout::default(),
        layout_path: None,
        pending_external: false,
        native_window: None,
        live: Live::Idle,
        live_bounds: None,
        open_story: None,
        layout_written: Layout::default(),
        layout_saved_at: 0.0,
        window_size: Cell::new(None),
        restored: true,
        toast: None,
        reduced: false,
        headless,
    }
}

pub(super) fn saved_store() -> SavedView {
    let store = [11, 6, 2, 1]
        .into_iter()
        .enumerate()
        .map(|(index, id)| crate::fixtures::item(index, id))
        .collect();
    SavedView::seeded(store)
}

/// A directory of this process's own, so two test binaries never share a file.
pub(super) fn temp_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("hn-desktop-{name}-{}", std::process::id()))
}

