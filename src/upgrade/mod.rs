mod apply;
mod fsops;
mod plan;

use crate::{config, git, lock, paths};
use anyhow::{bail, Context, Result};
use apply::{apply_staged_upgrades, apply_updates_to_lockfile, print_skipped, stage_upgrades};
use plan::{build_upgrade_plan, resolve_targets, UpgradePlanResult};
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
    let lf = lock::Lockfile::load(&lock_path)?;

    let targets = resolve_targets(&lf, &args)?;
    let upgrading_all = args.target == "--all";
    let UpgradePlanResult {
        tasks: plan,
        skipped: skipped_modified,
        refreshes,
    } = build_upgrade_plan(&targets, &install_root, upgrading_all)?;

    if args.dry_run {
        for task in &plan {
            if let Some(skill) = targets.iter().find(|s| s.install_name == task.install_name) {
                println!(
                    "{}: {} -> {}",
                    task.install_name,
                    &skill.commit[..7],
                    &task.new_commit[..7]
                );
            }
        }
        for refresh in &refreshes {
            println!(
                "{}: refresh lock to {} without rewiring files",
                refresh.install_name,
                &refresh.new_commit[..7]
            );
        }
        if upgrading_all {
            print_skipped(&skipped_modified);
        }
        return Ok(());
    }

    if plan.is_empty() && refreshes.is_empty() {
        if upgrading_all {
            print_skipped(&skipped_modified);
        }
        return Ok(());
    }

    let mut updates = Vec::new();
    if !plan.is_empty() {
        let staging = TempDir::new_in(&project_root).context("create staging dir")?;
        let staged = stage_upgrades(staging.path(), &plan)?;
        updates = apply_staged_upgrades(&staged)?;
    }

    updates.extend(refreshes.iter().map(|refresh| {
        (
            refresh.install_name.clone(),
            refresh.new_commit.clone(),
            refresh.new_digest.clone(),
        )
    }));
    lock::edit_lockfile(&lock_path, |lf| {
        apply_updates_to_lockfile(lf, &updates)?;
        Ok(())
    })?;

    if upgrading_all {
        print_skipped(&skipped_modified);
    }

    Ok(())
}
