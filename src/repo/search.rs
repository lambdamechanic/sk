use crate::{config, git, lock};
use anyhow::{bail, Result};
use serde::Serialize;

use super::{load_repo_snapshot, matches_query};

pub struct RepoSearchArgs<'a> {
    pub query: Option<&'a str>,
    pub target: Option<&'a str>,
    pub https: bool,
    pub json: bool,
    pub list_all: bool,
}

struct SearchSetup {
    list_mode: bool,
    trimmed_query: Option<String>,
    lowered_query: Option<String>,
    targets: Vec<(String, git::RepoSpec)>,
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

pub fn run_repo_search(args: RepoSearchArgs<'_>) -> Result<()> {
    let project_root = git::ensure_git_repo()?;
    let cfg = config::load_or_default()?;
    let lock_path = project_root.join("skills.lock.json");
    let lockfile = lock::Lockfile::load_or_empty(&lock_path)?;

    let setup = build_search_setup(&args, &lockfile, &cfg)?;
    if setup.list_mode && args.target.is_some() {
        let spec = &setup
            .targets
            .first()
            .expect("list mode with --repo should yield a single target")
            .1;
        return print_repo_skill_listing(&args, spec);
    }

    let hits = collect_search_hits(&setup)?;
    display_search_hits(
        &args,
        &hits,
        setup.list_mode,
        setup.trimmed_query.as_deref(),
    )
}

fn build_search_setup(
    args: &RepoSearchArgs<'_>,
    lockfile: &lock::Lockfile,
    cfg: &config::UserConfig,
) -> Result<SearchSetup> {
    let trimmed_query = args.query.and_then(|raw| {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    });
    if args.list_all && trimmed_query.is_some() {
        bail!(
            "--all cannot be combined with a search query. Remove the query to list every skill."
        );
    }
    let list_mode = args.list_all || trimmed_query.is_none();
    let lowered_query = trimmed_query.as_ref().map(|s| s.to_lowercase());

    let targets: Vec<(String, git::RepoSpec)> = if let Some(target) = args.target {
        let spec = super::resolve_target_spec(target, lockfile, cfg, args.https)?;
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

    Ok(SearchSetup {
        list_mode,
        trimmed_query,
        lowered_query,
        targets,
    })
}

fn print_repo_skill_listing(args: &RepoSearchArgs<'_>, spec: &git::RepoSpec) -> Result<()> {
    let snapshot = load_repo_snapshot(spec)?;
    if args.json {
        let entries: Vec<_> = snapshot
            .skills
            .iter()
            .map(|skill| CatalogEntry {
                name: skill.meta.name.clone(),
                description: skill.meta.description.clone(),
                path: skill.skill_path.clone(),
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&entries)?);
        return Ok(());
    }
    if snapshot.skills.is_empty() {
        println!("No skills found in {}/{}", spec.owner, spec.repo);
        return Ok(());
    }
    for skill in snapshot.skills.iter() {
        println!(
            "{}\t{}\t{}",
            skill.meta.name, skill.skill_path, skill.meta.description
        );
    }
    Ok(())
}

fn collect_search_hits(setup: &SearchSetup) -> Result<Vec<SearchHit>> {
    let mut hits = Vec::new();
    for (label, spec) in &setup.targets {
        let snapshot = load_repo_snapshot(spec)?;
        for skill in snapshot.skills.iter() {
            let include = if setup.list_mode {
                true
            } else if let Some(needle) = setup.lowered_query.as_ref() {
                matches_query(needle, skill)
            } else {
                false
            };
            if include {
                hits.push(SearchHit {
                    repo: label.clone(),
                    name: skill.meta.name.clone(),
                    description: skill.meta.description.clone(),
                    path: skill.skill_path.clone(),
                });
            }
        }
    }
    Ok(hits)
}

fn display_search_hits(
    args: &RepoSearchArgs<'_>,
    hits: &[SearchHit],
    list_mode: bool,
    trimmed_query: Option<&str>,
) -> Result<()> {
    if args.json {
        println!("{}", serde_json::to_string_pretty(&hits)?);
        return Ok(());
    }

    if hits.is_empty() {
        if list_mode {
            println!("No skills found in the cached repos.");
        } else if let Some(query) = trimmed_query {
            println!("No skills matching '{}' found.", query);
        } else {
            println!("No skills matching the requested filters found.");
        }
        return Ok(());
    }

    for hit in hits {
        println!(
            "{}\t{}\t{}\t{}",
            hit.repo, hit.name, hit.path, hit.description
        );
    }
    Ok(())
}
