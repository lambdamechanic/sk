use super::UpgradeArgs;
use crate::{digest, git, lock, paths};
use anyhow::{bail, Result};
use std::path::Path;

#[derive(Clone)]
pub struct UpgradeTask {
    pub install_name: String,
    pub dest: std::path::PathBuf,
    pub cache_dir: std::path::PathBuf,
    pub skill_path: String,
    pub new_commit: String,
}

pub struct StagedUpgrade {
    pub task: UpgradeTask,
    pub staged_path: std::path::PathBuf,
    pub new_digest: String,
}

pub fn resolve_targets(lf: &lock::Lockfile, args: &UpgradeArgs) -> Result<Vec<lock::LockSkill>> {
    if args.target == "--all" {
        Ok(lf.skills.clone())
    } else {
        let matches: Vec<lock::LockSkill> = lf
            .skills
            .iter()
            .filter(|s| s.install_name == args.target)
            .cloned()
            .collect();
        if matches.is_empty() {
            bail!("skill not found: {}", args.target);
        }
        Ok(matches)
    }
}

pub fn build_upgrade_plan(
    targets: &[lock::LockSkill],
    install_root: &Path,
    allow_skip_dirty: bool,
) -> Result<(Vec<UpgradeTask>, Vec<(String, Option<(String, String)>)>)> {
    let mut plan = Vec::new();
    let mut skipped = Vec::new();
    for skill in targets {
        let dest = install_root.join(&skill.install_name);
        if !dest.exists() {
            bail!(
                "installed dir missing for '{}'. Run 'sk doctor --apply' to rebuild first.",
                skill.install_name
            );
        }
        let cur_digest = digest::digest_dir(&dest).ok();
        let is_modified = match &cur_digest {
            Some(hash) => hash != &skill.digest,
            None => true,
        };
        let spec = skill.source.repo_spec_owned();
        let cache_dir =
            paths::resolve_or_primary_cache_path(&spec.url, &spec.host, &spec.owner, &spec.repo);
        git::ensure_cached_repo(&cache_dir, &spec)?;
        let default = git::detect_or_set_default_branch(&cache_dir, &spec)?;
        let new_commit = git::rev_parse(&cache_dir, &format!("refs/remotes/origin/{default}"))?;
        let needs_upgrade = new_commit != skill.commit;

        if is_modified {
            if allow_skip_dirty {
                let span = needs_upgrade.then(|| (skill.commit.clone(), new_commit.clone()));
                skipped.push((skill.install_name.clone(), span));
                continue;
            } else {
                bail!(
                    "Local edits detected. Refusing to upgrade. Run 'sk sync-back <name>' or revert changes."
                );
            }
        }

        if needs_upgrade {
            plan.push(UpgradeTask {
                install_name: skill.install_name.clone(),
                dest,
                cache_dir,
                skill_path: skill.source.skill_path().to_string(),
                new_commit,
            });
        }
    }
    Ok((plan, skipped))
}
