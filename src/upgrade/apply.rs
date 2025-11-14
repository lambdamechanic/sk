use super::fsops::copy_dir_all;
use super::plan::{SkippedUpgrade, StagedUpgrade, UpgradeTask};
use crate::{digest, install, lock};
use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use std::fs;
use std::path::Path;

pub fn stage_upgrades(staging_root: &Path, tasks: &[UpgradeTask]) -> Result<Vec<StagedUpgrade>> {
    let mut staged = Vec::new();
    for task in tasks {
        let staged_path = staging_root.join(&task.install_name);
        fs::create_dir_all(&staged_path)?;
        install::extract_subdir_from_commit(
            &task.cache_dir,
            &task.new_commit,
            &task.skill_path,
            &staged_path,
        )?;
        let new_digest = digest::digest_dir(&staged_path)?;
        staged.push(StagedUpgrade {
            task: task.clone(),
            staged_path,
            new_digest,
        });
    }
    Ok(staged)
}

pub fn apply_staged_upgrades(staged: &[StagedUpgrade]) -> Result<Vec<(String, String, String)>> {
    let simulate_exdev = std::env::var("SK_SIMULATE_EXDEV").ok().as_deref() == Some("1");
    let fail_after_first = std::env::var("SK_FAIL_AFTER_FIRST_SWAP").ok().as_deref() == Some("1");
    let mut updates = Vec::new();
    let mut applied: Vec<(String, std::path::PathBuf, std::path::PathBuf)> = Vec::new();

    for (idx, item) in staged.iter().enumerate() {
        let backup = apply_single(item, simulate_exdev)?;
        updates.push((
            item.task.install_name.clone(),
            item.task.new_commit.clone(),
            item.new_digest.clone(),
        ));
        applied.push((
            item.task.install_name.clone(),
            item.task.dest.clone(),
            backup,
        ));
        if fail_after_first && idx == 0 {
            rollback_applied(applied);
            return Err(anyhow!("simulate apply failure after first swap"));
        }
    }
    for (_, _, backup) in &applied {
        let _ = fs::remove_dir_all(backup);
    }
    Ok(updates)
}

fn apply_single(item: &StagedUpgrade, simulate_exdev: bool) -> Result<std::path::PathBuf> {
    let dest = &item.task.dest;
    let parent = dest
        .parent()
        .ok_or_else(|| anyhow!("no parent for dest {}", dest.display()))?;
    let backup = parent.join(format!(".sk-upgrade-bak-{}", item.task.install_name));
    if backup.exists() {
        fs::remove_dir_all(&backup).ok();
    }
    if dest.exists() {
        fs::rename(dest, &backup)
            .with_context(|| format!("backup {} -> {}", dest.display(), backup.display()))?;
    } else {
        fs::create_dir_all(&backup)?;
    }

    let stage_path = &item.staged_path;
    let rename_res = if simulate_exdev {
        Err(std::io::Error::other("simulate EXDEV"))
    } else {
        fs::rename(stage_path, dest)
    };
    if rename_res.is_err() {
        let temp_sibling = parent.join(format!(".sk-upgrade-tmp-{}", item.task.install_name));
        if temp_sibling.exists() {
            fs::remove_dir_all(&temp_sibling).ok();
        }
        fs::create_dir_all(&temp_sibling)?;
        if let Err(e) = copy_dir_all(stage_path, &temp_sibling) {
            let _ = fs::remove_dir_all(dest);
            let _ = fs::rename(&backup, dest).or_else(|_| copy_dir_all(&backup, dest));
            let _ = fs::remove_dir_all(&temp_sibling);
            return Err(e);
        }
        if dest.exists() {
            fs::remove_dir_all(dest).ok();
        }
        if let Err(e) = fs::rename(&temp_sibling, dest)
            .with_context(|| format!("rename {} -> {}", temp_sibling.display(), dest.display()))
        {
            let _ = fs::remove_dir_all(dest);
            let _ = fs::rename(&backup, dest).or_else(|_| copy_dir_all(&backup, dest));
            let _ = fs::remove_dir_all(&temp_sibling);
            return Err(e);
        }
    }
    Ok(backup)
}

fn rollback_applied(applied: Vec<(String, std::path::PathBuf, std::path::PathBuf)>) {
    for (_name, dest, backup) in applied.into_iter().rev() {
        let _ = fs::remove_dir_all(&dest);
        let _ = fs::rename(&backup, &dest).or_else(|_| copy_dir_all(&backup, &dest));
        let _ = fs::remove_dir_all(&backup);
    }
}

pub fn apply_updates_to_lockfile(
    lf: &mut lock::Lockfile,
    updates: &[(String, String, String)],
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    for (name, commit, digest_value) in updates {
        if let Some(entry) = lf.skills.iter_mut().find(|s| s.install_name == *name) {
            entry.commit = commit.clone();
            entry.digest = digest_value.clone();
            entry.installed_at = now.clone();
        } else {
            bail!("lockfile missing entry for {}", name);
        }
    }
    lf.generated_at = now;
    Ok(())
}

pub fn print_skipped(skipped: &[SkippedUpgrade]) {
    if skipped.is_empty() {
        return;
    }
    println!("Skipped {} skill(s) with local edits:", skipped.len());
    for entry in skipped {
        match &entry.span {
            Some(span) => println!(
                "- {name}: local edits plus upstream update ({} -> {}). Run 'sk sync-back {name}' or revert changes, then rerun 'sk upgrade {name}'.",
                short_sha(&span.current),
                short_sha(&span.available),
                name = entry.install_name
            ),
            None => println!(
                "- {name}: local edits (already at locked commit). Run 'sk sync-back {name}' or revert changes before upgrading."
                ,
                name = entry.install_name
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
