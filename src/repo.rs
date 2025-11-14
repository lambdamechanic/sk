use crate::{config, git, lock, paths, skills};
use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub struct RepoAddArgs<'a> {
    pub repo: &'a str,
    pub alias: Option<&'a str>,
    pub https: bool,
}

pub struct RepoListArgs {
    pub json: bool,
}

pub struct RepoCatalogArgs<'a> {
    pub target: &'a str,
    pub https: bool,
    pub json: bool,
}

pub struct RepoSearchArgs<'a> {
    pub query: &'a str,
    pub target: Option<&'a str>,
    pub https: bool,
    pub json: bool,
}

pub fn run_repo_add(args: RepoAddArgs) -> Result<()> {
    let project_root = git::ensure_git_repo()?;
    let cfg = config::load_or_default()?;
    let prefer_https = args.https || cfg.protocol.eq_ignore_ascii_case("https");
    let spec = git::parse_repo_input(args.repo, prefer_https, &cfg.default_host)?;
    let cache_dir =
        paths::resolve_or_primary_cache_path(&spec.url, &spec.host, &spec.owner, &spec.repo);
    git::ensure_cached_repo(&cache_dir, &spec)?;

    let alias = args
        .alias
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("{}/{}", spec.owner, spec.repo));

    let registry_path = registry_path(&project_root);
    let mut registry = RepoRegistry::load(&registry_path)?;
    ensure_unique_alias(&registry, &alias)?;
    ensure_unique_repo(&registry, &spec)?;

    registry.repos.push(RepoEntry {
        alias: alias.clone(),
        spec: spec.clone(),
        added_at: Utc::now().to_rfc3339(),
    });
    registry.updated_at = Utc::now().to_rfc3339();
    registry.save(&registry_path)?;

    println!("Registered repo '{alias}' -> {}:{}", spec.host, spec.url);
    Ok(())
}

pub fn run_repo_list(args: RepoListArgs) -> Result<()> {
    let project_root = git::ensure_git_repo()?;
    let registry = RepoRegistry::load(&registry_path(&project_root))?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&registry.repos)?);
        return Ok(());
    }

    if registry.repos.is_empty() {
        println!("(no repos registered)");
        return Ok(());
    }

    let lock_path = project_root.join("skills.lock.json");
    let lockfile = lock::Lockfile::load_or_empty(&lock_path)?;
    let mut installed_counts: HashMap<String, usize> = HashMap::new();
    for skill in lockfile.skills {
        let spec = skill.source.repo_spec();
        let key = repo_key(spec);
        *installed_counts.entry(key).or_default() += 1;
    }

    println!(
        "{:<12} {:<40} {:>6} {:>10}",
        "ALIAS", "REPO", "SKILLS", "INSTALLED"
    );
    for entry in &registry.repos {
        let spec = &entry.spec;
        let repo_label = format!("{}/{}/{}", spec.host, spec.owner, spec.repo);
        let available = load_skills_for_spec(spec)?.len();
        let installed = installed_counts.get(&repo_key(spec)).copied().unwrap_or(0);
        println!(
            "{:<12} {:<40} {:>6} {:>10}",
            entry.alias, repo_label, available, installed
        );
    }
    Ok(())
}

pub fn run_repo_catalog(args: RepoCatalogArgs) -> Result<()> {
    let project_root = git::ensure_git_repo()?;
    let cfg = config::load_or_default()?;
    let registry = RepoRegistry::load(&registry_path(&project_root))?;
    let (_, spec) = resolve_target_spec(args.target, &registry, &cfg, args.https)?;
    let skills = load_skills_for_spec(&spec)?;

    if args.json {
        let entries: Vec<_> = skills
            .iter()
            .map(|skill| CatalogEntry {
                name: skill.meta.name.clone(),
                description: skill.meta.description.clone(),
                path: skill.skill_path.clone(),
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else if skills.is_empty() {
        println!("No skills found in {}/{}", spec.owner, spec.repo);
    } else {
        for skill in skills {
            println!(
                "{}\t{}\t{}",
                skill.meta.name, skill.skill_path, skill.meta.description
            );
        }
    }

    Ok(())
}

pub fn run_repo_search(args: RepoSearchArgs) -> Result<()> {
    let project_root = git::ensure_git_repo()?;
    let cfg = config::load_or_default()?;
    let registry = RepoRegistry::load(&registry_path(&project_root))?;

    let targets: Vec<(String, git::RepoSpec)> = if let Some(target) = args.target {
        let (alias, spec) = resolve_target_spec(target, &registry, &cfg, args.https)?;
        let label = alias.unwrap_or_else(|| format!("{}/{}", spec.owner, spec.repo));
        vec![(label, spec)]
    } else if registry.repos.is_empty() {
        bail!(
            "No repos registered. Run 'sk repo add <repo>' first or pass --repo <alias-or-repo>."
        );
    } else {
        registry
            .repos
            .iter()
            .map(|entry| (entry.alias.clone(), entry.spec.clone()))
            .collect()
    };

    let needle = args.query.to_lowercase();
    let mut hits: Vec<SearchHit> = vec![];
    for (label, spec) in targets {
        let skills = load_skills_for_spec(&spec)?;
        for skill in skills {
            if matches_query(&needle, &skill) {
                hits.push(SearchHit {
                    repo: label.clone(),
                    name: skill.meta.name.clone(),
                    description: skill.meta.description.clone(),
                    path: skill.skill_path.clone(),
                });
            }
        }
    }

    if args.json {
        println!("{}", serde_json::to_string_pretty(&hits)?);
    } else if hits.is_empty() {
        println!("No skills matching '{}' found.", args.query);
    } else {
        for hit in hits {
            println!(
                "{}\t{}\t{}\t{}",
                hit.repo, hit.name, hit.path, hit.description
            );
        }
    }
    Ok(())
}

fn ensure_unique_alias(registry: &RepoRegistry, alias: &str) -> Result<()> {
    if let Some(existing) = registry.repos.iter().find(|r| r.alias == alias) {
        bail!(
            "alias '{}' already registered for repo {}/{}",
            alias,
            existing.spec.owner,
            existing.spec.repo
        );
    }
    Ok(())
}

fn ensure_unique_repo(registry: &RepoRegistry, spec: &git::RepoSpec) -> Result<()> {
    if let Some(existing) = registry.repos.iter().find(|r| {
        r.spec.host == spec.host && r.spec.owner == spec.owner && r.spec.repo == spec.repo
    }) {
        bail!(
            "repo {}/{} already registered under alias '{}'",
            spec.owner,
            spec.repo,
            existing.alias
        );
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RepoEntry {
    alias: String,
    spec: git::RepoSpec,
    added_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct RepoRegistry {
    repos: Vec<RepoEntry>,
    updated_at: String,
}

impl RepoRegistry {
    fn load(path: &Path) -> Result<Self> {
        if path.exists() {
            let data = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
            let registry: RepoRegistry = serde_json::from_slice(&data)
                .with_context(|| format!("parsing {}", path.display()))?;
            Ok(registry)
        } else {
            Ok(Self::empty_now())
        }
    }

    fn save(&self, path: &Path) -> Result<()> {
        let pretty = serde_json::to_string_pretty(self)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, pretty).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    fn by_alias(&self, alias: &str) -> Option<&RepoEntry> {
        self.repos.iter().find(|r| r.alias == alias)
    }

    fn empty_now() -> Self {
        Self {
            repos: vec![],
            updated_at: Utc::now().to_rfc3339(),
        }
    }
}

#[derive(Serialize)]
struct CatalogEntry {
    name: String,
    description: String,
    path: String,
}

#[derive(Serialize)]
struct SearchHit {
    repo: String,
    name: String,
    description: String,
    path: String,
}

fn registry_path(project_root: &Path) -> PathBuf {
    project_root.join("skills.repos.json")
}

fn resolve_target_spec(
    target: &str,
    registry: &RepoRegistry,
    cfg: &config::UserConfig,
    https_flag: bool,
) -> Result<(Option<String>, git::RepoSpec)> {
    if let Some(entry) = registry.by_alias(target) {
        Ok((Some(entry.alias.clone()), entry.spec.clone()))
    } else {
        let prefer_https = https_flag || cfg.protocol.eq_ignore_ascii_case("https");
        let spec = git::parse_repo_input(target, prefer_https, &cfg.default_host)?;
        Ok((None, spec))
    }
}

fn load_skills_for_spec(spec: &git::RepoSpec) -> Result<Vec<skills::DiscoveredSkill>> {
    let cache_dir =
        paths::resolve_or_primary_cache_path(&spec.url, &spec.host, &spec.owner, &spec.repo);
    git::ensure_cached_repo(&cache_dir, spec)?;
    let default_branch = git::detect_or_set_default_branch(&cache_dir, spec)?;
    let commit = git::rev_parse(&cache_dir, &format!("refs/remotes/origin/{default_branch}"))?;
    skills::list_skills_in_repo(&cache_dir, &commit)
}

fn repo_key(spec: &git::RepoSpec) -> String {
    format!("{}|{}|{}", spec.host, spec.owner, spec.repo)
}

fn matches_query(needle: &str, skill: &skills::DiscoveredSkill) -> bool {
    let mut haystacks = vec![
        skill.meta.name.to_lowercase(),
        skill.skill_path.to_lowercase(),
    ];
    haystacks.push(skill.meta.description.to_lowercase());
    haystacks.iter().any(|field| field.contains(needle))
}
