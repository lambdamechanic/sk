use crate::{digest, git, lock, paths};
use anyhow::{Context, Result};
use std::fs;

pub fn run_doctor(apply: bool) -> Result<()> {
    let project_root = git::ensure_git_repo()?;
    let lock_path = project_root.join("skills.lock.json");
    if !lock_path.exists() { println!("No lockfile found."); return Ok(()); }
    let data = fs::read(&lock_path)?;
    let lf: lock::Lockfile = serde_json::from_slice(&data).context("parse lockfile")?;
    let mut had_issues = false;
    for s in &lf.skills {
        println!("== {} ==", s.installName);
        let install_dir = project_root.join("skills").join(&s.installName); // default; not reading config here for simplicity
        if !install_dir.exists() {
            had_issues = true; println!("- Missing installed dir: {}", install_dir.display());
            if apply {
                let cache_dir = paths::cache_repo_path(&s.source.host, &s.source.owner, &s.source.repo);
                if cache_dir.exists() && git::has_object(&cache_dir, &s.commit).unwrap_or(false) {
                    // attempt rebuild via archive
                    if let Err(e) = crate::install::extract_subdir_from_commit(&cache_dir, &s.commit, &s.source.skillPath, &install_dir) {
                        println!("  Rebuild failed: {}", e);
                    } else {
                        println!("  Rebuilt from locked commit.");
                    }
                } else {
                    println!("  Cannot rebuild: cache/commit missing.");
                }
            }
        } else {
            // digest
            let cur = digest::digest_dir(&install_dir).ok();
            match cur {
                Some(h) if h == s.digest => println!("- Digest ok"),
                Some(_) => { had_issues = true; println!("- Digest mismatch (modified)"); },
                None => { had_issues = true; println!("- Digest compute failed"); }
            }
        }
        // cache presence
        let cache_dir = paths::cache_repo_path(&s.source.host, &s.source.owner, &s.source.repo);
        if !cache_dir.exists() { had_issues = true; println!("- Cache clone missing: {}", cache_dir.display()); }
        else if !git::has_object(&cache_dir, &s.commit).unwrap_or(false) { had_issues = true; println!("- Locked commit missing from cache (force-push?)"); }
    }
    if !had_issues { println!("All checks passed."); }
    Ok(())
}

