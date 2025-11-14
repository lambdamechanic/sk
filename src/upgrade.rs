use crate::{config, digest, git, install, lock, paths};
use anyhow::{bail, Context, Result};
use chrono::Utc;
use std::fs;
use tempfile::TempDir;

pub struct UpgradeArgs<'a> {
    pub target: &'a str, // installed name or "--all"
    pub root: Option<&'a str>,
    pub dry_run: bool,
}

pub fn run_upgrade(args: UpgradeArgs) -> Result<()> {
    let project_root = git::ensure_git_repo()?;
    let cfg = config::load_or_default()?;
    let install_root_rel = args.root.unwrap_or(&cfg.default_root);
    let install_root = paths::resolve_project_path(&project_root, install_root_rel);

    let lock_path = project_root.join("skills.lock.json");
    if !lock_path.exists() {
        bail!("no lockfile found");
    }
    let mut lf = lock::Lockfile::load(&lock_path)?;

    // Select targets
    let all = args.target == "--all";
    let targets: Vec<lock::LockSkill> = if all {
        lf.skills.clone()
    } else {
        lf.skills
            .iter()
            .filter(|s| s.install_name == args.target)
            .cloned()
            .collect()
    };
    if targets.is_empty() {
        bail!("skill not found: {}", args.target);
    }

    // Preflight: compute plan and detect modified without mutating
    let mut plan: Vec<(String, std::path::PathBuf, String)> = vec![]; // (install_name, dest, new_commit)
    let mut skipped_modified: Vec<(String, Option<(String, String)>)> = vec![];
    for skill in &targets {
        let dest = install_root.join(&skill.install_name);
        if !dest.exists() {
            bail!(
                "installed dir missing for '{}'. Run 'sk doctor --apply' to rebuild first.",
                skill.install_name
            );
        }
        let cur_digest = digest::digest_dir(&dest).ok();
        let is_modified = match &cur_digest {
            Some(h) => h != &skill.digest,
            None => true,
        };

        let cache_dir = paths::resolve_or_primary_cache_path(
            &skill.source.url,
            &skill.source.host,
            &skill.source.owner,
            &skill.source.repo,
        );
        // Always refresh cache to see latest remote state
        let spec = git::RepoSpec {
            url: skill.source.url.clone(),
            host: skill.source.host.clone(),
            owner: skill.source.owner.clone(),
            repo: skill.source.repo.clone(),
        };
        git::ensure_cached_repo(&cache_dir, &spec)?;

        let default = git::detect_or_set_default_branch(&cache_dir, &skill.source.url)?;
        let rev = format!("refs/remotes/origin/{default}");
        let new_commit = git::rev_parse(&cache_dir, &rev)?;

        let needs_upgrade = new_commit != skill.commit;

        if is_modified {
            if all {
                let span = needs_upgrade.then(|| (skill.commit.clone(), new_commit.clone()));
                skipped_modified.push((skill.install_name.clone(), span));
                continue;
            } else {
                bail!(
                    "Local edits detected. Refusing to upgrade. Run 'sk sync-back <name>' or revert changes."
                );
            }
        }

        if needs_upgrade {
            plan.push((skill.install_name.clone(), dest.clone(), new_commit));
        }
    }

    if args.dry_run {
        for (name, _dest, new_commit) in &plan {
            if let Some(s) = targets.iter().find(|t| &t.install_name == name) {
                println!("{}: {} -> {}", name, &s.commit[..7], &new_commit[..7]);
            }
        }
        if all {
            print_skipped(&skipped_modified);
        }
        return Ok(());
    }

    if plan.is_empty() {
        if all {
            print_skipped(&skipped_modified);
        }
        return Ok(());
    }

    // Stage all planned changes into a temp dir; only after all succeed, atomically swap in
    let staging = TempDir::new_in(&project_root).context("create staging dir")?;
    let mut staged: Vec<(String, std::path::PathBuf, String, String)> = vec![]; // (name, staged_path, new_commit, new_digest)
    for (name, dest, new_commit) in &plan {
        let s = targets.iter().find(|t| t.install_name == *name).unwrap();
        let cache_dir = paths::resolve_or_primary_cache_path(
            &s.source.url,
            &s.source.host,
            &s.source.owner,
            &s.source.repo,
        );
        let staged_path = staging.path().join(name);
        fs::create_dir_all(&staged_path)?;
        install::extract_subdir_from_commit(
            &cache_dir,
            new_commit,
            &s.source.skill_path,
            &staged_path,
        )?;
        let new_digest = digest::digest_dir(&staged_path)?;
        staged.push((name.clone(), dest.clone(), new_commit.clone(), new_digest));
    }

    // Apply staged contents transactionally with rollback on failure
    let mut updates: Vec<(String, String, String)> = vec![];
    let mut applied: Vec<(String, std::path::PathBuf, std::path::PathBuf)> = vec![]; // (name, dest, backup)
    let simulate_exdev = std::env::var("SK_SIMULATE_EXDEV").ok().as_deref() == Some("1");
    let fail_after_first = std::env::var("SK_FAIL_AFTER_FIRST_SWAP").ok().as_deref() == Some("1");
    let mut apply_err: Option<anyhow::Error> = None;
    for (idx, (name, dest, new_commit, new_digest)) in staged.into_iter().enumerate() {
        let staged_path = staging.path().join(&name);
        let parent = match dest.parent() {
            Some(p) => p.to_path_buf(),
            None => {
                apply_err = Some(anyhow::anyhow!("no parent for dest"));
                break;
            }
        };
        let backup = parent.join(format!(".sk-upgrade-bak-{name}"));
        if backup.exists() {
            fs::remove_dir_all(&backup).ok();
        }
        if dest.exists() {
            fs::rename(&dest, &backup)
                .with_context(|| format!("backup {} -> {}", dest.display(), backup.display()))?;
        } else {
            fs::create_dir_all(&backup)?; // placeholder for rollback path
        }
        // Try direct rename staged -> dest
        let rename_res = if simulate_exdev {
            Err(std::io::Error::other("simulate EXDEV"))
        } else {
            fs::rename(&staged_path, &dest)
        };
        if let Err(_e) = rename_res {
            // Fallback: copy staged into a temp sibling and then rename into place
            let temp_sibling = parent.join(format!(".sk-upgrade-tmp-{name}"));
            if temp_sibling.exists() {
                fs::remove_dir_all(&temp_sibling).ok();
            }
            fs::create_dir_all(&temp_sibling)?;
            if let Err(e) = copy_dir_all(&staged_path, &temp_sibling) {
                // Restore backup for current item before breaking
                let _ = fs::remove_dir_all(&dest);
                let _ = fs::rename(&backup, &dest).or_else(|_| copy_dir_all(&backup, &dest));
                let _ = fs::remove_dir_all(&temp_sibling);
                apply_err = Some(e);
                break;
            }
            // Ensure dest does not exist (moved to backup already), then move temp into place
            if dest.exists() {
                fs::remove_dir_all(&dest).ok();
            }
            if let Err(e) = fs::rename(&temp_sibling, &dest)
                .with_context(|| format!("rename {} -> {}", temp_sibling.display(), dest.display()))
            {
                // Restore backup for current item before breaking
                let _ = fs::remove_dir_all(&dest);
                let _ = fs::rename(&backup, &dest).or_else(|_| copy_dir_all(&backup, &dest));
                let _ = fs::remove_dir_all(&temp_sibling);
                apply_err = Some(e);
                break;
            }
        }
        // Success for this target
        updates.push((name.clone(), new_commit, new_digest));
        applied.push((name.clone(), dest.clone(), backup.clone()));

        if fail_after_first && idx == 0 {
            apply_err = Some(anyhow::anyhow!("simulate apply failure after first swap"));
            break;
        }
    }
    // On failure, rollback prior changes and surface error
    if let Some(err) = apply_err {
        for (name, dest, backup) in applied.into_iter().rev() {
            let _ = fs::remove_dir_all(&dest);
            // Restore backup -> dest
            let _ = fs::rename(&backup, &dest).or_else(|_| {
                // rename failed; copy back recursively
                copy_dir_all(&backup, &dest)
            });
            // Cleanup backup dir if still present
            let _ = fs::remove_dir_all(&backup);
            let _ = name; // silence
        }
        // Ensure we do not mutate lockfile
        return Err(err);
    }
    // Success path: cleanup backups
    for (_name, _dest, backup) in &applied {
        let _ = fs::remove_dir_all(backup);
    }

    fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
        let fail_copy = std::env::var("SK_FAIL_COPY").ok().as_deref() == Some("1");
        let mut seen_files: u64 = 0;
        for entry in walkdir::WalkDir::new(src) {
            let entry = entry?;
            let path = entry.path();
            let rel = path.strip_prefix(src).unwrap();
            let target = dst.join(rel);
            if entry.file_type().is_dir() {
                fs::create_dir_all(&target)?;
            } else if entry.file_type().is_file() {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                // Simulate copy error after the first file to validate rollback path
                seen_files += 1;
                if fail_copy && seen_files == 1 {
                    return Err(anyhow::anyhow!("simulated copy failure"));
                }
                fs::copy(path, &target)
                    .with_context(|| format!("copy {} -> {}", path.display(), target.display()))?;
            } else if entry.file_type().is_symlink() {
                let link_target = std::fs::read_link(path)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs as unixfs;
                    if let Some(parent) = target.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    unixfs::symlink(&link_target, &target).with_context(|| {
                        format!("symlink {} -> {}", link_target.display(), target.display())
                    })?;
                }
                #[cfg(windows)]
                {
                    use std::os::windows::fs as winfs;
                    let real = path.parent().unwrap().join(&link_target);
                    if let Some(parent) = target.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    let created = if real.is_dir() {
                        winfs::symlink_dir(&link_target, &target).is_ok()
                    } else {
                        winfs::symlink_file(&link_target, &target).is_ok()
                    };
                    if !created {
                        if real.is_dir() {
                            copy_dir_all(&real, &target)?;
                        } else {
                            fs::copy(&real, &target).with_context(|| {
                                format!("copy {} -> {}", real.display(), target.display())
                            })?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    // Persist lockfile updates (and optional ref override)
    for (name, new_commit, new_digest) in &updates {
        if let Some(entry) = lf.skills.iter_mut().find(|s| s.install_name == *name) {
            entry.commit = new_commit.clone();
            entry.digest = new_digest.clone();
        }
    }
    lf.generated_at = Utc::now().to_rfc3339();
    crate::lock::save_lockfile(&lock_path, &lf)?;
    if all {
        print_skipped(&skipped_modified);
    }
    println!("Upgrade complete.");
    Ok(())
}

fn print_skipped(skipped: &[(String, Option<(String, String)>)]) {
    if skipped.is_empty() {
        return;
    }
    println!("Skipped {} skill(s) with local edits:", skipped.len());
    for (name, span) in skipped {
        match span {
            Some((old, new)) => println!(
                "- {name}: local edits plus upstream update ({} -> {}). Run 'sk sync-back {name}' or revert changes, then rerun 'sk upgrade {name}'.",
                short_sha(old),
                short_sha(new)
            ),
            None => println!(
                "- {name}: local edits (already at locked commit). Run 'sk sync-back {name}' or revert changes before upgrading."
            ),
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
