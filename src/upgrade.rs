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

    // Preflight: compute plan and detect modified without mutating
    let mut plan: Vec<(String, std::path::PathBuf, String)> = vec![]; // (install_name, dest, new_commit)
    let mut any_modified = false;
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

        let effective_ref: Option<String> = if let Some(r) = args.r#ref {
            Some(r.to_string())
        } else {
            skill.ref_.clone()
        };
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
                    (git::rev_parse(&cache_dir, r)?, true)
                }
            }
        };

        if !(pinned && !args.include_pinned && args.r#ref.is_none()) && new_commit != skill.commit {
            plan.push((skill.install_name.clone(), dest.clone(), new_commit));
        }

        if is_modified {
            any_modified = true;
        }
    }

    if args.dry_run {
        for (name, _dest, new_commit) in &plan {
            if let Some(s) = targets.iter().find(|t| &t.install_name == name) {
                println!("{}: {} -> {}", name, &s.commit[..7], &new_commit[..7]);
            }
        }
        return Ok(());
    }

    if any_modified {
        bail!("Local edits detected. Refusing to upgrade. Run 'sk sync-back <name>' or revert changes.");
    }

    // Apply all planned changes, then update lockfile
    let mut updates: Vec<(String, String, String)> = vec![]; // (install_name, new_commit, new_digest)
    for (name, dest, new_commit) in plan {
        if dest.exists() {
            fs::remove_dir_all(&dest).with_context(|| format!("remove {}", dest.display()))?;
        }
        fs::create_dir_all(&dest)?;
        let s = targets.iter().find(|t| t.install_name == name).unwrap();
        let cache_dir = paths::cache_repo_path(&s.source.host, &s.source.owner, &s.source.repo);
        install::extract_subdir_from_commit(&cache_dir, &new_commit, &s.source.skill_path, &dest)?;
        let new_digest = digest::digest_dir(&dest)?;
        updates.push((name, new_commit, new_digest));
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
