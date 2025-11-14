use crate::{git, lock, paths};
use anyhow::Result;
use std::collections::BTreeSet;
use std::path::PathBuf;

pub fn run_update() -> Result<()> {
    let project_root = git::ensure_git_repo()?;
    let lock_path = project_root.join("skills.lock.json");
    if !lock_path.exists() {
        println!(
            "No lockfile found at {}, nothing to update.",
            lock_path.display()
        );
        return Ok(());
    }
    let lf = lock::Lockfile::load(&lock_path)?;
    // gather unique repos by host/owner/repo/url
    let mut uniq = BTreeSet::new();
    for s in lf.skills {
        let spec = s.source.repo_spec_owned();
        uniq.insert((spec.url, spec.host, spec.owner, spec.repo));
    }
    if uniq.is_empty() {
        println!("Lockfile has no skills; update complete.");
        return Ok(());
    }

    for (url, host, owner, repo) in uniq.into_iter() {
        let cache_dir: PathBuf = paths::resolve_or_primary_cache_path(&url, &host, &owner, &repo);
        let spec = git::RepoSpec {
            url: url.clone(),
            host: host.clone(),
            owner: owner.clone(),
            repo: repo.clone(),
        };
        git::ensure_cached_repo(&cache_dir, &spec)?;
        let default_branch = git::refresh_default_branch(&cache_dir, &spec)?;
        println!("Updated cache for {owner}/{repo} (default branch {default_branch})");
    }
    Ok(())
}
