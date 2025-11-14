use crate::{config, git, lock, paths};
use anyhow::{anyhow, bail, Result};
use std::path::PathBuf;

pub(crate) struct SyncTarget {
    pub(crate) spec: git::RepoSpec,
    pub(crate) cache_dir: PathBuf,
    pub(crate) commit: String,
    pub(crate) skill_path: String,
    pub(crate) lock_index: Option<usize>,
}

pub(super) fn build_existing_target(entry: lock::LockSkill, index: usize) -> Result<SyncTarget> {
    let spec = entry.source.repo_spec_owned();
    let cache_dir =
        paths::resolve_or_primary_cache_path(&spec.url, &spec.host, &spec.owner, &spec.repo);
    git::ensure_cached_repo(&cache_dir, &spec)?;
    if !git::has_object(&cache_dir, &entry.commit)? {
        bail!(
            "locked commit {} missing in cache for {}/{}. Run 'sk update' or 'sk doctor --apply' first.",
            &entry.commit[..7],
            &spec.owner,
            &spec.repo
        );
    }
    Ok(SyncTarget {
        spec,
        cache_dir,
        commit: entry.commit.clone(),
        skill_path: entry.source.skill_path().to_string(),
        lock_index: Some(index),
    })
}

pub(super) fn build_new_target(
    repo_flag: Option<&str>,
    skill_path_flag: Option<&str>,
    installed_name: &str,
    https: bool,
    cfg: &config::UserConfig,
) -> Result<SyncTarget> {
    let repo_value = match repo_flag {
        Some(val) => val.to_string(),
        None => {
            let trimmed = cfg.default_repo.trim();
            if trimmed.is_empty() {
                return Err(anyhow!(
                    "skill '{}' not found in skills.lock.json. Provide --repo or set default_repo via 'sk config set default_repo <repo>'.",
                    installed_name
                ));
            }
            trimmed.to_string()
        }
    };
    let prefer_https = https || cfg.protocol.eq_ignore_ascii_case("https");
    let spec = git::parse_repo_input(&repo_value, prefer_https, &cfg.default_host)?;
    let cache_dir =
        paths::resolve_or_primary_cache_path(&spec.url, &spec.host, &spec.owner, &spec.repo);
    git::ensure_cached_repo(&cache_dir, &spec)?;
    let default_branch = git::detect_or_set_default_branch(&cache_dir, &spec)?;
    let commit = git::rev_parse(&cache_dir, &format!("refs/remotes/origin/{default_branch}"))?;
    let skill_path = skill_path_flag
        .map(normalize_skill_path)
        .unwrap_or_else(|| normalize_skill_path(installed_name));
    Ok(SyncTarget {
        spec,
        cache_dir,
        commit,
        skill_path,
        lock_index: None,
    })
}

fn normalize_skill_path(input: &str) -> String {
    let mut trimmed = input.trim();
    while let Some(rest) = trimmed.strip_prefix("./") {
        trimmed = rest;
    }
    trimmed = trimmed.trim_matches('/');
    if trimmed.is_empty() || trimmed == "." {
        ".".to_string()
    } else {
        trimmed.to_string()
    }
}
