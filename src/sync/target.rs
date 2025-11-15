use crate::{config, git, paths};
use anyhow::Result;
use std::path::PathBuf;

pub(crate) struct SyncTarget {
    pub(crate) spec: git::RepoSpec,
    pub(crate) cache_dir: PathBuf,
    pub(crate) commit: String,
    pub(crate) skill_path: String,
    pub(crate) lock_index: Option<usize>,
}

pub(super) fn build_target_for_repo(
    repo_value: &str,
    skill_path_flag: Option<&str>,
    installed_name: &str,
    https: bool,
    cfg: &config::UserConfig,
    lock_index: Option<usize>,
) -> Result<SyncTarget> {
    let prefer_https = https || cfg.protocol.eq_ignore_ascii_case("https");
    let spec = git::parse_repo_input(repo_value, prefer_https, &cfg.default_host)?;
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
        lock_index,
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
