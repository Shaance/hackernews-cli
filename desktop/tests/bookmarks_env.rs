//! The Saved store's real path, isolated to a temp `XDG_DATA_HOME`.
//!
//! One test in its own binary: `std::env::set_var` is process-global, so a second test in the
//! same process could see the temp root, or race the write. `hn_lib::bookmarks::default_path`
//! reads `XDG_DATA_HOME` first, which is what makes the isolation possible without touching the
//! user's `~/.local/share/hn/saved.json`.

use hn_desktop::bookmarks::SavedView;
use hn_desktop::fixtures;

#[test]
fn the_default_path_follows_xdg_data_home_and_a_corrupt_file_banners_until_reload() {
    let root = std::env::temp_dir().join(format!("hn-desktop-xdg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    // SAFETY: this binary holds exactly one test, so no other thread reads the environment.
    unsafe { std::env::set_var("XDG_DATA_HOME", &root) };

    let path = root.join("hn/saved.json");

    // A missing file is an empty store, not an error.
    let view = SavedView::load();
    assert_eq!(view.path(), Some(path.as_path()), "the store lives under XDG_DATA_HOME");
    assert!(view.error().is_none(), "a missing file is not an error");
    assert!(view.store().is_empty());

    // A corrupt file is a banner, not a silently empty list.
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"{ not json").unwrap();
    let mut view = SavedView::load();
    assert!(view.error().is_some(), "a corrupt file raises the banner");
    assert!(view.store().is_empty());

    // Reload after the file is repaired clears it.
    std::fs::write(&path, b"[]").unwrap();
    view.reload();
    assert!(view.error().is_none(), "Reload clears the banner");

    // A toggle writes through the same path and survives a fresh load.
    let mut view = SavedView::load();
    assert!(view.toggle(&fixtures::item(0, 1)));
    assert!(view.error().is_none());
    let reloaded = SavedView::load();
    assert_eq!(reloaded.ids(), vec![1], "the save landed in the temp store");

    let _ = std::fs::remove_dir_all(&root);
}
