use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub fn resolve_project_path(project_root: &Path, rel_or_abs: &str) -> PathBuf {
    let p = PathBuf::from(rel_or_abs);
    if p.is_absolute() {
        p
    } else {
        project_root.join(p)
    }
}

pub fn cache_root() -> PathBuf {
    // Allow tests to override with SK_CACHE_DIR
    if let Ok(override_dir) = std::env::var("SK_CACHE_DIR") {
        return PathBuf::from(override_dir).join("repos");
    }
    // ~/.cache/sk/repos
    if let Some(pd) = directories::ProjectDirs::from("", "", "sk") {
        pd.cache_dir().join("repos")
    } else {
        // Fallback to ~/.cache/sk/repos
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        home.join(".cache/sk/repos")
    }
}

pub fn cache_repo_path(host: &str, owner: &str, repo: &str) -> PathBuf {
    cache_root().join(host).join(owner).join(repo)
}

fn hashed_local_leaf(url: &str, repo: &str) -> Option<String> {
    if url.starts_with("file://") {
        let h = Sha256::digest(url.as_bytes());
        let hex = format!("{h:x}");
        let short = &hex[..12];
        Some(format!("{repo}-{short}"))
    } else {
        None
    }
}

pub fn resolve_or_primary_cache_path(url: &str, host: &str, owner: &str, repo: &str) -> PathBuf {
    // For local file:// sources, prefer a hashed leaf to avoid collisions; fall back to legacy path if present.
    if host == "local" {
        if let Some(leaf) = hashed_local_leaf(url, repo) {
            let hashed = cache_root().join(host).join(owner).join(leaf);
            if hashed.exists() {
                return hashed;
            }
            let legacy = cache_repo_path(host, owner, repo);
            if legacy.exists() {
                return legacy;
            }
            return hashed; // default clone target
        }
    }
    cache_repo_path(host, owner, repo)
}
