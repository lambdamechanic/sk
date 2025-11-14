use crate::{config, digest, git, lock, paths, skills};
use anyhow::Result;
use chrono::Utc;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

pub fn run_doctor(apply: bool) -> Result<()> {
    let project_root = git::ensure_git_repo()?;
    let lock_path = project_root.join("skills.lock.json");
    if !lock_path.exists() {
        println!("No lockfile found.");
        return Ok(());
    }
    // Respect configured install root (default ./skills)
    let cfg = config::load_or_default()?;
    let install_root = paths::resolve_project_path(&project_root, &cfg.default_root);
    let lf = lock::Lockfile::load(&lock_path)?;
    let mut had_issues = false;
    // Track exact lock entries to drop by a stable composite key to avoid
    // accidentally removing other entries that share the same installName.
    let mut orphans_to_drop: HashSet<String> = HashSet::new();

    fn lock_entry_key(s: &lock::LockSkill) -> String {
        // installName + full source identity + commit + digest ensures uniqueness
        format!(
            "{}|{}|{}|{}|{}|{}|{}",
            s.install_name,
            s.source.host,
            s.source.owner,
            s.source.repo,
            s.source.skill_path,
            s.commit,
            s.digest
        )
    }

    // Detect duplicate installName values in the lockfile
    {
        let mut seen = HashSet::new();
        for s in &lf.skills {
            if !seen.insert(s.install_name.clone()) {
                had_issues = true;
                println!("- Duplicate installName in lockfile: {}", s.install_name);
            }
        }
    }

    // Track cache repos referenced by the lockfile for later pruning
    let mut referenced_caches: HashSet<PathBuf> = HashSet::new();
    for s in &lf.skills {
        println!("== {} ==", s.install_name);
        let install_dir = install_root.join(&s.install_name);
        if !install_dir.exists() {
            had_issues = true;
            println!("- Missing installed dir: {}", install_dir.display());
            if apply {
                let cache_dir = paths::resolve_or_primary_cache_path(
                    &s.source.url,
                    &s.source.host,
                    &s.source.owner,
                    &s.source.repo,
                );
                if cache_dir.exists() && git::has_object(&cache_dir, &s.commit).unwrap_or(false) {
                    // attempt rebuild via archive
                    if let Err(e) = crate::install::extract_subdir_from_commit(
                        &cache_dir,
                        &s.commit,
                        &s.source.skill_path,
                        &install_dir,
                    ) {
                        println!("  Rebuild failed: {e}");
                    } else {
                        println!("  Rebuilt from locked commit.");
                    }
                } else {
                    println!("  Cannot rebuild: cache/commit missing.");
                    // mark this exact lock entry for removal on apply
                    orphans_to_drop.insert(lock_entry_key(s));
                }
            }
        } else {
            if let Err(msg) = validate_skill_manifest(&install_dir) {
                had_issues = true;
                println!("- {msg}");
            }
            // digest
            let cur = digest::digest_dir(&install_dir).ok();
            match cur {
                Some(h) if h == s.digest => println!("- Digest ok"),
                Some(_) => {
                    had_issues = true;
                    println!("- Digest mismatch (modified)");
                }
                None => {
                    had_issues = true;
                    println!("- Digest compute failed");
                }
            }
        }
        // cache presence
        let cache_dir = paths::resolve_or_primary_cache_path(
            &s.source.url,
            &s.source.host,
            &s.source.owner,
            &s.source.repo,
        );
        referenced_caches.insert(cache_dir.clone());
        if !cache_dir.exists() {
            had_issues = true;
            println!("- Cache clone missing: {}", cache_dir.display());
        } else if !git::has_object(&cache_dir, &s.commit).unwrap_or(false) {
            had_issues = true;
            println!("- Locked commit missing from cache (force-push?)");
        }
    }

    // Detect and optionally prune unreferenced cache clones
    {
        let cache_root = paths::cache_root();
        if cache_root.exists() {
            println!("== Cache ==");
            // Walk cache_root/<host>/<owner>/<repo>
            if let Ok(hosts) = fs::read_dir(&cache_root) {
                for host in hosts.flatten() {
                    if !host.path().is_dir() {
                        continue;
                    }
                    if let Ok(owners) = fs::read_dir(host.path()) {
                        for owner in owners.flatten() {
                            if !owner.path().is_dir() {
                                continue;
                            }
                            if let Ok(repos) = fs::read_dir(owner.path()) {
                                for repo in repos.flatten() {
                                    let repo_path = repo.path();
                                    if !repo_path.is_dir() {
                                        continue;
                                    }
                                    // Only consider directories that look like git clones
                                    if !repo_path.join(".git").exists() {
                                        continue;
                                    }
                                    if !referenced_caches.contains(&repo_path) {
                                        had_issues = true;
                                        println!(
                                            "- Unreferenced cache clone: {}",
                                            repo_path.display()
                                        );
                                        if apply {
                                            if let Err(e) = fs::remove_dir_all(&repo_path) {
                                                println!(
                                                    "  Failed to prune cache '{}': {}",
                                                    repo_path.display(),
                                                    e
                                                );
                                            } else {
                                                println!(
                                                    "  Pruned unreferenced cache: {}",
                                                    repo_path.display()
                                                );
                                                // Try cleaning up empty parents (owner/, host/)
                                                let _ = clean_if_empty(owner.path());
                                                let _ = clean_if_empty(host.path());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    // Normalize lockfile and drop orphan entries when applying
    if apply {
        let mut lf_new = lf.clone();
        if !orphans_to_drop.is_empty() {
            let before = lf_new.skills.len();
            lf_new
                .skills
                .retain(|s| !orphans_to_drop.contains(&lock_entry_key(s)));
            let removed = before - lf_new.skills.len();
            println!("Removed {removed} orphan lock entries.");
            had_issues = true;
        }
        // Sort by installName for stable diffs
        lf_new
            .skills
            .sort_by(|a, b| a.install_name.cmp(&b.install_name));
        lf_new.generated_at = Utc::now().to_rfc3339();
        // Save only if changes differ from original lockfile
        if serde_json::to_string(&lf_new)? != serde_json::to_string(&lf)? {
            crate::lock::save_lockfile(&lock_path, &lf_new)?;
            println!("Normalized lockfile (ordering/timestamps).");
        }
    }

    if !had_issues {
        println!("All checks passed.");
    }
    Ok(())
}

fn clean_if_empty(dir: PathBuf) -> Result<()> {
    if dir.is_dir() && dir.read_dir()?.next().is_none() {
        fs::remove_dir_all(dir)?;
    }
    Ok(())
}

fn validate_skill_manifest(dir: &Path) -> Result<(), String> {
    let skill_md = dir.join("SKILL.md");
    if !skill_md.exists() {
        return Err(format!("Missing SKILL.md at {}", skill_md.display()));
    }
    match skills::parse_frontmatter_file(&skill_md) {
        Ok(meta) => {
            if meta.name.trim().is_empty() || meta.description.trim().is_empty() {
                Err(format!(
                    "SKILL.md missing required name/description fields at {}",
                    skill_md.display()
                ))
            } else {
                Ok(())
            }
        }
        Err(e) => Err(format!("Invalid SKILL.md at {} ({e})", skill_md.display())),
    }
}
