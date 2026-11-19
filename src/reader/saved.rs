//! Private, per-half storage shared by the terminal and desktop readers.

use super::{Article, Discussion, Profile, Provider, Source};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const VERSION: u32 = 1;
const RETENTION: u64 = 30 * 24 * 60 * 60;

pub fn state_root() -> Option<PathBuf> {
    std::env::var_os("XDG_STATE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .map(|root| root.join("hn"))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Header {
    pub version: u32,
    pub story_id: i32,
    pub saved_at: u64,
    pub provider: Provider,
    pub model: String,
    pub profile: Profile,
}

impl Header {
    pub fn new(story_id: i32, provider: Provider, profile: Profile) -> Self {
        Self {
            version: VERSION,
            story_id,
            saved_at: now(),
            provider,
            model: profile.model(provider).into(),
            profile,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Record<T> {
    #[serde(flatten)]
    pub header: Header,
    #[serde(flatten)]
    pub data: T,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ArticleData {
    pub article: Article,
    pub partial: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DiscussionData {
    pub discussion: Discussion,
    #[serde(flatten)]
    pub context: DiscussionContext,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DiscussionContext {
    pub comments_at_save: u32,
    pub coverage: String,
    pub sources: Vec<Source>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Half {
    Article,
    Discussion,
}

impl Half {
    fn suffix(self) -> &'static str {
        match self {
            Self::Article => ".article.json",
            Self::Discussion => ".discussion.json",
        }
    }

    fn filename(self, id: i32) -> String {
        format!("{id}{}", self.suffix())
    }
}

enum Backend {
    Disk(PathBuf),
    Memory {
        files: HashMap<(i32, Half), Vec<u8>>,
        now: u64,
    },
}

pub struct SummaryStore(Backend);

impl SummaryStore {
    pub fn disk(root: PathBuf) -> Self {
        Self(Backend::Disk(root))
    }

    /// A deterministic store for headless scenes.
    pub fn memory(now: u64) -> Self {
        Self(Backend::Memory {
            files: HashMap::new(),
            now,
        })
    }

    pub fn age(&self, saved_at: u64) -> u64 {
        match self.0 {
            Backend::Disk(_) => now(),
            Backend::Memory { now, .. } => now,
        }
        .saturating_sub(saved_at)
    }

    pub fn load_article(&self, id: i32) -> Option<Record<ArticleData>> {
        self.load(id, Half::Article)
    }

    pub fn load_discussion(&self, id: i32) -> Option<Record<DiscussionData>> {
        self.load(id, Half::Discussion)
    }

    pub fn save_article(
        &mut self,
        record: Record<ArticleData>,
        empty_capture: bool,
    ) -> io::Result<()> {
        if empty_capture {
            return Ok(());
        }
        self.write(Half::Article, record)
    }

    pub fn save_discussion(&mut self, record: Record<DiscussionData>) -> io::Result<()> {
        if record
            .data
            .context
            .sources
            .iter()
            .all(|source| source.excerpt.is_empty())
        {
            return Ok(());
        }
        self.write(Half::Discussion, record)
    }

    fn load<T: DeserializeOwned>(&self, id: i32, half: Half) -> Option<Record<T>> {
        let bytes;
        let data = match &self.0 {
            Backend::Disk(root) => {
                bytes = fs::read(root.join(half.filename(id))).ok()?;
                &bytes
            }
            Backend::Memory { files, .. } => files.get(&(id, half))?,
        };
        let record: Record<T> = serde_json::from_slice(data).ok()?;
        (record.header.version == VERSION && record.header.story_id == id).then_some(record)
    }

    fn write<T: Serialize>(&mut self, half: Half, record: Record<T>) -> io::Result<()> {
        let id = record.header.story_id;
        let data = serde_json::to_vec(&record)?;
        match &mut self.0 {
            Backend::Disk(root) => write_atomic(root, &half.filename(id), &data),
            Backend::Memory { files, .. } => {
                files.insert((id, half), data);
                Ok(())
            }
        }
    }

    pub fn prune(&self) {
        let Backend::Disk(root) = &self.0 else { return };
        let Ok(entries) = fs::read_dir(root) else {
            return;
        };
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
                continue;
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            for half in [Half::Article, Half::Discussion] {
                let Some(id) = name
                    .strip_suffix(half.suffix())
                    .and_then(|id| id.parse::<i32>().ok())
                else {
                    continue;
                };
                if name != half.filename(id) {
                    continue;
                }
                let saved_at = match half {
                    Half::Article => self.load_article(id).map(|record| record.header.saved_at),
                    Half::Discussion => self
                        .load_discussion(id)
                        .map(|record| record.header.saved_at),
                };
                if saved_at.is_some_and(|time| self.age(time) > RETENTION) {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
    }
}

pub(super) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn saved_label(
    seconds: u64,
    comments_at_save: Option<u32>,
    current_comments: Option<i32>,
) -> String {
    let age = match seconds {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{}m ago", seconds / 60),
        3600..=86399 => format!("{}h ago", seconds / 3600),
        _ => format!("{}d ago", seconds / 86400),
    };
    let mut label = format!("Saved {age}");
    if let (Some(saved), Some(current)) = (
        comments_at_save,
        current_comments.and_then(|n| u32::try_from(n).ok()),
    ) {
        let new = current.saturating_sub(saved);
        if new > 0 {
            label.push_str(&format!(
                " · {new} new comment{}",
                if new == 1 { "" } else { "s" }
            ));
        }
    }
    label
}

fn write_atomic(root: &Path, name: &str, data: &[u8]) -> io::Result<()> {
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    let mut dirs = fs::DirBuilder::new();
    dirs.recursive(true);
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
        dirs.mode(0o700).create(root)?;
        fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    dirs.create(root)?;
    let (temp, mut file) = loop {
        let seq = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let temp = root.join(format!(".{name}-{}-{seq}.tmp", std::process::id()));
        match options.open(&temp) {
            Ok(file) => break (temp, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(data)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, root.join(name))
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

#[cfg(test)]
#[path = "saved_tests.rs"]
pub(super) mod tests;
