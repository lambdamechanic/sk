use anyhow::{bail, Context, Result};
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
    #[allow(dead_code)]
    #[serde(default, rename = "ref", skip_serializing)]
    pub legacy_ref: Option<String>,
    pub commit: String,
    pub digest: String,
    #[serde(rename = "installedAt")]
    pub installed_at: String,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Source {
    #[serde(flatten)]
    pub spec: crate::git::RepoSpec,
    #[serde(rename = "skillPath")]
    pub skill_path: String,
}

impl Source {
    pub fn repo_spec(&self) -> &crate::git::RepoSpec {
        &self.spec
    }

    pub fn repo_spec_owned(&self) -> crate::git::RepoSpec {
        self.spec.clone()
    }

    pub fn skill_path(&self) -> &str {
        &self.skill_path
    }
}

impl Lockfile {
    pub fn empty_now() -> Self {
        Self {
            version: 1,
            skills: vec![],
            generated_at: Utc::now().to_rfc3339(),
        }
    }

    pub fn assert_no_legacy_refs(&self) -> Result<()> {
        if let Some(entry) = self.skills.iter().find(|s| s.legacy_ref.is_some()) {
            bail!(
                "skills.lock.json still contains \"ref\" for install '{}'. The field is obsolete—remove the \"ref\" key (or reinstall the skill) and re-run sk.",
                entry.install_name
            );
        }
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self> {
        let data = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let lf: Lockfile =
            serde_json::from_slice(&data).with_context(|| format!("parsing {}", path.display()))?;
        lf.assert_no_legacy_refs()?;
        Ok(lf)
    }

    pub fn load_or_empty(path: &Path) -> Result<Self> {
        if path.exists() {
            Self::load(path)
        } else {
            Ok(Self::empty_now())
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
