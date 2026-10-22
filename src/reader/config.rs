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
    pub fn model(self, provider: Provider) -> &'static str {
        match (provider, self) {
            (Provider::OpenRouter, Self::Fast) => "anthropic/claude-haiku-5.5",
            (Provider::OpenRouter, Self::Thorough) => "anthropic/claude-sonnet-5.5",
            (Provider::Anthropic, Self::Fast) => "claude-haiku-5-5",
            (Provider::Anthropic, Self::Thorough) => "claude-sonnet-5-5",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    #[default]
    OpenRouter,
    Anthropic,
}
impl Provider {
    pub fn label(self) -> &'static str {
        match self {
            Self::OpenRouter => "OpenRouter",
            Self::Anthropic => "Anthropic",
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub provider: Provider,
    pub anthropic_api_key: String,
    #[serde(skip)]
    pub anthropic_environment_key: String,
    pub api_key: String,
    pub automatic: bool,
    pub profile: Profile,
    #[serde(skip)]
    pub environment_key: String,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            provider: Provider::default(),
            anthropic_api_key: String::new(),
            anthropic_environment_key: String::new(),
            api_key: String::new(),
            automatic: true,
            profile: Profile::Fast,
            environment_key: String::new(),
        }
    }
}
impl Config {
    pub fn key(&self) -> &str {
        self.key_for(self.provider)
    }
    pub fn key_for(&self, provider: Provider) -> &str {
        let (saved, environment) = match provider {
            Provider::OpenRouter => (&self.api_key, &self.environment_key),
            Provider::Anthropic => (&self.anthropic_api_key, &self.anthropic_environment_key),
        };
        if saved.is_empty() {
            environment
        } else {
            saved
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
        let mut fresh = false;
        let mut config = match fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .context("Cannot read summary settings; fix summary.json")?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                fresh = true;
                Self::default()
            }
            Err(e) => return Err(e).context("Cannot read summary settings"),
        };
        config.environment_key = std::env::var("OPENROUTER_API_KEY")
            .unwrap_or_default()
            .trim()
            .to_owned();
        config.anthropic_environment_key = std::env::var("ANTHROPIC_API_KEY")
            .unwrap_or_default()
            .trim()
            .to_owned();
        if fresh
            && config.key_for(Provider::OpenRouter).is_empty()
            && !config.key_for(Provider::Anthropic).is_empty()
        {
            config.provider = Provider::Anthropic;
        }
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
