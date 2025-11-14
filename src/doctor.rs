use crate::{config, digest, git, lock, paths, skills};
use anyhow::Result;
use chrono::Utc;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

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
        let spec = s.source.repo_spec();
        format!(
            "{}|{}|{}|{}|{}|{}|{}",
            s.install_name,
            spec.host,
            spec.owner,
            spec.repo,
            s.source.skill_path(),
            s.commit,
            s.digest
        )
    }

    fn rebuild_missing_install(
        cache_dir: &Path,
        skill: &lock::LockSkill,
        install_dir: &Path,
        skill_messages: &mut Vec<String>,
        orphans_to_drop: &mut HashSet<String>,
    ) {
        if cache_dir.exists() && git::has_object(cache_dir, &skill.commit).unwrap_or(false) {
            match crate::install::extract_subdir_from_commit(
                cache_dir,
                &skill.commit,
                skill.source.skill_path(),
                install_dir,
            ) {
                Ok(_) => skill_messages.push("  Rebuilt from locked commit.".to_string()),
                Err(e) => skill_messages.push(format!("  Rebuild failed: {e}")),
            }
        } else {
            skill_messages.push("  Cannot rebuild: cache/commit missing.".to_string());
            orphans_to_drop.insert(lock_entry_key(skill));
        }
    }

    fn compute_upstream_update(
        cache_dir: &Path,
        spec: &git::RepoSpec,
        current_commit: &str,
    ) -> Option<String> {
        if !cache_dir.exists() {
            return None;
        }
        let branch = git::detect_or_set_default_branch(cache_dir, spec).ok()?;
        let tip_ref = format!("refs/remotes/origin/{branch}");
        let new_sha = git::rev_parse(cache_dir, &tip_ref).ok()?;
        if new_sha == current_commit {
            None
        } else {
            Some(format!(
                "{} -> {}",
                short_sha(current_commit),
                short_sha(&new_sha)
            ))
        }
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
        let mut skill_messages: Vec<String> = Vec::new();
        let install_dir = install_root.join(&s.install_name);
        let spec = s.source.repo_spec();
        let cache_dir =
            paths::resolve_or_primary_cache_path(&spec.url, &spec.host, &spec.owner, &spec.repo);
        referenced_caches.insert(cache_dir.clone());
        let mut local_modified = false;
        if !install_dir.exists() {
            had_issues = true;
            skill_messages.push(format!(
                "- Missing installed dir: {}",
                install_dir.display()
            ));
            if apply {
                rebuild_missing_install(
                    &cache_dir,
                    s,
                    &install_dir,
                    &mut skill_messages,
                    &mut orphans_to_drop,
                );
            }
        } else {
            if let Err(msg) = validate_skill_manifest(&install_dir) {
                had_issues = true;
                skill_messages.push(format!("- {msg}"));
            }
            // digest
            let cur = digest::digest_dir(&install_dir).ok();
            match cur {
                Some(h) if h == s.digest => {}
                Some(_) => {
                    had_issues = true;
                    skill_messages.push("- Digest mismatch (modified)".to_string());
                    local_modified = true;
                }
                None => {
                    had_issues = true;
                    skill_messages.push("- Digest compute failed".to_string());
                    local_modified = true;
                }
            }
        }
        let upstream_update = compute_upstream_update(&cache_dir, &spec, &s.commit);
        if !cache_dir.exists() {
            had_issues = true;
            skill_messages.push(format!("- Cache clone missing: {}", cache_dir.display()));
        } else if !git::has_object(&cache_dir, &s.commit).unwrap_or(false) {
            had_issues = true;
            skill_messages.push("- Locked commit missing from cache (force-push?)".to_string());
        }

        match (local_modified, upstream_update.as_ref()) {
            (true, Some(update)) => {
                skill_messages.push(format!(
                    "- Local edits present and upstream advanced ({update}). Run 'sk sync-back {name}' to publish or revert changes, then 'sk upgrade {name}' to pick up the remote tip.",
                    name = s.install_name
                ));
            }
            (true, None) => {
                skill_messages.push(format!(
                    "- Local edits are ahead of the lockfile. Run 'sk sync-back {name}' if intentional, or discard them to restore the locked digest.",
                    name = s.install_name
                ));
            }
            (false, Some(update)) => {
                skill_messages.push(format!(
                    "- Upgrade available ({update}). Run 'sk upgrade {name}' to sync.",
                    name = s.install_name
                ));
            }
            (false, None) => {}
        }

        if !skill_messages.is_empty() {
            println!("== {} ==", s.install_name);
            for msg in skill_messages {
                println!("{msg}");
            }
        }
    }

    let cache_messages = gather_cache_messages(&referenced_caches, apply);
    if !cache_messages.is_empty() {
        had_issues = true;
        println!("== Cache ==");
        for msg in cache_messages {
            println!("{msg}");
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

fn gather_cache_messages(referenced_caches: &HashSet<PathBuf>, apply: bool) -> Vec<String> {
    let mut cache_messages = Vec::new();
    let cache_root = paths::cache_root();
    if !cache_root.exists() {
        return cache_messages;
    }
    let walker = WalkDir::new(&cache_root)
        .min_depth(3)
        .max_depth(3)
        .into_iter()
        .filter_map(|entry| entry.ok());
    for entry in walker {
        if !entry.file_type().is_dir() {
            continue;
        }
        let repo_path = entry.into_path();
        if !repo_path.join(".git").exists() || referenced_caches.contains(&repo_path) {
            continue;
        }
        cache_messages.push(format!(
            "- Unreferenced cache clone: {}",
            repo_path.display()
        ));
        if apply {
            if let Err(e) = fs::remove_dir_all(&repo_path) {
                cache_messages.push(format!(
                    "  Failed to prune cache '{}': {}",
                    repo_path.display(),
                    e
                ));
            } else {
                cache_messages.push(format!(
                    "  Pruned unreferenced cache: {}",
                    repo_path.display()
                ));
                prune_empty_parents(&repo_path);
            }
        }
    }
    cache_messages
}

fn prune_empty_parents(repo_path: &Path) {
    if let Some(owner_dir) = repo_path.parent() {
        let _ = clean_if_empty(owner_dir.to_path_buf());
        if let Some(host_dir) = owner_dir.parent() {
            let _ = clean_if_empty(host_dir.to_path_buf());
        }
    }
}

fn short_sha(full: &str) -> &str {
    if full.len() > 7 {
        &full[..7]
    } else {
        full
    }
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
