use crate::{config, digest, git, lock, paths};
use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::symlink;
#[cfg(windows)]
use std::os::windows::fs::{symlink_dir, symlink_file};
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;
use walkdir::WalkDir;

pub struct SyncBackArgs<'a> {
    pub installed_name: &'a str,
    pub branch: Option<&'a str>,
    pub message: Option<&'a str>,
    pub root: Option<&'a str>,
    pub repo: Option<&'a str>,
    pub skill_path: Option<&'a str>,
    pub https: bool,
}

struct SyncTarget {
    spec: git::RepoSpec,
    cache_dir: PathBuf,
    commit: String,
    skill_path: String,
    lock_index: Option<usize>,
}

pub fn run_sync_back(args: SyncBackArgs) -> Result<()> {
    // Locate project and lock entry for the installed skill
    let project_root = git::ensure_git_repo()?;
    let cfg = config::load_or_default()?;
    let install_root_rel = args.root.unwrap_or(&cfg.default_root);
    let install_root = paths::resolve_project_path(&project_root, install_root_rel);
    let dest_installed = install_root.join(args.installed_name);
    if !dest_installed.exists() {
        bail!(
            "installed dir missing for '{}'. Run 'sk doctor --apply' to rebuild first.",
            args.installed_name
        );
    }

    let lock_path = project_root.join("skills.lock.json");
    let mut lockfile = if lock_path.exists() {
        let data =
            fs::read(&lock_path).with_context(|| format!("reading {}", lock_path.display()))?;
        serde_json::from_slice::<lock::Lockfile>(&data).context("parse lockfile")?
    } else {
        lock::Lockfile::empty_now()
    };

    let existing_idx = lockfile
        .skills
        .iter()
        .position(|s| s.install_name == args.installed_name);
    let target = if let Some(idx) = existing_idx {
        let entry = lockfile.skills[idx].clone();
        build_existing_target(entry, idx)?
    } else {
        build_new_target(
            args.repo,
            args.skill_path,
            args.installed_name,
            args.https,
            &cfg,
        )?
    };

    // Determine branch name
    let branch_name = match args.branch {
        Some(b) => b.to_string(),
        None => default_branch_name(args.installed_name),
    };

    // Create a unique temp base and choose a non-existent child path for the worktree target.
    // Git requires the worktree path to not already exist.
    let _wt_base = TempDir::new().context("create temp base for worktree")?;
    let wt_path = _wt_base.path().join("wt");
    run(
        Command::new("git").args([
            "-C",
            &target.cache_dir.to_string_lossy(),
            "worktree",
            "add",
            "-b",
            &branch_name,
            wt_path.to_string_lossy().as_ref(),
            &target.commit,
        ]),
        "git worktree add",
    )?;
    // Guard: ensure we always remove the worktree even on early errors
    let mut wt_guard = WorktreeGuard::new(target.cache_dir.clone(), wt_path.clone());

    // Rsync or copy installed dir into worktree skillPath
    let target_subdir = wt_path.join(&target.skill_path);
    if let Some(parent) = target_subdir.parent() {
        fs::create_dir_all(parent).ok();
    }
    // Prefer rsync for accurate mirroring (including deletions)
    let use_rsync = which::which("rsync").is_ok();
    if use_rsync {
        // rsync -a --delete --exclude .git <installed>/. <worktree>/<skill_path>/
        fs::create_dir_all(&target_subdir)?;
        run(
            Command::new("rsync").args([
                "-a",
                "--delete",
                "--exclude",
                ".git",
                &format!(
                    "{}/",
                    dest_installed.to_string_lossy().trim_end_matches('/')
                ),
                &format!("{}/", target_subdir.to_string_lossy().trim_end_matches('/')),
            ]),
            "rsync contents",
        )?;
    } else {
        // Fallback: destructive copy when rsync is unavailable.
        // Special-case root-level skills (skill_path == ".") to avoid deleting the
        // worktree itself or attempting to remove '.'. Instead, purge only the
        // children of the worktree root while preserving VCS metadata like '.git'.
        let is_root = target.skill_path.trim().is_empty() || target.skill_path.trim() == ".";

        if target_subdir.exists() {
            if is_root {
                purge_children_except_git(&target_subdir)
                    .with_context(|| format!("purge children in {}", target_subdir.display()))?;
            } else {
                fs::remove_dir_all(&target_subdir)
                    .with_context(|| format!("remove {}", target_subdir.display()))?;
            }
        }

        // Ensure destination directory exists (no-op for root).
        fs::create_dir_all(&target_subdir)?;

        mirror_dir(&dest_installed, &target_subdir).with_context(|| {
            format!(
                "copy {} -> {}",
                dest_installed.display(),
                target_subdir.display()
            )
        })?;
    }

    // Commit changes
    run(
        Command::new("git").args(["-C", wt_path.to_string_lossy().as_ref(), "add", "-A"]),
        "git add",
    )?;
    let msg = args
        .message
        .map(|s| s.to_string())
        .unwrap_or_else(|| default_commit_message(args.installed_name));
    let commit_out = Command::new("git")
        .args([
            "-C",
            wt_path.to_string_lossy().as_ref(),
            "commit",
            "-m",
            &msg,
        ])
        .output()
        .context("spawn git commit failed")?;
    if !commit_out.status.success() {
        // Capture actual error from git and surface it. Treat classic "nothing to commit" as non-fatal.
        let stderr = String::from_utf8_lossy(&commit_out.stderr);
        let stdout = String::from_utf8_lossy(&commit_out.stdout);
        let combined = format!("{stderr}{stdout}");
        let lower = combined.to_lowercase();
        let no_changes = lower.contains("nothing to commit")
            || lower.contains("no changes added to commit")
            || lower.contains("nothing added to commit");

        if no_changes {
            println!(
                "No changes to commit for '{}': {}",
                args.installed_name,
                combined.trim()
            );
            // Remove worktree now so we can delete the local branch cleanly.
            let rm_status = Command::new("git")
                .args([
                    "-C",
                    &target.cache_dir.to_string_lossy(),
                    "worktree",
                    "remove",
                    "--force",
                    wt_path.to_string_lossy().as_ref(),
                ])
                .status();
            let mut removed = false;
            if let Ok(st) = rm_status {
                if st.success() {
                    wt_guard.disarm();
                    removed = true;
                } else {
                    eprintln!(
                        "Warning: git worktree remove failed (status {st}). Branch cleanup skipped; guard will retry on drop."
                    );
                }
            } else {
                eprintln!(
                    "Warning: failed to spawn 'git worktree remove'. Branch cleanup skipped; guard will retry on drop."
                );
            }
            if removed {
                let del = Command::new("git")
                    .args([
                        "-C",
                        &target.cache_dir.to_string_lossy(),
                        "branch",
                        "-D",
                        &branch_name,
                    ])
                    .status();
                if let Ok(st) = del {
                    if !st.success() {
                        eprintln!(
                            "Warning: failed to delete temp branch '{branch_name}' (status {st})."
                        );
                    }
                } else {
                    eprintln!("Warning: failed to spawn 'git branch -D {branch_name}'.");
                }
            }
            return Ok(());
        } else {
            bail!("git commit failed: {}", combined.trim());
        }
    }

    // Push branch
    let push_out = Command::new("git")
        .args([
            "-C",
            wt_path.to_string_lossy().as_ref(),
            "push",
            "-u",
            "origin",
            &branch_name,
        ])
        .output()
        .context("spawn git push failed")?;
    if push_out.status.success() {
        let owner = &target.spec.owner;
        let repo = &target.spec.repo;
        println!("Pushed branch '{branch_name}' to origin for {owner}/{repo}.");
        println!("PR hint: gh pr create --fill --head {branch_name}");
    } else {
        let stderr = String::from_utf8_lossy(&push_out.stderr);
        let stdout = String::from_utf8_lossy(&push_out.stdout);
        let combined = format!("{stderr}{stdout}");
        bail!(
            "git push failed: {}. You may not have write access; consider forking and repointing the source.",
            combined.trim()
        );
    }

    let head = git::rev_parse(&wt_path, "HEAD")?;

    // Success: attempt to remove worktree now; only disarm guard on success
    let rm_status = Command::new("git")
        .args([
            "-C",
            &target.cache_dir.to_string_lossy(),
            "worktree",
            "remove",
            "--force",
            wt_path.to_string_lossy().as_ref(),
        ])
        .status();
    if let Ok(st) = rm_status {
        if st.success() {
            wt_guard.disarm();
        } else {
            eprintln!(
                "Warning: git worktree remove failed (status {st}). Guard will retry on drop."
            );
        }
    } else {
        eprintln!("Warning: failed to spawn 'git worktree remove'. Guard will retry on drop.");
    }

    let digest = digest::digest_dir(&dest_installed)?;
    let entry = lock::LockSkill {
        install_name: args.installed_name.to_string(),
        source: lock::Source {
            url: target.spec.url.clone(),
            host: target.spec.host.clone(),
            owner: target.spec.owner.clone(),
            repo: target.spec.repo.clone(),
            skill_path: target.skill_path.clone(),
        },
        legacy_ref: None,
        commit: head,
        digest,
        installed_at: Utc::now().to_rfc3339(),
    };
    if let Some(idx) = target.lock_index {
        lockfile.skills[idx] = entry;
    } else {
        lockfile.skills.push(entry);
    }
    lockfile.generated_at = Utc::now().to_rfc3339();
    lock::save_lockfile(&lock_path, &lockfile)?;

    Ok(())
}

// Remove all direct children of `dir`, except entries named '.git'.
fn purge_children_except_git(dir: &std::path::Path) -> Result<()> {
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

fn build_existing_target(entry: lock::LockSkill, index: usize) -> Result<SyncTarget> {
    let spec = git::RepoSpec {
        url: entry.source.url.clone(),
        host: entry.source.host.clone(),
        owner: entry.source.owner.clone(),
        repo: entry.source.repo.clone(),
    };
    let cache_dir =
        paths::resolve_or_primary_cache_path(&spec.url, &spec.host, &spec.owner, &spec.repo);
    git::ensure_cached_repo(&cache_dir, &spec)?;
    if !git::has_object(&cache_dir, &entry.commit)? {
        bail!(
            "locked commit {} missing in cache for {}/{}. Run 'sk update' or 'sk doctor --apply' first.",
            &entry.commit[..7],
            &entry.source.owner,
            &entry.source.repo
        );
    }
    Ok(SyncTarget {
        spec,
        cache_dir,
        commit: entry.commit.clone(),
        skill_path: entry.source.skill_path.clone(),
        lock_index: Some(index),
    })
}

fn build_new_target(
    repo_flag: Option<&str>,
    skill_path_flag: Option<&str>,
    installed_name: &str,
    https: bool,
    cfg: &config::UserConfig,
) -> Result<SyncTarget> {
    let repo_input = repo_flag.ok_or_else(|| {
        anyhow!(
            "skill '{}' not found in skills.lock.json. Provide --repo to publish a new skill.",
            installed_name
        )
    })?;
    let spec = git::parse_repo_input(repo_input, https, &cfg.default_host)?;
    let cache_dir =
        paths::resolve_or_primary_cache_path(&spec.url, &spec.host, &spec.owner, &spec.repo);
    git::ensure_cached_repo(&cache_dir, &spec)?;
    let default_branch = git::detect_or_set_default_branch(&cache_dir, &spec.url)?;
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

fn mirror_dir(src: &Path, dest: &Path) -> Result<()> {
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

fn default_branch_name(name: &str) -> String {
    let sanitized = name
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '/' => c,
            _ => '-',
        })
        .collect::<String>();
    let ts = Utc::now().format("%Y%m%d-%H%M%S");
    format!("sk/sync/{}/{}", sanitized.trim_matches('/'), ts)
}

fn default_commit_message(name: &str) -> String {
    let ts = Utc::now().to_rfc3339();
    format!("sk sync-back: {name} ({ts})")
}

fn run(cmd: &mut Command, what: &str) -> Result<()> {
    let st = cmd.status().with_context(|| format!("spawn {what}"))?;
    if !st.success() {
        bail!("{what} failed");
    }
    Ok(())
}

struct WorktreeGuard {
    cache_dir: PathBuf,
    wt_path: PathBuf,
    active: bool,
}

impl WorktreeGuard {
    fn new(cache_dir: PathBuf, wt_path: PathBuf) -> Self {
        Self {
            cache_dir,
            wt_path,
            active: true,
        }
    }
    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for WorktreeGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let _ = Command::new("git")
            .args([
                "-C",
                &self.cache_dir.to_string_lossy(),
                "worktree",
                "remove",
                "--force",
                self.wt_path.to_string_lossy().as_ref(),
            ])
            .status();
    }
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

        // Create a fake .git directory and other entries
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join(".git").join("HEAD"), b"ref: refs/heads/main\n").unwrap();
        fs::create_dir_all(root.join("subdir")).unwrap();
        fs::write(root.join("file.txt"), b"hello").unwrap();

        purge_children_except_git(root).unwrap();

        // .git remains; others removed
        assert!(root.join(".git").exists());
        assert!(!root.join("subdir").exists());
        assert!(!root.join("file.txt").exists());
    }
}
