use crate::paths;
use anyhow::Result;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub fn gather_cache_messages(referenced_caches: &HashSet<PathBuf>, apply: bool) -> Vec<String> {
    let mut cache_messages = Vec::new();
    let cache_root = paths::cache_root();
    if !cache_root.exists() {
        return cache_messages;
    }
    let walker = WalkDir::new(&cache_root)
        .min_depth(3)
        .max_depth(3)
        .into_iter()
        .filter_map(|entry| entry.ok());
    for entry in walker {
        if !entry.file_type().is_dir() {
            continue;
        }
        let repo_path = entry.into_path();
        if !repo_path.join(".git").exists() || referenced_caches.contains(&repo_path) {
            continue;
        }
        cache_messages.push(format!(
            "- Unreferenced cache clone: {}",
            repo_path.display()
        ));
        if apply {
            if let Err(e) = fs::remove_dir_all(&repo_path) {
                cache_messages.push(format!(
                    "  Failed to prune cache '{}': {}",
                    repo_path.display(),
                    e
                ));
            } else {
                cache_messages.push(format!(
                    "  Pruned unreferenced cache: {}",
                    repo_path.display()
                ));
                prune_empty_parents(&repo_path);
            }
        }
    }
    cache_messages
}

fn prune_empty_parents(repo_path: &Path) {
    if let Some(owner_dir) = repo_path.parent() {
        let _ = clean_if_empty(owner_dir.to_path_buf());
        if let Some(host_dir) = owner_dir.parent() {
            let _ = clean_if_empty(host_dir.to_path_buf());
        }
    }
}

fn clean_if_empty(dir: PathBuf) -> Result<()> {
    if dir.is_dir() && dir.read_dir()?.next().is_none() {
        fs::remove_dir_all(dir)?;
    }
    Ok(())
}
