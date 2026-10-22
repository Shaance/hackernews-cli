use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    #[default]
    Fast,
    Thorough,
}
impl Profile {
    pub fn label(self) -> &'static str {
        match self {
            Self::Fast => "Fast",
            Self::Thorough => "Thorough",
        }
    }
    pub fn model(self) -> &'static str {
        match self {
            Self::Fast => "anthropic/claude-haiku-5.5",
            Self::Thorough => "anthropic/claude-sonnet-5.5",
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub api_key: String,
    pub automatic: bool,
    pub profile: Profile,
    #[serde(skip)]
    pub environment_key: String,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            automatic: true,
            profile: Profile::Fast,
            environment_key: String::new(),
        }
    }
}
impl Config {
    pub fn key(&self) -> &str {
        if self.api_key.is_empty() {
            &self.environment_key
        } else {
            &self.api_key
        }
    }
    pub fn path() -> Result<PathBuf> {
        let root = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .context("Set HOME or XDG_CONFIG_HOME to save summary settings")?;
        Ok(root.join("hn/summary.json"))
    }
    pub fn load(path: &Path) -> Result<Self> {
        let mut config = match fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .context("Cannot read summary settings; fix summary.json")?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => return Err(e).context("Cannot read summary settings"),
        };
        config.environment_key = std::env::var("OPENROUTER_API_KEY")
            .unwrap_or_default()
            .trim()
            .to_owned();
        Ok(config)
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        let parent = path.parent().context("Settings need a directory")?;
        fs::create_dir_all(parent)?;
        let temp = parent.join(format!(".summary-{}.tmp", std::process::id()));
        let result = (|| -> Result<()> {
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temp)?;
            file.write_all(&serde_json::to_vec_pretty(self)?)?;
            file.sync_all()?;
            fs::rename(&temp, path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result.context("Could not save summary settings")
    }
}
