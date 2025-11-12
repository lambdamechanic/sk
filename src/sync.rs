use crate::{config, git, lock, paths};
use anyhow::{bail, Context, Result};
use chrono::Utc;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

pub struct SyncBackArgs<'a> {
    pub installed_name: &'a str,
    pub branch: Option<&'a str>,
    pub message: Option<&'a str>,
    pub root: Option<&'a str>,
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

    // Load lockfile and find matching entry
    let lock_path = project_root.join("skills.lock.json");
    if !lock_path.exists() {
        bail!("no lockfile found");
    }
    let data = fs::read(&lock_path).with_context(|| format!("reading {}", lock_path.display()))?;
    let lf: lock::Lockfile = serde_json::from_slice(&data).context("parse lockfile")?;
    let skill = lf
        .skills
        .iter()
        .find(|s| s.install_name == args.installed_name)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("skill not found: {}", args.installed_name))?;

    // Ensure cached repo exists and up-to-date enough for branching
    let cache_dir = paths::resolve_or_primary_cache_path(
        &skill.source.url,
        &skill.source.host,
        &skill.source.owner,
        &skill.source.repo,
    );
    let spec = git::RepoSpec {
        url: skill.source.url.clone(),
        host: skill.source.host.clone(),
        owner: skill.source.owner.clone(),
        repo: skill.source.repo.clone(),
    };
    git::ensure_cached_repo(&cache_dir, &spec)?;
    // Verify the locked commit exists in cache
    if !git::has_object(&cache_dir, &skill.commit)? {
        bail!(
            "locked commit {} missing in cache for {}/{}. Run 'sk update' or 'sk doctor --apply' first.",
            &skill.commit[..7],
            &skill.source.owner,
            &skill.source.repo
        );
    }

    // Determine branch name
    let branch_name = match args.branch {
        Some(b) => b.to_string(),
        None => default_branch_name(args.installed_name),
    };

    // Create a temporary worktree for the branch based at locked commit
    let wt_dir = TempDir::new().context("create worktree dir")?;
    let wt_path = wt_dir.path().to_path_buf();
    run(
        Command::new("git").args([
            "-C",
            &cache_dir.to_string_lossy(),
            "worktree",
            "add",
            "-b",
            &branch_name,
            wt_path.to_string_lossy().as_ref(),
            &skill.commit,
        ]),
        "git worktree add",
    )?;
    // Guard: ensure we always remove the worktree even on early errors
    let mut wt_guard = WorktreeGuard::new(cache_dir.clone(), wt_path.clone());

    // Rsync or copy installed dir into worktree skillPath
    let target_subdir = wt_path.join(&skill.source.skill_path);
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
        // Fallback: destructive copy by removing target then copying over
        if target_subdir.exists() {
            fs::remove_dir_all(&target_subdir)
                .with_context(|| format!("remove {}", target_subdir.display()))?;
        }
        fs::create_dir_all(&target_subdir)?;
        run(
            Command::new("bash").args([
                "-lc",
                &format!(
                    "cp -a '{}'/. '{}'",
                    dest_installed.display(),
                    target_subdir.display()
                ),
            ]),
            "copy contents",
        )?;
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
        let owner = &skill.source.owner;
        let repo = &skill.source.repo;
        println!("Pushed branch '{branch_name}' to origin for {owner}/{repo}.");
        println!("PR hint: gh pr create --fill --head {branch_name}");
    } else {
        let stderr = String::from_utf8_lossy(&push_out.stderr);
        let stdout = String::from_utf8_lossy(&push_out.stdout);
        let combined = format!("{stderr}{stdout}");
        bail!("git push failed: {}", combined.trim());
    }

    // Success: remove worktree now and disarm guard
    let _ = Command::new("git")
        .args([
            "-C",
            &cache_dir.to_string_lossy(),
            "worktree",
            "remove",
            "--force",
            wt_path.to_string_lossy().as_ref(),
        ])
        .status();
    wt_guard.disarm();

    Ok(())
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
