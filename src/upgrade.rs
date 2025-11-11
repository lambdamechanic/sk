use crate::{config, digest, git, install, lock, paths};
use anyhow::{bail, Context, Result};
use chrono::Utc;
use std::fs;

pub struct UpgradeArgs<'a> {
    pub target: &'a str,        // installed name or "--all"
    pub r#ref: Option<&'a str>, // optional override ref
    pub root: Option<&'a str>,
    pub dry_run: bool,
    pub include_pinned: bool,
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
    let data = fs::read(&lock_path)?;
    let mut lf: lock::Lockfile = serde_json::from_slice(&data).context("parse lockfile")?;

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

    // We'll stage lockfile mutations here
    let mut updates: Vec<(String, String, String)> = vec![]; // (install_name, new_commit, new_digest)

    for skill in &targets {
        let dest = install_root.join(&skill.install_name);
        if !dest.exists() {
            bail!(
                "installed dir missing for '{}'. Run 'sk doctor --apply' to rebuild first.",
                skill.install_name
            );
        }
        // Determine state (modified vs clean)
        let cur_digest = digest::digest_dir(&dest).ok();
        let modified = match &cur_digest {
            Some(h) => h != &skill.digest,
            None => true,
        };

        // Ensure cache exists for this repo (do NOT fetch here; rely on 'sk update')
        let cache_dir =
            paths::cache_repo_path(&skill.source.host, &skill.source.owner, &skill.source.repo);
        if !cache_dir.exists() {
            let spec = git::RepoSpec {
                url: skill.source.url.clone(),
                host: skill.source.host.clone(),
                owner: skill.source.owner.clone(),
                repo: skill.source.repo.clone(),
            };
            git::ensure_cached_repo(&cache_dir, &spec)?;
        }

        // Resolve target commit according to ref precedence
        let effective_ref: Option<String> = if let Some(r) = args.r#ref {
            Some(r.to_string())
        } else {
            skill.ref_.clone()
        };

        // Determine new commit and whether ref is pinned (tag/SHA)
        let (new_commit, pinned): (String, bool) = match effective_ref.as_deref() {
            None => {
                let default = git::detect_or_set_default_branch(&cache_dir, &skill.source.url)?;
                let rev = format!("refs/remotes/origin/{default}");
                (git::rev_parse(&cache_dir, &rev)?, false)
            }
            Some(r) => {
                if let Ok(Some(_)) = git::remote_branch_tip(&cache_dir, r) {
                    let rev = format!("refs/remotes/origin/{r}");
                    (git::rev_parse(&cache_dir, &rev)?, false)
                } else {
                    // treat as tag or exact SHA
                    (git::rev_parse(&cache_dir, r)?, true)
                }
            }
        };

        if pinned && !args.include_pinned && args.r#ref.is_none() {
            // Skip pinned when not explicitly requested and no override passed
            continue;
        }

        if new_commit == skill.commit {
            // already up to date
            continue;
        }

        if args.dry_run {
            println!(
                "{}: {} -> {}",
                skill.install_name,
                &skill.commit[..7],
                &new_commit[..7]
            );
            continue;
        }

        if modified {
            bail!(
                "Local edits in 'skills/{}' detected (digest mismatch). Refusing to upgrade. Fork and repoint, or run 'sk sync-back {}' if you have push access.",
                skill.install_name,
                skill.install_name
            );
        }

        // Apply: replace contents with new commit's subdir
        if dest.exists() {
            fs::remove_dir_all(&dest).with_context(|| format!("remove {}", dest.display()))?;
        }
        fs::create_dir_all(&dest)?;
        install::extract_subdir_from_commit(
            &cache_dir,
            &new_commit,
            &skill.source.skill_path,
            &dest,
        )?;
        let new_digest = digest::digest_dir(&dest)?;
        updates.push((skill.install_name.clone(), new_commit, new_digest));
    }

    if args.dry_run {
        return Ok(());
    }

    if updates.is_empty() {
        println!("Nothing to upgrade.");
        return Ok(());
    }

    // Persist lockfile updates (and optional ref override)
    for (name, new_commit, new_digest) in updates {
        if let Some(entry) = lf.skills.iter_mut().find(|s| s.install_name == name) {
            if let Some(r) = args.r#ref {
                entry.ref_ = Some(r.to_string());
            }
            entry.commit = new_commit;
            entry.digest = new_digest;
        }
    }
    lf.generated_at = Utc::now().to_rfc3339();
    crate::lock::save_lockfile(&lock_path, &lf)?;
    println!("Upgrade complete.");
    Ok(())
}
