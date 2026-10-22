//! The local saved-story file. A failed write never replaces the previous file.
use crate::{
    app::{App, StoryType},
    HNCLIItem,
};
use anyhow::{Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn default_path() -> Result<PathBuf> {
    let root = std::env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .context("Set HOME or XDG_DATA_HOME to store saved stories")?;
    Ok(root.join("hn/saved.json"))
}

pub fn load(path: &Path) -> Result<Vec<HNCLIItem>> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .with_context(|| format!("Cannot read saved stories at {}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error).with_context(|| format!("Cannot read {}", path.display())),
    }
}

pub fn save(path: &Path, stories: &[HNCLIItem]) -> Result<()> {
    let parent = path
        .parent()
        .context("Saved-story path needs a directory")?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(".saved-{}.tmp", std::process::id()));
    let result = (|| -> Result<()> {
        fs::write(&temp, serde_json::to_vec_pretty(stories)?)?;
        fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.with_context(|| format!("Could not save stories at {}", path.display()))
}

pub fn toggle(app: &mut App, path: Option<&Path>) -> Result<()> {
    let path = path.context("Saved stories unavailable. Open Saved (4) and press r to retry")?;
    let Some(story) = app.bookmark_story().cloned() else {
        return Ok(());
    };
    let mut next = app.bookmarks.clone();
    if let Some(index) = next.iter().position(|saved| saved.id == story.id) {
        next.remove(index);
    } else {
        next.insert(0, story);
    }
    save(path, &next)?;
    app.bookmarks = next;
    app.bookmark_notice = None;
    if app.story_type == StoryType::Saved {
        app.show_saved_stories();
    }
    Ok(())
}

pub fn reload(app: &mut App) -> Option<PathBuf> {
    match default_path().and_then(|path| Ok((load(&path)?, path))) {
        Ok((stories, path)) => {
            app.bookmarks = stories;
            app.bookmark_notice = None;
            Some(path)
        }
        Err(error) => {
            app.bookmark_notice = Some(format!("{error}. Saved: 4, r retry"));
            None
        }
    }
}
