use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserConfig {
    pub default_root: String, // e.g., "./skills"
    pub protocol: String,     // "ssh" | "https"
    pub default_host: String, // e.g., "github.com"
    pub github_user: String,
}

impl Default for UserConfig {
    fn default() -> Self {
        Self {
            default_root: "./skills".to_string(),
            protocol: "ssh".to_string(),
            default_host: "github.com".to_string(),
            github_user: String::new(),
        }
    }
}

pub fn config_dir() -> Result<PathBuf> {
    let pd = ProjectDirs::from("", "", "sk").context("unable to determine config dir")?;
    Ok(pd.config_dir().to_path_buf())
}

pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.json"))
}

pub fn load_or_default() -> Result<UserConfig> {
    let path = config_path()?;
    if path.exists() {
        let data = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        let cfg: UserConfig =
            serde_json::from_slice(&data).with_context(|| format!("parsing {}", path.display()))?;
        Ok(cfg)
    } else {
        Ok(UserConfig::default())
    }
}

pub fn save_if_missing(cfg: &UserConfig) -> Result<()> {
    let path = config_path()?;
    if !path.exists() {
        save(cfg)?;
    }
    Ok(())
}

pub fn save(cfg: &UserConfig) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let pretty = serde_json::to_string_pretty(cfg)?;
    fs::write(&path, pretty).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}
