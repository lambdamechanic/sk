use crate::{config, git, lock, paths, skills};
use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::Serialize;
use std::collections::HashMap;

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

pub struct RepoRemoveArgs<'a> {
    pub target: &'a str,
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
        .unwrap_or_else(|| preferred_alias(&spec));

    let lock_path = project_root.join("skills.lock.json");
    lock::edit_lockfile(&lock_path, |lf| {
        ensure_alias_available(&lf.repos, &alias, None)?;
        let key = lock::repo_key(&spec);
        if let Some(existing) = lf.repos.entry_by_key(&key) {
            bail!(
                "repo {}/{} already registered under alias '{}'",
                spec.owner,
                spec.repo,
                existing.alias
            );
        }
        lf.repos.insert_if_missing(&spec, Some(alias.clone()), None);
        lf.generated_at = Utc::now().to_rfc3339();
        Ok(())
    })?;

    println!("Registered repo '{alias}' -> {}:{}", spec.host, spec.url);
    Ok(())
}

pub fn run_repo_list(args: RepoListArgs) -> Result<()> {
    let project_root = git::ensure_git_repo()?;
    let lock_path = project_root.join("skills.lock.json");
    let lockfile = lock::Lockfile::load_or_empty(&lock_path)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&lockfile.repos.entries)?);
        return Ok(());
    }

    if lockfile.repos.entries.is_empty() {
        println!("(no repos registered)");
        return Ok(());
    }

    let mut installed_counts: HashMap<String, usize> = HashMap::new();
    for skill in &lockfile.skills {
        let key = skill.source.repo_key().to_string();
        *installed_counts.entry(key).or_default() += 1;
    }

    println!(
        "{:<12} {:<40} {:>6} {:>10}",
        "ALIAS", "REPO", "SKILLS", "INSTALLED"
    );
    let mut had_dirty = false;
    for entry in &lockfile.repos.entries {
        let spec = &entry.spec;
        let repo_label = format!("{}/{}/{}", spec.host, spec.owner, spec.repo);
        let counts = match load_available_skills_with_cache(spec) {
            Ok(counts) => counts,
            Err(err) => {
                eprintln!(
                    "warning: skipping repo {} ({repo_label}): {err}",
                    entry.alias
                );
                continue;
            }
        };
        if counts.dirty {
            had_dirty = true;
        }
        let installed = installed_counts.get(entry.repo_key()).copied().unwrap_or(0);
        let dirty_flag = if counts.dirty { "*" } else { "" };
        println!(
            "{:<12} {:<40} {:>6}{} {:>10}",
            entry.alias, repo_label, counts.available, dirty_flag, installed
        );
    }
    if had_dirty {
        println!("* stale cache: failed to refresh remote; showing last-known counts");
    }
    Ok(())
}

pub fn run_repo_catalog(args: RepoCatalogArgs) -> Result<()> {
    let project_root = git::ensure_git_repo()?;
    let cfg = config::load_or_default()?;
    let lock_path = project_root.join("skills.lock.json");
    let lockfile = lock::Lockfile::load_or_empty(&lock_path)?;
    let spec = resolve_target_spec(args.target, &lockfile, &cfg, args.https)?;
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
    let lock_path = project_root.join("skills.lock.json");
    let lockfile = lock::Lockfile::load_or_empty(&lock_path)?;

    let targets: Vec<(String, git::RepoSpec)> = if let Some(target) = args.target {
        let spec = resolve_target_spec(target, &lockfile, &cfg, args.https)?;
        let label = lockfile
            .repos
            .entry_by_key(&lock::repo_key(&spec))
            .map(|entry| entry.alias.clone())
            .unwrap_or_else(|| format!("{}/{}", spec.owner, spec.repo));
        vec![(label, spec)]
    } else if lockfile.repos.entries.is_empty() {
        bail!(
            "No repos registered. Run 'sk repo add <repo>' first or pass --repo <alias-or-repo>."
        );
    } else {
        lockfile
            .repos
            .entries
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

pub fn run_repo_remove(args: RepoRemoveArgs) -> Result<()> {
    let project_root = git::ensure_git_repo()?;
    let cfg = config::load_or_default()?;
    let lock_path = project_root.join("skills.lock.json");

    let removed = lock::edit_lockfile(&lock_path, |lf| {
        let removed = remove_repo_entry(lf, args.target, &cfg, args.https)?;
        if removed.is_some() {
            lf.generated_at = Utc::now().to_rfc3339();
        }
        Ok(removed)
    })?;
    match removed {
        Some(entry) => {
            if args.json {
                println!(
                    "{}",
                    serde_json::json!({
                        "status": "removed",
                        "alias": entry.alias,
                        "repo": {
                            "host": entry.spec.host,
                            "owner": entry.spec.owner,
                            "name": entry.spec.repo
                        }
                    })
                );
            } else {
                println!(
                    "Removed repo '{}' ({}/{}/{}).",
                    entry.alias, entry.spec.host, entry.spec.owner, entry.spec.repo
                );
            }
        }
        None => {
            if args.json {
                println!(
                    "{}",
                    serde_json::json!({
                        "status": "not_found",
                        "target": args.target
                    })
                );
            } else {
                println!("No repo registered for '{}'.", args.target);
            }
        }
    }
    Ok(())
}

fn remove_repo_entry(
    lockfile: &mut lock::Lockfile,
    target: &str,
    cfg: &config::UserConfig,
    https_flag: bool,
) -> Result<Option<lock::RepoEntry>> {
    if let Some(entry) = lockfile.repos.entry_by_alias(target).cloned() {
        ensure_repo_unused(lockfile, &entry)?;
        return Ok(lockfile.repos.remove_by_alias(target));
    }

    let prefer_https = https_flag || cfg.protocol.eq_ignore_ascii_case("https");
    if let Ok(spec) = git::parse_repo_input(target, prefer_https, &cfg.default_host) {
        let desired = lock::repo_key(&spec);
        if let Some(entry) = lockfile.repos.entry_by_key(&desired).cloned() {
            ensure_repo_unused(lockfile, &entry)?;
            return Ok(lockfile.repos.remove_by_key(&desired));
        }
    }

    Ok(None)
}

fn ensure_repo_unused(lockfile: &lock::Lockfile, entry: &lock::RepoEntry) -> Result<()> {
    let dependents: Vec<String> = lockfile
        .skills
        .iter()
        .filter(|skill| skill.source.repo_key() == entry.repo_key())
        .map(|skill| skill.install_name.clone())
        .collect();
    if dependents.is_empty() {
        Ok(())
    } else {
        bail!(
            "Cannot remove repo '{}' while skills ({}) depend on it.",
            entry.alias,
            dependents.join(", ")
        );
    }
}

fn ensure_alias_available(
    registry: &lock::RepoRegistry,
    alias: &str,
    except_key: Option<&str>,
) -> Result<()> {
    if let Some(existing) = registry
        .entries
        .iter()
        .find(|entry| entry.alias == alias && except_key.map_or(true, |key| entry.key != key))
    {
        bail!(
            "alias '{}' already registered for repo {}/{}",
            alias,
            existing.spec.owner,
            existing.spec.repo
        );
    }
    Ok(())
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

fn resolve_target_spec(
    target: &str,
    lockfile: &lock::Lockfile,
    cfg: &config::UserConfig,
    https_flag: bool,
) -> Result<git::RepoSpec> {
    if let Some(entry) = lockfile.repos.entry_by_alias(target) {
        return Ok(entry.spec.clone());
    }
    let prefer_https = https_flag || cfg.protocol.eq_ignore_ascii_case("https");
    git::parse_repo_input(target, prefer_https, &cfg.default_host)
}

fn preferred_alias(spec: &git::RepoSpec) -> String {
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

fn load_skills_for_spec(spec: &git::RepoSpec) -> Result<Vec<skills::DiscoveredSkill>> {
    let cache_dir =
        paths::resolve_or_primary_cache_path(&spec.url, &spec.host, &spec.owner, &spec.repo);
    git::ensure_cached_repo(&cache_dir, spec)?;
    let default_branch = git::detect_or_set_default_branch(&cache_dir, spec)?;
    let commit = git::rev_parse(&cache_dir, &format!("refs/remotes/origin/{default_branch}"))?;
    skills::list_skills_in_repo(&cache_dir, &commit)
}

struct RepoListCounts {
    available: usize,
    dirty: bool,
}

fn load_available_skills_with_cache(spec: &git::RepoSpec) -> Result<RepoListCounts> {
    let cache_dir =
        paths::resolve_or_primary_cache_path(&spec.url, &spec.host, &spec.owner, &spec.repo);
    let repo_label = format!("{}/{}/{}", spec.host, spec.owner, spec.repo);
    let mut dirty = false;
    match git::ensure_cached_repo(&cache_dir, spec) {
        Ok(_) => {}
        Err(err) => {
            if cache_dir.join(".git").exists() {
                dirty = true;
                eprintln!("warning: unable to refresh {repo_label}; using cached counts ({err})");
            } else {
                return Err(err.context(format!("failed to cache repo {repo_label}")));
            }
        }
    }
    let default_branch = git::detect_or_set_default_branch(&cache_dir, spec)
        .with_context(|| format!("determining default branch for {repo_label}"))?;
    let commit = git::rev_parse(&cache_dir, &format!("refs/remotes/origin/{default_branch}"))
        .with_context(|| format!("reading cached commit for {repo_label}"))?;
    let available = skills::list_skills_in_repo(&cache_dir, &commit)
        .with_context(|| format!("listing skills for {repo_label}"))?
        .len();
    Ok(RepoListCounts { available, dirty })
}

fn matches_query(needle: &str, skill: &skills::DiscoveredSkill) -> bool {
    let mut haystacks = vec![
        skill.meta.name.to_lowercase(),
        skill.skill_path.to_lowercase(),
    ];
    haystacks.push(skill.meta.description.to_lowercase());
    haystacks.iter().any(|field| field.contains(needle))
}
