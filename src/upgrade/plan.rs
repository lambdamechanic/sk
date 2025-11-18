use super::UpgradeArgs;
use crate::{digest, git, install, lock, paths};
use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

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
pub struct RefreshTarget {
    pub install_name: String,
    pub new_commit: String,
    pub new_digest: String,
}

#[derive(Clone)]
pub struct UpgradeSpan {
    pub current: String,
    pub available: String,
}

#[derive(Clone)]
pub struct SkippedUpgrade {
    pub install_name: String,
    pub span: Option<UpgradeSpan>,
}

pub struct UpgradePlanResult {
    pub tasks: Vec<UpgradeTask>,
    pub skipped: Vec<SkippedUpgrade>,
    pub refreshes: Vec<RefreshTarget>,
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
) -> Result<UpgradePlanResult> {
    let mut plan = Vec::new();
    let mut skipped = Vec::new();
    let mut refreshes = Vec::new();
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
            let zero_diff_digest = if needs_upgrade {
                detect_zero_diff(
                    &dest,
                    &cache_dir,
                    skill.source.skill_path(),
                    &new_commit,
                    cur_digest.as_deref(),
                )?
            } else {
                None
            };
            if let Some(new_digest) = zero_diff_digest {
                refreshes.push(RefreshTarget {
                    install_name: skill.install_name.clone(),
                    new_commit: new_commit.clone(),
                    new_digest,
                });
                continue;
            }
            if allow_skip_dirty {
                let span = needs_upgrade.then(|| UpgradeSpan {
                    current: skill.commit.clone(),
                    available: new_commit.clone(),
                });
                skipped.push(SkippedUpgrade {
                    install_name: skill.install_name.clone(),
                    span,
                });
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
    Ok(UpgradePlanResult {
        tasks: plan,
        skipped,
        refreshes,
    })
}

fn detect_zero_diff(
    dest: &Path,
    cache_dir: &Path,
    skill_path: &str,
    new_commit: &str,
    current_digest: Option<&str>,
) -> Result<Option<String>> {
    let checkout = tempdir().context("create temporary directory for refresh comparison")?;
    install::extract_subdir_from_commit(cache_dir, new_commit, skill_path, checkout.path())
        .with_context(|| {
            format!(
                "extracting '{}' from {}",
                skill_path,
                &new_commit[..7]
            )
        })?;
    let output = Command::new("git")
        .arg("--no-pager")
        .arg("-c")
        .arg("core.autocrlf=false")
        .arg("diff")
        .arg("--no-index")
        .arg("--src-prefix=local/")
        .arg("--dst-prefix=remote/")
        .arg("--")
        .arg(dest)
        .arg(checkout.path())
        .output()
        .context("git diff --no-index failed to run")?;
    let no_diff = match output.status.code() {
        Some(0) => true,
        Some(1) => {
            let text = String::from_utf8_lossy(&output.stdout);
            text.trim().is_empty()
        }
        Some(code) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git diff exited with status {code}: {stderr}")
        }
        None => bail!("git diff terminated by signal"),
    };
    if !no_diff {
        return Ok(None);
    }
    let new_digest = if let Some(hash) = current_digest {
        hash.to_string()
    } else {
        digest::digest_dir(checkout.path())?
    };
    Ok(Some(new_digest))
}
