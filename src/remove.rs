use crate::{config, digest, git, lock, paths};
use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use std::fs;

pub struct RemoveArgs<'a> {
    pub installed_name: &'a str,
    pub root: Option<&'a str>,
    pub force: bool,
}

pub fn run_remove(args: RemoveArgs) -> Result<()> {
    let project_root = git::ensure_git_repo()?;
    let cfg = config::load_or_default()?;
    let install_root_rel = args.root.unwrap_or(&cfg.default_root);
    let install_root = paths::resolve_project_path(&project_root, install_root_rel);

    let lock_path = project_root.join("skills.lock.json");
    if !lock_path.exists() {
        bail!("no lockfile found");
    }

    let removed_name = lock::edit_lockfile(&lock_path, |lf| {
        let idx = lf
            .skills
            .iter()
            .position(|s| s.install_name == args.installed_name)
            .ok_or_else(|| anyhow!("skill not found: {}", args.installed_name))?;
        let entry = lf.skills[idx].clone();
        let dest = install_root.join(&entry.install_name);
        if !dest.exists() {
            bail!(
                "installed dir missing for '{}'. Run 'sk doctor --apply' to rebuild first.",
                entry.install_name
            );
        }
        let digest_result = digest::digest_dir(&dest);
        let is_modified = match digest_result {
            Ok(cur) => cur != entry.digest,
            Err(e) => {
                if !args.force {
                    return Err(e)
                        .context(format!("failed to compute digest for '{}'", dest.display()));
                }
                true
            }
        };
        if is_modified && !args.force {
            bail!(
                "Local edits detected in '{}'. Refusing to remove. Use --force to override.",
                dest.display()
            );
        }
        fs::remove_dir_all(&dest).with_context(|| format!("remove {}", dest.display()))?;
        lf.skills.remove(idx);
        lf.generated_at = Utc::now().to_rfc3339();
        Ok(entry.install_name)
    })?;
    println!("Removed '{}'.", removed_name);
    Ok(())
}
