use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Lockfile {
    pub version: u32,
    pub skills: Vec<LockSkill>,
    #[serde(rename = "generatedAt")]
    pub generated_at: String,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct LockSkill {
    #[serde(rename = "installName")]
    pub install_name: String,
    pub source: Source,
    #[serde(rename = "ref")]
    pub ref_: Option<String>,
    pub commit: String,
    pub digest: String,
    #[serde(rename = "installedAt")]
    pub installed_at: String,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Source {
    pub url: String,
    pub host: String,
    pub owner: String,
    pub repo: String,
    #[serde(rename = "skillPath")]
    pub skill_path: String,
}

impl Lockfile {
    pub fn empty_now() -> Self {
        Self {
            version: 1,
            skills: vec![],
            generated_at: Utc::now().to_rfc3339(),
        }
    }
}

pub fn save_lockfile(path: &Path, lf: &Lockfile) -> Result<()> {
    let data = serde_json::to_string_pretty(lf)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, data).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}
