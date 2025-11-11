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
