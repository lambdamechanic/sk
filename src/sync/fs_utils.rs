use super::target::SyncTarget;
use crate::install;
use anyhow::{Context, Result};
#[cfg(not(any(unix, windows)))]
use anyhow::bail;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::symlink;
#[cfg(windows)]
use std::os::windows::fs::{symlink_dir, symlink_file};
use std::path::Path;
use walkdir::WalkDir;

pub(crate) fn purge_children_except_git(dir: &Path) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            fs::remove_dir_all(&path).with_context(|| format!("remove {}", path.display()))?;
        } else {
            fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
        }
    }
    Ok(())
}

pub(crate) fn refresh_install_from_commit(
    target: &SyncTarget,
    dest: &Path,
    commit: &str,
) -> Result<()> {
    purge_children_except_git(dest)?;
    install::extract_subdir_from_commit(&target.cache_dir, commit, &target.skill_path, dest)
}

pub(crate) fn mirror_dir(src: &Path, dest: &Path) -> Result<()> {
    for entry in WalkDir::new(src).follow_links(false) {
        let entry = entry?;
        let rel = entry
            .path()
            .strip_prefix(src)
            .unwrap_or_else(|_| entry.path());
        if rel.as_os_str().is_empty() {
            continue;
        }
        let target = dest.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)
                .with_context(|| format!("create dir {}", target.display()))?;
        } else if entry.file_type().is_symlink() {
            copy_symlink(entry.path(), &target)?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("create parent {}", parent.display()))?;
            }
            fs::copy(entry.path(), &target).with_context(|| {
                format!("copy {} -> {}", entry.path().display(), target.display())
            })?;
        }
    }
    Ok(())
}

fn remove_existing(path: &Path) {
    let _ = fs::remove_file(path);
    let _ = fs::remove_dir_all(path);
}

#[cfg(unix)]
fn copy_symlink(src: &Path, dest: &Path) -> Result<()> {
    let target = fs::read_link(src).with_context(|| format!("read symlink {}", src.display()))?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create parent {}", parent.display()))?;
    }
    remove_existing(dest);
    symlink(target, dest).with_context(|| format!("create symlink {}", dest.display()))
}

#[cfg(windows)]
fn copy_symlink(src: &Path, dest: &Path) -> Result<()> {
    let target = fs::read_link(src).with_context(|| format!("read symlink {}", src.display()))?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create parent {}", parent.display()))?;
    }
    remove_existing(dest);
    let meta = fs::metadata(src);
    match meta {
        Ok(m) if m.is_dir() => symlink_dir(target, dest)
            .with_context(|| format!("create dir symlink {}", dest.display())),
        Ok(_) => symlink_file(target, dest)
            .with_context(|| format!("create file symlink {}", dest.display())),
        Err(_) => symlink_file(target, dest)
            .with_context(|| format!("create file symlink {}", dest.display())),
    }
}

#[cfg(not(any(unix, windows)))]
fn copy_symlink(src: &Path, _dest: &Path) -> Result<()> {
    bail!(
        "symlinks at {} are not supported on this platform",
        src.display()
    );
}

#[cfg(test)]
mod tests {
    use super::purge_children_except_git;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn purge_children_preserves_git_and_removes_others() {
        let td = tempdir().unwrap();
        let root = td.path();

        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join(".git").join("HEAD"), b"ref: refs/heads/main\n").unwrap();
        fs::create_dir_all(root.join("subdir")).unwrap();
        fs::write(root.join("file.txt"), b"hello").unwrap();

        purge_children_except_git(root).unwrap();

        assert!(root.join(".git").exists());
        assert!(!root.join("subdir").exists());
        assert!(!root.join("file.txt").exists());
    }
}
