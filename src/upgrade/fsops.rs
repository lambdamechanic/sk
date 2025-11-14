#[cfg(not(any(unix, windows)))]
use anyhow::bail;
use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::Path;

pub fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    let fail_copy = std::env::var("SK_FAIL_COPY").ok().as_deref() == Some("1");
    let mut seen_files: u64 = 0;
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry?;
        let path = entry.path();
        let rel = path.strip_prefix(src).unwrap();
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            seen_files += 1;
            if fail_copy && seen_files == 1 {
                return Err(anyhow!("simulated copy failure"));
            }
            fs::copy(path, &target)
                .with_context(|| format!("copy {} -> {}", path.display(), target.display()))?;
        } else if entry.file_type().is_symlink() {
            copy_symlink(path, &target)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn copy_symlink(src: &Path, dest: &Path) -> Result<()> {
    use std::os::unix::fs::symlink;
    let target = std::fs::read_link(src)?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let _ = fs::remove_file(dest);
    symlink(target, dest).with_context(|| format!("create symlink {}", dest.display()))
}

#[cfg(windows)]
fn copy_symlink(src: &Path, dest: &Path) -> Result<()> {
    use std::os::windows::fs::{symlink_dir, symlink_file};
    let target = std::fs::read_link(src)?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let _ = fs::remove_file(dest);
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
