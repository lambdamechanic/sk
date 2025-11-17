use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Lockfile {
    pub version: u32,
    #[serde(default)]
    pub repos: RepoRegistry,
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

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct RepoRegistry {
    #[serde(default)]
    pub entries: Vec<RepoEntry>,
    #[serde(rename = "updatedAt")]
    pub updated_at: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct RepoEntry {
    #[serde(rename = "key")]
    pub key: String,
    pub alias: String,
    #[serde(flatten)]
    pub spec: crate::git::RepoSpec,
    #[serde(rename = "addedAt")]
    pub added_at: String,
}

#[derive(Clone, Debug)]
pub struct Source {
    repo_key: String,
    skill_path: String,
    spec: Option<crate::git::RepoSpec>,
}

impl Source {
    pub fn new(spec: crate::git::RepoSpec, skill_path: String) -> Self {
        Self {
            repo_key: repo_key(&spec),
            skill_path,
            spec: Some(spec),
        }
    }

    pub fn repo_key(&self) -> &str {
        &self.repo_key
    }

    pub fn repo_spec(&self) -> &crate::git::RepoSpec {
        self.spec
            .as_ref()
            .expect("lock source repo spec has not been hydrated")
    }

    pub fn repo_spec_owned(&self) -> crate::git::RepoSpec {
        self.repo_spec().clone()
    }

    pub fn skill_path(&self) -> &str {
        &self.skill_path
    }

    pub fn set_spec(&mut self, spec: crate::git::RepoSpec) {
        self.spec = Some(spec);
    }
}

impl RepoRegistry {
    pub fn touch(&mut self) {
        self.updated_at = Some(Utc::now().to_rfc3339());
    }

    pub fn entry_by_key(&self, key: &str) -> Option<&RepoEntry> {
        self.entries.iter().find(|entry| entry.key == key)
    }

    pub fn entry_by_key_mut(&mut self, key: &str) -> Option<&mut RepoEntry> {
        self.entries.iter_mut().find(|entry| entry.key == key)
    }

    pub fn entry_by_alias(&self, alias: &str) -> Option<&RepoEntry> {
        self.entries.iter().find(|entry| entry.alias == alias)
    }

    pub fn remove_by_alias(&mut self, alias: &str) -> Option<RepoEntry> {
        if let Some(idx) = self.entries.iter().position(|entry| entry.alias == alias) {
            self.touch();
            Some(self.entries.remove(idx))
        } else {
            None
        }
    }

    pub fn remove_by_key(&mut self, key: &str) -> Option<RepoEntry> {
        if let Some(idx) = self.entries.iter().position(|entry| entry.key == key) {
            self.touch();
            Some(self.entries.remove(idx))
        } else {
            None
        }
    }

    pub fn insert_if_missing(
        &mut self,
        spec: &crate::git::RepoSpec,
        alias: Option<String>,
        added_at: Option<String>,
    ) {
        let key = repo_key(spec);
        let mut updated_existing = false;
        let mut found_existing = false;
        {
            if let Some(entry) = self.entry_by_key_mut(&key) {
                found_existing = true;
                if entry.spec.url.is_empty() {
                    entry.spec = spec.clone();
                    updated_existing = true;
                }
                if let Some(ref a) = alias {
                    if entry.alias != *a {
                        entry.alias = a.clone();
                        updated_existing = true;
                    }
                }
            }
        }
        if found_existing {
            if updated_existing {
                self.touch();
            }
            return;
        }
        let alias_value = alias.unwrap_or_else(|| default_alias(spec));
        self.entries.push(RepoEntry {
            key,
            alias: alias_value,
            spec: spec.clone(),
            added_at: added_at.unwrap_or_else(|| Utc::now().to_rfc3339()),
        });
        self.touch();
    }

    pub fn backfill_from_skills(&mut self, skills: &[LockSkill]) {
        for skill in skills {
            if let Some(spec) = &skill.source.spec {
                let added_at = Some(skill.installed_at.clone());
                self.insert_if_missing(spec, None, added_at);
            }
        }
    }
}

impl RepoEntry {
    pub fn repo_key(&self) -> &str {
        &self.key
    }
}

impl Serialize for Source {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[derive(Serialize)]
        struct SourceOut<'a> {
            #[serde(rename = "repoKey")]
            repo_key: &'a str,
            #[serde(rename = "skillPath")]
            skill_path: &'a str,
        }
        let helper = SourceOut {
            repo_key: &self.repo_key,
            skill_path: &self.skill_path,
        };
        helper.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Source {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum SourceSerde {
            Legacy {
                url: String,
                host: String,
                owner: String,
                repo: String,
                #[serde(rename = "skillPath")]
                skill_path: String,
            },
            Current {
                #[serde(rename = "repoKey")]
                repo_key: String,
                #[serde(rename = "skillPath")]
                skill_path: String,
            },
        }

        match SourceSerde::deserialize(deserializer)? {
            SourceSerde::Legacy {
                url,
                host,
                owner,
                repo,
                skill_path,
            } => {
                let spec = crate::git::RepoSpec {
                    url,
                    host,
                    owner,
                    repo,
                };
                Ok(Source {
                    repo_key: repo_key(&spec),
                    skill_path,
                    spec: Some(spec),
                })
            }
            SourceSerde::Current {
                repo_key,
                skill_path,
            } => Ok(Source {
                repo_key,
                skill_path,
                spec: None,
            }),
        }
    }
}

impl Lockfile {
    pub fn empty_now() -> Self {
        Self {
            version: 1,
            repos: RepoRegistry::default(),
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
        let mut lf: Lockfile =
            serde_json::from_slice(&data).with_context(|| format!("parsing {}", path.display()))?;
        lf.assert_no_legacy_refs()?;
        if let Some(parent) = path.parent() {
            let legacy_path = parent.join("skills.repos.json");
            lf.import_legacy_registry(path, &legacy_path)?;
        }
        lf.repos.backfill_from_skills(&lf.skills);
        lf.hydrate_sources()?;
        Ok(lf)
    }

    pub fn load_or_empty(path: &Path) -> Result<Self> {
        if path.exists() {
            Self::load(path)
        } else {
            let mut lf = Self::empty_now();
            if let Some(parent) = path.parent() {
                let legacy_path = parent.join("skills.repos.json");
                lf.import_legacy_registry(path, &legacy_path)?;
            }
            Ok(lf)
        }
    }

    fn hydrate_sources(&mut self) -> Result<()> {
        let mut catalog: HashMap<String, crate::git::RepoSpec> = HashMap::new();
        for entry in &self.repos.entries {
            catalog.insert(entry.key.clone(), entry.spec.clone());
        }
        let mut missing: Vec<String> = Vec::new();
        for skill in &mut self.skills {
            if skill.source.spec.is_some() {
                continue;
            }
            if let Some(spec) = catalog.get(skill.source.repo_key()) {
                skill.source.set_spec(spec.clone());
            } else {
                missing.push(skill.install_name.clone());
            }
        }
        if !missing.is_empty() {
            bail!(
                "Lockfile missing repo metadata for: {}. Try reinstalling those skills.",
                missing.join(", ")
            );
        }
        Ok(())
    }

    pub fn ensure_repo_entry(&mut self, spec: &crate::git::RepoSpec) {
        self.repos.insert_if_missing(spec, None, None)
    }

    fn import_legacy_registry(&mut self, lock_path: &Path, legacy_path: &Path) -> Result<()> {
        if !legacy_path.exists() {
            return Ok(());
        }
        let data =
            fs::read(legacy_path).with_context(|| format!("reading {}", legacy_path.display()))?;
        let legacy: LegacyRepoRegistry = serde_json::from_slice(&data)
            .with_context(|| format!("parsing {}", legacy_path.display()))?;
        let mut imported = false;
        for entry in legacy.repos {
            self.repos
                .insert_if_missing(&entry.spec, Some(entry.alias), Some(entry.added_at));
            imported = true;
        }
        if imported {
            save_lockfile(lock_path, self)?;
            let _ = fs::remove_file(legacy_path);
        }
        Ok(())
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

pub fn edit_lockfile<F, T>(path: &Path, mutate: F) -> Result<T>
where
    F: FnOnce(&mut Lockfile) -> Result<T>,
{
    let existed = path.exists();
    let mut lf = if existed {
        Lockfile::load(path)?
    } else {
        Lockfile::empty_now()
    };
    let before = if existed {
        Some(serde_json::to_vec(&lf)?)
    } else {
        None
    };
    let result = mutate(&mut lf)?;
    let after = serde_json::to_vec(&lf)?;
    let changed = match before {
        Some(bytes) => bytes != after,
        None => true,
    };
    if changed {
        save_lockfile(path, &lf)?;
    }
    Ok(result)
}

pub fn repo_key(spec: &crate::git::RepoSpec) -> String {
    format!("{}/{}/{}", spec.host, spec.owner, spec.repo)
}

fn default_alias(spec: &crate::git::RepoSpec) -> String {
    let base = if spec.owner.is_empty() {
        spec.repo.clone()
    } else {
        format!("{}/{}", spec.owner, spec.repo)
    };
    if spec.host.is_empty() || spec.host == "github.com" {
        base
    } else {
        format!("{}:{base}", spec.host)
    }
}

#[derive(Deserialize)]
struct LegacyRepoRegistry {
    repos: Vec<LegacyRepoEntry>,
}

#[derive(Deserialize)]
struct LegacyRepoEntry {
    alias: String,
    spec: crate::git::RepoSpec,
    #[serde(rename = "added_at")]
    added_at: String,
}
