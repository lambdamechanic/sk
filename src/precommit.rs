use crate::{git, lock};
use anyhow::{bail, Context, Result};
use std::fs;

pub fn run_precommit(allow_local: bool) -> Result<()> {
    let project_root = git::ensure_git_repo()?;
    let lock_path = project_root.join("skills.lock.json");
    if !lock_path.exists() {
        // No lockfile; nothing to check.
        return Ok(());
    }
    let data = fs::read(&lock_path).with_context(|| format!("reading {}", lock_path.display()))?;
    let lf: lock::Lockfile = serde_json::from_slice(&data).context("parse lockfile")?;

    let mut local_entries: Vec<String> = vec![];
    for s in &lf.skills {
        let url = s.source.url.as_str();
        let is_local = url.starts_with("file://") || s.source.host == "local" || url.contains("localhost");
        if is_local {
            local_entries.push(format!(
                "{} -> {} (path: {})",
                s.install_name, url, s.source.skill_path
            ));
        }
    }

    if !local_entries.is_empty() {
        eprintln!("sk precommit: detected local (file:// or localhost) sources in skills.lock.json:");
        for e in &local_entries {
            eprintln!("  - {e}");
        }
        eprintln!(
            "These entries will not be usable by collaborators. Replace with ssh/https URLs, or run with --allow-local to bypass."
        );
        if !allow_local {
            bail!("local sources present; failing precommit");
        }
    }
    Ok(())
}

