//! The Saved store: `hn_lib::bookmarks`' file format, plus the desktop's own error surface.
//!
//! The file, its atomic write, and the toggle rule are the shared ones. What the desktop adds
//! is a view that can be *in memory* (headless goldens must not read the user's real
//! `saved.json`) and a corrupt-file state the Saved view renders as a banner with Reload.

use std::path::{Path, PathBuf};

use hn_lib::bookmarks::{self, SavedStore};
use hn_lib::HNCLIItem;

pub struct SavedView {
    /// `None` means in-memory only: toggles work, nothing is written or read.
    path: Option<PathBuf>,
    store: SavedStore,
    /// The last load or save failure, shown as the Saved view's banner.
    error: Option<String>,
}

impl SavedView {
    /// The user's real store, at `hn_lib::bookmarks::default_path()`.
    pub fn load() -> SavedView {
        match bookmarks::default_path() {
            Ok(path) => SavedView::load_at(path),
            Err(error) => SavedView {
                path: None,
                store: Vec::new(),
                error: Some(format!("{error:#}")),
            },
        }
    }

    pub fn load_at(path: PathBuf) -> SavedView {
        let mut view = SavedView {
            path: Some(path),
            store: Vec::new(),
            error: None,
        };
        view.reload();
        view
    }

    /// A store that never touches disk. Headless runs and tests use this so a golden never
    /// depends on the machine it runs on.
    pub fn in_memory() -> SavedView {
        SavedView {
            path: None,
            store: Vec::new(),
            error: None,
        }
    }

    /// A store that already holds `store`, still in memory only. A scene uses it to show the
    /// Saved view without reading the machine's file.
    pub fn seeded(store: SavedStore) -> SavedView {
        SavedView {
            path: None,
            store,
            error: None,
        }
    }

    /// The banner state: a file that could not be read. A scene uses it to render the banner
    /// without writing a corrupt file first.
    pub fn corrupt(path: PathBuf, message: impl Into<String>) -> SavedView {
        SavedView {
            path: Some(path),
            store: Vec::new(),
            error: Some(message.into()),
        }
    }

    /// Re-read the file. A corrupt file keeps the previous store and raises the banner.
    pub fn reload(&mut self) {
        let Some(path) = self.path.clone() else {
            self.error = None;
            return;
        };
        match bookmarks::load(&path) {
            Ok(store) => {
                self.store = store;
                self.error = None;
            }
            Err(error) => self.error = Some(format!("{error:#}")),
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn path_label(&self) -> String {
        match self.path() {
            Some(path) => path.display().to_string(),
            None => "in memory".to_string(),
        }
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn store(&self) -> &[HNCLIItem] {
        &self.store
    }

    /// The Saved tab's ranking: save-time order, newest first.
    pub fn ids(&self) -> Vec<i32> {
        self.store.iter().map(|story| story.id).collect()
    }

    pub fn contains(&self, id: i32) -> bool {
        self.store.iter().any(|story| story.id == id)
    }

    /// Toggle `item`, returning whether it is saved afterwards. A failed write leaves the
    /// previous file in place and raises the banner.
    pub fn toggle(&mut self, item: &HNCLIItem) -> bool {
        let mut next = self.store.clone();
        let saved = bookmarks::toggle_saved(&mut next, item);
        if let Some(path) = self.path.clone() {
            if let Err(error) = bookmarks::save(&path, &next) {
                self.error = Some(format!("{error:#}"));
                return self.contains(item.id);
            }
        }
        self.store = next;
        self.error = None;
        saved
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hn-desktop-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_in_memory_store_toggles_without_touching_disk() {
        let mut view = SavedView::in_memory();
        assert!(view.store().is_empty());
        assert!(view.toggle(&fixtures::item(0, 1)));
        assert!(view.contains(1));
        assert_eq!(view.ids(), vec![1]);
        assert!(!view.toggle(&fixtures::item(0, 1)));
        assert!(!view.contains(1));
        assert_eq!(view.path_label(), "in memory");
    }

    #[test]
    fn the_store_is_newest_first() {
        let mut view = SavedView::in_memory();
        view.toggle(&fixtures::item(0, 1));
        view.toggle(&fixtures::item(1, 2));
        view.toggle(&fixtures::item(2, 3));
        assert_eq!(view.ids(), vec![3, 2, 1], "the last save leads");
    }

    #[test]
    fn a_missing_file_loads_empty_and_a_round_trip_survives_reload() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("hn/saved.json");
        let mut view = SavedView::load_at(path.clone());
        assert!(view.error().is_none(), "a missing file is not an error");
        assert!(view.store().is_empty());
        view.toggle(&fixtures::item(0, 1));
        view.toggle(&fixtures::item(1, 2));
        let reloaded = SavedView::load_at(path);
        assert_eq!(reloaded.ids(), vec![2, 1]);
        assert!(reloaded.error().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_corrupt_file_raises_the_banner_and_keeps_the_previous_store() {
        let dir = temp_dir("corrupt");
        let path = dir.join("hn/saved.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{ not json").unwrap();
        let mut view = SavedView::load_at(path.clone());
        assert!(view.error().is_some(), "a corrupt file is a banner, not a panic");
        assert!(view.store().is_empty());
        // Reload after the file is repaired clears the banner.
        std::fs::write(&path, b"[]").unwrap();
        view.reload();
        assert!(view.error().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_write_keeps_the_previous_file_and_raises_the_banner() {
        let dir = temp_dir("readonly");
        let path = dir.join("hn/saved.json");
        let mut view = SavedView::load_at(path.clone());
        view.toggle(&fixtures::item(0, 1));
        // Make the parent a file so the atomic write cannot create its temp file.
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path.parent().unwrap(), b"blocked").unwrap();
        let saved = view.toggle(&fixtures::item(1, 2));
        assert!(!saved, "the toggle did not land");
        assert!(view.error().is_some());
        assert_eq!(view.ids(), vec![1], "the in-memory store is unchanged");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
