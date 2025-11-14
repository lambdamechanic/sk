use crate::{git, lock};
use std::path::Path;

pub fn lock_entry_key(skill: &lock::LockSkill) -> String {
    let spec = skill.source.repo_spec();
    format!(
        "{}|{}|{}|{}|{}|{}|{}",
        skill.install_name,
        spec.host,
        spec.owner,
        spec.repo,
        skill.source.skill_path(),
        skill.commit,
        skill.digest
    )
}

pub fn compute_upstream_update(
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

pub fn short_sha(full: &str) -> &str {
    if full.len() > 7 {
        &full[..7]
    } else {
        full
    }
}
