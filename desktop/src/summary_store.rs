//! Private, atomic persistence for provider results and their citation sources.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use hn_lib::reader::{Article, Discussion, Source};
use serde::{Deserialize, Serialize};

use crate::story_io::{Provenance, StoryUpdate};

const VERSION: u32 = 1;
const RETENTION: u64 = 30 * 24 * 60 * 60;

#[derive(Serialize, Deserialize)]
pub(crate) struct SavedSummary {
    pub version: u32,
    pub story_id: i32,
    pub article: Option<SavedArticle>,
    pub discussion: Option<SavedDiscussion>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct SavedArticle {
    pub saved_at: u64,
    #[serde(flatten)]
    pub provenance: Provenance,
    #[serde(flatten)]
    pub article: Article,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct SavedDiscussion {
    pub saved_at: u64,
    pub comments_at_save: u32,
    #[serde(flatten)]
    pub provenance: Provenance,
    #[serde(flatten)]
    pub discussion: Discussion,
    pub sources: Vec<Source>,
}

enum Backend {
    Disk(PathBuf),
    Memory(HashMap<i32, Vec<u8>>),
}

pub(crate) struct SummaryStore {
    backend: Backend,
    clock: Option<u64>,
}

pub(crate) enum PendingSave {
    Article { story_id: i32, provenance: Provenance, article: Article },
    Discussion { story_id: i32, provenance: Provenance, comments: u32, discussion: Discussion, sources: Vec<Source> },
}

impl SummaryStore {
    pub(crate) fn disk(path: PathBuf) -> Self {
        Self { backend: Backend::Disk(path), clock: None }
    }

    pub(crate) fn memory(now: u64) -> Self {
        Self { backend: Backend::Memory(HashMap::new()), clock: Some(now) }
    }

    fn now(&self) -> u64 {
        self.clock.unwrap_or_else(|| std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs())
    }

    pub(crate) fn age(&self, saved_at: u64) -> u64 {
        self.now().saturating_sub(saved_at)
    }

    /// Copy a provider result before landing consumes it; only a landed result is saved.
    pub(crate) fn prepare(update: &StoryUpdate, comments: u32) -> Option<PendingSave> {
        Some(match update {
            StoryUpdate::ArticleSummary { story_id, summary: Ok(article), provenance, empty_capture: false } =>
                PendingSave::Article { story_id: *story_id, provenance: provenance.clone(), article: article.clone() },
            StoryUpdate::Summary { story_id, summary: Ok((discussion, sources)), provenance }
                if sources.iter().any(|source| !source.excerpt.is_empty()) =>
                PendingSave::Discussion { story_id: *story_id, provenance: provenance.clone(), comments,
                    discussion: discussion.clone(), sources: sources.clone() },
            _ => return None,
        })
    }

    pub(crate) fn save(&mut self, pending: PendingSave) {
        let story_id = match &pending {
            PendingSave::Article { story_id, .. } | PendingSave::Discussion { story_id, .. } => *story_id,
        };
        let mut saved = self.load(story_id).unwrap_or(SavedSummary {
            version: VERSION, story_id, article: None, discussion: None,
        });
        let saved_at = self.now();
        match pending {
            PendingSave::Article { provenance, article, .. } =>
                saved.article = Some(SavedArticle { saved_at, provenance, article }),
            PendingSave::Discussion { provenance, comments, discussion, sources, .. } =>
                saved.discussion = Some(SavedDiscussion { saved_at, comments_at_save: comments, provenance, discussion, sources }),
        }
        if let Err(error) = self.write(&saved) { eprintln!("summary {story_id}: write failed: {error}"); }
    }

    pub(crate) fn load(&self, story_id: i32) -> Option<SavedSummary> {
        let data = match &self.backend {
            Backend::Memory(files) => files.get(&story_id).cloned(),
            Backend::Disk(root) => match std::fs::read(root.join(format!("{story_id}.json"))) {
                Ok(bytes) => Some(bytes),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => { eprintln!("summary {story_id}: ignored: {error}"); None }
            },
        }?;
        let saved: SavedSummary = match serde_json::from_slice(&data) {
            Ok(saved) => saved,
            Err(error) => { eprintln!("summary {story_id}: ignored: {error}"); return None; }
        };
        if saved.version != VERSION || saved.story_id != story_id {
            eprintln!("summary {story_id}: ignored: invalid version or story ID");
            return None;
        }
        Some(saved)
    }

    pub(crate) fn prune(&mut self) {
        let Backend::Disk(root) = &self.backend else { return; };
        let entries = match std::fs::read_dir(root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => { eprintln!("summaries: ignored: {error}"); return; }
        };
        let root = root.clone();
        for entry in entries {
            let entry = match entry { Ok(entry) => entry, Err(error) => { eprintln!("summaries: ignored: {error}"); continue; } };
            let name = entry.file_name();
            let Some(id) = name.to_str().and_then(|name| name.strip_suffix(".json")).and_then(|id| id.parse::<i32>().ok()) else { continue; };
            let Some(mut saved) = self.load(id) else { continue; };
            let old_article = saved.article.as_ref().is_some_and(|part| self.age(part.saved_at) > RETENTION);
            let old_discussion = saved.discussion.as_ref().is_some_and(|part| self.age(part.saved_at) > RETENTION);
            if old_article { saved.article = None; }
            if old_discussion { saved.discussion = None; }
            if !old_article && !old_discussion { continue; }
            let result = if saved.article.is_none() && saved.discussion.is_none() {
                std::fs::remove_file(root.join(format!("{id}.json")))
            } else {
                self.write(&saved)
            };
            if let Err(error) = result { eprintln!("summary {id}: ignored: {error}"); }
        }
    }

    pub(crate) fn write(&mut self, saved: &SavedSummary) -> std::io::Result<()> {
        let data = serde_json::to_vec_pretty(saved)?;
        match &mut self.backend {
            Backend::Memory(files) => { files.insert(saved.story_id, data); Ok(()) }
            Backend::Disk(root) => write_atomic(root, saved.story_id, &data),
        }
    }
}

fn write_atomic(root: &Path, story_id: i32, data: &[u8]) -> std::io::Result<()> {
    let mut dirs = std::fs::DirBuilder::new();
    dirs.recursive(true);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        dirs.mode(0o700);
        options.mode(0o600);
    }
    dirs.create(root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))?;
    }
    let temp = root.join(format!(".{story_id}-{}.tmp", std::process::id()));
    if let Err(error) = std::fs::remove_file(&temp) && error.kind() != std::io::ErrorKind::NotFound {
        return Err(error);
    }
    let mut file = options.open(&temp)?;
    let result = (|| {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(data)?;
        file.sync_all()?;
        std::fs::rename(&temp, root.join(format!("{story_id}.json")))
    })();
    if result.is_err() { let _ = std::fs::remove_file(temp); }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use hn_lib::reader::{Config, Point};

    fn sample(id: i32, article_at: u64, discussion_at: u64) -> SavedSummary {
        let provenance = Provenance::from(&Config::default());
        SavedSummary {
            version: 1,
            story_id: id,
            article: Some(SavedArticle {
                saved_at: article_at,
                provenance: provenance.clone(),
                article: Article { text: "Article text".into() },
            }),
            discussion: Some(SavedDiscussion {
                saved_at: discussion_at,
                comments_at_save: 42,
                provenance,
                discussion: Discussion {
                    overview: "Overview".into(),
                    points: vec![Point { text: "A point".into(), sources: vec![7] }],
                },
                sources: vec![Source {
                    id: 7, parent: Some(3), author: "alice".into(), text: "full comment".into(), excerpt: "excerpt".into(),
                }],
            }),
        }
    }

    #[test]
    fn reader_results_and_all_citation_fields_round_trip() {
        let root = std::env::temp_dir().join(format!("hn-summary-round-trip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut store = SummaryStore::disk(root.clone());
        store.clock = Some(1_700_000_000);
        let saved = sample(123, 1_700_000_000, 1_700_000_000);
        store.write(&saved).unwrap();
        let file = std::fs::read_to_string(root.join("123.json")).unwrap();
        println!("Saved summary example:\n{file}");
        let actual = serde_json::to_value(store.load(123).unwrap()).unwrap();
        let expected = serde_json::json!({
            "version": 1, "story_id": 123,
            "article": { "saved_at": 1700000000, "provider": "OpenRouter",
                "model": "anthropic/claude-haiku-5.5", "profile": "Fast", "text": "Article text" },
            "discussion": { "saved_at": 1700000000, "comments_at_save": 42,
                "provider": "OpenRouter", "model": "anthropic/claude-haiku-5.5", "profile": "Fast",
                "overview": "Overview", "points": [{ "text": "A point", "sources": [7] }],
                "sources": [{ "id": 7, "parent": 3, "author": "alice", "text": "full comment", "excerpt": "excerpt" }] }
        });
        assert_eq!(actual, expected);
        assert_eq!(serde_json::from_str::<serde_json::Value>(&file).unwrap(), expected);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn retention_removes_expired_halves_and_deletes_empty_files() {
        let root = std::env::temp_dir().join(format!("hn-summary-retention-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut store = SummaryStore::disk(root.clone());
        store.clock = Some(1_700_000_000);
        let fresh = 1_700_000_000 - RETENTION;
        let old = fresh - 1;
        store.write(&sample(1, old, old)).unwrap();
        store.write(&sample(2, old, fresh)).unwrap();
        store.write(&sample(3, fresh, fresh)).unwrap();
        store.prune();
        assert!(!root.join("1.json").exists());
        let partial = store.load(2).unwrap();
        assert!(partial.article.is_none() && partial.discussion.is_some());
        let kept = store.load(3).unwrap();
        assert!(kept.article.is_some() && kept.discussion.is_some());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn disk_directory_and_files_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("hn-summary-mode-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut store = SummaryStore::disk(root.clone());
        store.write(&sample(1, 1_700_000_000, 1_700_000_000)).unwrap();
        assert_eq!(std::fs::metadata(&root).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(std::fs::metadata(root.join("1.json")).unwrap().permissions().mode() & 0o777, 0o600);
        assert!(!root.join(format!(".1-{}.tmp", std::process::id())).exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
