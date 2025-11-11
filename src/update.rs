use crate::{git, lock, paths};
use anyhow::{Context, Result};
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
    let data =
        std::fs::read(&lock_path).with_context(|| format!("reading {}", lock_path.display()))?;
    let lf: lock::Lockfile = serde_json::from_slice(&data)
        .with_context(|| format!("parsing {}", lock_path.display()))?;
    // gather unique repos by host/owner/repo/url
    let mut uniq = BTreeSet::new();
    for s in lf.skills {
        uniq.insert((s.source.url, s.source.host, s.source.owner, s.source.repo));
    }
    if uniq.is_empty() {
        println!("Lockfile has no skills; update complete.");
        return Ok(());
    }

    for (url, host, owner, repo) in uniq.into_iter() {
        let cache_dir: PathBuf = paths::cache_repo_path(&host, &owner, &repo);
        let spec = git::RepoSpec {
            url: url.clone(),
            host: host.clone(),
            owner: owner.clone(),
            repo: repo.clone(),
        };
        git::ensure_cached_repo(&cache_dir, &spec)?;
        let _default = git::detect_or_set_default_branch(&cache_dir, &url)?;
        println!("Updated cache for {owner}/{repo}, default branch ok");
    }
    Ok(())
}
