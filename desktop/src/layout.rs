//! What the window remembers between runs: its size, zoom, the inspector's width, and where
//! the reader was in the feed.
//!
//! The file is `$XDG_STATE_HOME/hn/desktop.json`, falling back to `~/.local/state/hn/`, which is
//! the convention the rest of the app already uses for state that is not settings: `Config` owns
//! `$XDG_CONFIG_HOME/hn/summary.json` and the Saved store owns `$XDG_DATA_HOME/hn/saved.json`.
//! State is neither, so it gets the state directory.
//!
//! A headless run never reads or writes it: a golden must not depend on the machine's window
//! size, and a golden run must not overwrite the reader's. A missing or unreadable file is the
//! defaults, never an error — losing the window's position is not worth a dialog.

use std::path::PathBuf;

use hn_lib::app::StoryType;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ZoomStep { Z75, Z80, Z90, Z100, Z110, Z125, Z150 }

impl ZoomStep {
    const ALL: [Self; 7] = [Self::Z75, Self::Z80, Self::Z90, Self::Z100, Self::Z110, Self::Z125, Self::Z150];

    pub(crate) fn scale(self) -> f32 {
        match self {
            Self::Z75 => 0.75, Self::Z80 => 0.8, Self::Z90 => 0.9, Self::Z100 => 1.0,
            Self::Z110 => 1.1, Self::Z125 => 1.25, Self::Z150 => 1.5,
        }
    }

    pub(crate) fn increase(self) -> Self {
        match self {
            Self::Z75 => Self::Z80, Self::Z80 => Self::Z90, Self::Z90 => Self::Z100,
            Self::Z100 => Self::Z110, Self::Z110 => Self::Z125, Self::Z125 | Self::Z150 => Self::Z150,
        }
    }

    pub(crate) fn decrease(self) -> Self {
        match self {
            Self::Z75 | Self::Z80 => Self::Z75, Self::Z90 => Self::Z80, Self::Z100 => Self::Z90,
            Self::Z110 => Self::Z100, Self::Z125 => Self::Z110, Self::Z150 => Self::Z125,
        }
    }

    fn from_persisted(value: Option<f32>) -> Self {
        let value = value.unwrap_or(1.0).clamp(Self::Z75.scale(), Self::Z150.scale());
        Self::ALL.into_iter().skip(1).fold(Self::Z75, |nearest, candidate| {
            if (value - candidate.scale()).abs().total_cmp(&(value - nearest.scale()).abs()).is_lt() {
                candidate
            } else {
                nearest
            }
        })
    }
}

/// The persisted window state. Every field is optional so an older file still loads.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    /// The window's size in points, when the reader resized it.
    pub window: Option<(f32, f32)>,
    /// The window's UI and article page zoom, 1.0 at actual size.
    pub zoom: Option<f32>,
    /// The inspector's width in window points, when the reader dragged the grip.
    pub inspector: Option<f32>,
    /// The feed tab that was showing.
    #[serde(default, with = "tab")]
    pub tab: Option<StoryType>,
    /// The 1-based page the feed was on.
    pub page: Option<u32>,
    /// The story that was selected, so the feed re-anchors on it.
    pub selected: Option<i32>,
}

impl Layout {
    /// Restore the nearest supported zoom, defaulting to actual size.
    pub(crate) fn zoom_level(&self) -> ZoomStep {
        ZoomStep::from_persisted(self.zoom)
    }

    /// The file's path, or `None` when neither `XDG_STATE_HOME` nor `HOME` is set.
    pub fn path() -> Option<PathBuf> {
        let root = std::env::var_os("XDG_STATE_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state"))
            })?;
        Some(root.join("hn/desktop.json"))
    }

    /// Read the file at `path`. A missing, unreadable or malformed file is the defaults.
    pub fn load(path: &std::path::Path) -> Layout {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Write the file at `path`, creating its directory. A failure is reported, not fatal.
    pub fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        let Some(parent) = path.parent() else {
            return Ok(());
        };
        std::fs::create_dir_all(parent)?;
        let temp = parent.join(format!(".desktop-{}.tmp", std::process::id()));
        std::fs::write(&temp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&temp, path)
    }

    /// Whether anything worth writing has been recorded.
    pub fn is_empty(&self) -> bool {
        *self == Layout::default()
    }
}

/// `StoryType` is `hn_lib`'s and has no serde impls, so the file spells the tab out.
mod tab {
    use super::{Layout, StoryType};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(tab: &Option<StoryType>, s: S) -> Result<S::Ok, S::Error> {
        match tab {
            Some(tab) => s.serialize_some(Layout::tab_name(*tab)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<StoryType>, D::Error> {
        let name = Option::<String>::deserialize(d)?;
        Ok(name.as_deref().and_then(Layout::tab_from_name))
    }
}

impl Layout {
    pub fn tab_name(tab: StoryType) -> &'static str {
        match tab {
            StoryType::Top => "top",
            StoryType::New => "new",
            StoryType::Best => "best",
            StoryType::Saved => "saved",
        }
    }

    pub fn tab_from_name(name: &str) -> Option<StoryType> {
        match name {
            "top" => Some(StoryType::Top),
            "new" => Some(StoryType::New),
            "best" => Some(StoryType::Best),
            "saved" => Some(StoryType::Saved),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_is_the_defaults() {
        let dir = std::env::temp_dir().join(format!("hn-layout-{}", std::process::id()));
        let path = dir.join("desktop.json");
        let _ = std::fs::remove_file(&path);
        assert_eq!(Layout::load(&path), Layout::default());
    }

    #[test]
    fn a_round_trip_keeps_every_field() {
        let dir = std::env::temp_dir().join(format!("hn-layout-rt-{}", std::process::id()));
        let path = dir.join("hn/desktop.json");
        let layout = Layout {
            window: Some((1280.0, 820.0)),
            zoom: Some(1.25),
            inspector: Some(512.0),
            tab: Some(StoryType::Best),
            page: Some(3),
            selected: Some(42),
        };
        layout.save(&path).expect("the temp directory is writable");
        assert!(std::fs::read_to_string(&path).unwrap().contains("\"zoom\": 1.25"));
        assert_eq!(Layout::load(&path), layout);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn zoom_defaults_and_snaps_to_the_nearest_step() {
        let missing: Layout = serde_json::from_str(r#"{"window":[1100,760]}"#).unwrap();
        assert_eq!(missing.zoom, None);
        assert_eq!(missing.zoom_level(), ZoomStep::Z100);
        for (value, expected) in [(0.3, ZoomStep::Z75), (0.87, ZoomStep::Z90),
            (1.18, ZoomStep::Z125), (1.375, ZoomStep::Z125), (2.0, ZoomStep::Z150)] {
            let layout: Layout = serde_json::from_str(&format!(r#"{{"zoom":{value}}}"#)).unwrap();
            assert_eq!(layout.zoom_level(), expected);
        }
    }

    #[test]
    fn a_malformed_file_is_the_defaults_not_an_error() {
        let dir = std::env::temp_dir().join(format!("hn-layout-bad-{}", std::process::id()));
        let path = dir.join("desktop.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, b"{ not json").unwrap();
        assert_eq!(Layout::load(&path), Layout::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_tab_names_round_trip() {
        for tab in [StoryType::Top, StoryType::New, StoryType::Best, StoryType::Saved] {
            assert_eq!(Layout::tab_from_name(Layout::tab_name(tab)), Some(tab));
        }
        assert_eq!(Layout::tab_from_name("nope"), None);
    }
}
