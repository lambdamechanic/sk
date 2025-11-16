mod fs_utils;
mod pr;
mod target;

use crate::{config, digest, git, lock, paths};
use anyhow::{bail, Context, Result};
use chrono::Utc;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

use fs_utils::{mirror_dir, purge_children_except_git, refresh_install_from_commit};
use pr::{automate_pr_flow, maybe_wait_for_auto_merge, PrAutomationReport};
use target::{build_existing_target, build_new_target, SyncTarget};

pub struct SyncBackArgs<'a> {
    pub installed_name: &'a str,
    pub branch: Option<&'a str>,
    pub message: Option<&'a str>,
    pub root: Option<&'a str>,
    pub repo: Option<&'a str>,
    pub skill_path: Option<&'a str>,
    pub https: bool,
}

struct SyncSession<'a> {
    args: SyncBackArgs<'a>,
    dest_installed: PathBuf,
    lock_path: PathBuf,
    lockfile: lock::Lockfile,
    target: SyncTarget,
    branch_name: String,
    worktree_base: Option<TempDir>,
}

impl<'a> SyncSession<'a> {
    fn new(args: SyncBackArgs<'a>) -> Result<Self> {
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
        let lockfile = lock::Lockfile::load_or_empty(&lock_path)?;
        let lock_index = lockfile
            .skills
            .iter()
            .position(|s| s.install_name == args.installed_name);
        let target = if let Some(idx) = lock_index {
            build_existing_target(lockfile.skills[idx].clone(), idx)?
        } else {
            let repo_value = match args.repo {
                Some(raw) if !raw.trim().is_empty() => raw.trim().to_string(),
                _ => {
                    let trimmed = cfg.default_repo.trim();
                    if trimmed.is_empty() {
                        bail!(
                            "default_repo is not configured. Run 'sk config set default_repo <repo>' or pass --repo <target> when calling 'sk sync-back {}'.",
                            args.installed_name
                        );
                    }
                    trimmed.to_string()
                }
            };
            build_new_target(
                &repo_value,
                args.skill_path,
                args.installed_name,
                args.https,
                &cfg,
            )?
        };
        let branch_name = args
            .branch
            .map(|b| b.to_string())
            .unwrap_or_else(|| default_branch_name(args.installed_name));
        Ok(Self {
            args,
            dest_installed,
            lock_path,
            lockfile,
            target,
            branch_name,
            worktree_base: None,
        })
    }

    fn execute(mut self) -> Result<()> {
        let (wt_path, mut guard) = self.add_worktree()?;
        self.sync_installed_dir(&wt_path)?;
        let head = match self.commit_worktree(&wt_path, &mut guard)? {
            Some(head) => head,
            None => return Ok(()),
        };
        let pr_report = self.push_branch_and_maybe_open_pr(&wt_path)?;
        self.remove_worktree(&mut guard, &wt_path);

        let final_commit = self.finalize_commit(&head, pr_report.as_ref())?;
        if final_commit != head {
            println!(
                "Auto-merge landed additional upstream changes; refreshing '{}' to {}.",
                self.args.installed_name,
                short_sha(&final_commit)
            );
            self.refresh_install(&final_commit)?;
        }
        let digest = digest::digest_dir(&self.dest_installed)?;
        self.write_lock_entry(final_commit, digest)?;
        Ok(())
    }

    fn add_worktree(&mut self) -> Result<(PathBuf, WorktreeGuard)> {
        let base = TempDir::new().context("create temp base for worktree")?;
        let wt_path = base.path().join("wt");
        run(
            Command::new("git").args([
                "-C",
                &self.target.cache_dir.to_string_lossy(),
                "worktree",
                "add",
                "-b",
                &self.branch_name,
                wt_path.to_string_lossy().as_ref(),
                &self.target.commit,
            ]),
            "git worktree add",
        )?;
        self.worktree_base = Some(base);
        Ok((
            wt_path.clone(),
            WorktreeGuard::new(self.target.cache_dir.clone(), wt_path),
        ))
    }

    fn sync_installed_dir(&self, wt_path: &Path) -> Result<()> {
        let target_subdir = wt_path.join(&self.target.skill_path);
        if let Some(parent) = target_subdir.parent() {
            fs::create_dir_all(parent).ok();
        }
        let force_missing_rsync = env::var_os("SK_FORCE_RSYNC_MISSING").is_some();
        if !force_missing_rsync && which::which("rsync").is_ok() {
            fs::create_dir_all(&target_subdir)?;
            run(
                Command::new("rsync").args([
                    "-a",
                    "--delete",
                    "--exclude",
                    ".git",
                    &format!(
                        "{}/",
                        self.dest_installed.to_string_lossy().trim_end_matches('/')
                    ),
                    &format!("{}/", target_subdir.to_string_lossy().trim_end_matches('/')),
                ]),
                "rsync contents",
            )?
        } else {
            eprintln!(
                "Warning: 'rsync' not found; falling back to a recursive copy for '{}'. Install rsync for faster sync-back runs.",
                self.args.installed_name
            );
            let is_root =
                self.target.skill_path.trim().is_empty() || self.target.skill_path.trim() == ".";
            if target_subdir.exists() {
                if is_root {
                    purge_children_except_git(&target_subdir).with_context(|| {
                        format!("purge children in {}", target_subdir.display())
                    })?;
                } else {
                    fs::remove_dir_all(&target_subdir)
                        .with_context(|| format!("remove {}", target_subdir.display()))?;
                }
            }
            fs::create_dir_all(&target_subdir)?;
            mirror_dir(&self.dest_installed, &target_subdir).with_context(|| {
                format!(
                    "copy {} -> {}",
                    self.dest_installed.display(),
                    target_subdir.display()
                )
            })?;
        }
        Ok(())
    }

    fn commit_worktree(&self, wt_path: &Path, guard: &mut WorktreeGuard) -> Result<Option<String>> {
        run(
            Command::new("git").args(["-C", wt_path.to_string_lossy().as_ref(), "add", "-A"]),
            "git add",
        )?;
        let msg = self
            .args
            .message
            .map(|s| s.to_string())
            .unwrap_or_else(|| default_commit_message(self.args.installed_name));
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
        if commit_out.status.success() {
            let head = git::rev_parse(wt_path, "HEAD")?;
            if env::var_os("SK_TEST_GH_STATE_FILE").is_some() {
                env::set_var("SK_TEST_SYNC_BACK_HEAD_SHA", &head);
            }
            return Ok(Some(head));
        }
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
                self.args.installed_name,
                combined.trim()
            );
            self.cleanup_noop_branch(wt_path, guard);
            Ok(None)
        } else {
            bail!("git commit failed: {}", combined.trim());
        }
    }

    fn cleanup_noop_branch(&self, wt_path: &Path, guard: &mut WorktreeGuard) {
        let removed = match Command::new("git")
            .args([
                "-C",
                &self.target.cache_dir.to_string_lossy(),
                "worktree",
                "remove",
                "--force",
                wt_path.to_string_lossy().as_ref(),
            ])
            .status()
        {
            Ok(st) if st.success() => {
                guard.disarm();
                true
            }
            Ok(st) => {
                eprintln!(
                    "Warning: git worktree remove failed (status {st}). Branch cleanup skipped; guard will retry on drop."
                );
                false
            }
            Err(_) => {
                eprintln!(
                    "Warning: failed to spawn 'git worktree remove'. Branch cleanup skipped; guard will retry on drop."
                );
                false
            }
        };
        if removed {
            match Command::new("git")
                .args([
                    "-C",
                    &self.target.cache_dir.to_string_lossy(),
                    "branch",
                    "-D",
                    &self.branch_name,
                ])
                .status()
            {
                Ok(st) if st.success() => {}
                Ok(st) => eprintln!(
                    "Warning: failed to delete temp branch '{}' (status {st}).",
                    self.branch_name
                ),
                Err(_) => eprintln!(
                    "Warning: failed to spawn 'git branch -D {}'.",
                    self.branch_name
                ),
            }
        }
    }

    fn push_branch_and_maybe_open_pr(&self, wt_path: &Path) -> Result<Option<PrAutomationReport>> {
        let push_out = Command::new("git")
            .args([
                "-C",
                wt_path.to_string_lossy().as_ref(),
                "push",
                "-u",
                "origin",
                &self.branch_name,
            ])
            .output()
            .context("spawn git push failed")?;
        if !push_out.status.success() {
            let stderr = String::from_utf8_lossy(&push_out.stderr);
            let stdout = String::from_utf8_lossy(&push_out.stdout);
            let combined = format!("{stderr}{stdout}");
            bail!(
                "git push failed: {}. You may not have write access; consider forking and repointing the source.",
                combined.trim()
            );
        }
        println!(
            "Pushed branch '{}' to origin for {}/{}.",
            self.branch_name, self.target.spec.owner, self.target.spec.repo
        );
        match automate_pr_flow(wt_path, &self.branch_name, &self.target.spec) {
            Ok(report) => Ok(report),
            Err(err) => {
                eprintln!(
                    "Warning: failed to automate PR creation/merge for '{}': {err:#}",
                    self.branch_name
                );
                Ok(None)
            }
        }
    }

    fn remove_worktree(&self, guard: &mut WorktreeGuard, wt_path: &Path) {
        let rm_status = Command::new("git")
            .args([
                "-C",
                &self.target.cache_dir.to_string_lossy(),
                "worktree",
                "remove",
                "--force",
                wt_path.to_string_lossy().as_ref(),
            ])
            .status();
        if let Ok(st) = rm_status {
            if st.success() {
                guard.disarm();
            } else {
                eprintln!(
                    "Warning: git worktree remove failed (status {st}). Guard will retry on drop."
                );
            }
        } else {
            eprintln!("Warning: failed to spawn 'git worktree remove'. Guard will retry on drop.");
        }
    }

    fn finalize_commit(&self, head: &str, report: Option<&PrAutomationReport>) -> Result<String> {
        if let Some(info) = report {
            maybe_wait_for_auto_merge(&self.target, info, head, self.args.installed_name)
        } else {
            Ok(head.to_string())
        }
    }

    fn refresh_install(&self, commit: &str) -> Result<()> {
        refresh_install_from_commit(&self.target, &self.dest_installed, commit)
    }

    fn write_lock_entry(&mut self, final_commit: String, digest: String) -> Result<()> {
        self.lockfile.ensure_repo_entry(&self.target.spec);
        let entry = lock::LockSkill {
            install_name: self.args.installed_name.to_string(),
            source: lock::Source::new(self.target.spec.clone(), self.target.skill_path.clone()),
            legacy_ref: None,
            commit: final_commit,
            digest,
            installed_at: Utc::now().to_rfc3339(),
        };
        if let Some(idx) = self.target.lock_index {
            self.lockfile.skills[idx] = entry;
        } else {
            self.lockfile.skills.push(entry);
        }
        self.lockfile.generated_at = Utc::now().to_rfc3339();
        lock::save_lockfile(&self.lock_path, &self.lockfile)
    }
}

pub fn run_sync_back(args: SyncBackArgs) -> Result<()> {
    SyncSession::new(args)?.execute()
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

pub(super) fn short_sha(full: &str) -> &str {
    full.get(0..7).unwrap_or(full)
}
